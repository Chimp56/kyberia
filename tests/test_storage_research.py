"""Actual optional engine contracts; run with research/storage/.venv/bin/python.

Temporary artifacts intentionally retained per repository deletion policy.
"""

from dataclasses import replace
import importlib.util
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "research/storage"))
from model import generate, batch_validate, digest_rows  # noqa: E402
from engines import (  # noqa: E402
    Store,
    Cancelled,
    export_parquet,
    import_parquet,
    file_hash,
    arrow_table,
)

ENGINES = ("sqlite", "parquet", "duckdb")
AVAILABLE = all(importlib.util.find_spec(n) for n in ("pyarrow", "duckdb"))


class ModelTests(unittest.TestCase):
    def test_original_generator_determinism_and_missing_evidence(self):
        rows = next(generate(100))
        self.assertEqual(rows, next(generate(100)))
        self.assertNotEqual(rows, next(generate(100, seed=43)))
        self.assertIsNone(rows[0].rssi_dbm)
        self.assertEqual(rows[0].rssi_unknown, "not_observable")
        self.assertEqual(rows[1].utc_ns - rows[0].utc_ns, 25_000_001)

    def test_reject_malformed_projection(self):
        row = next(generate(1))[0]
        variants = [
            replace(row, observation_id=b""),
            replace(row, utc_ns=1.0),
            replace(row, utc_ns=2**63),
            replace(row, monotonic_ns=-1),
            replace(row, rssi_unknown=None),
            replace(row, rssi_dbm=-42.0),
            replace(row, noise_unknown="unknown"),
            replace(row, x_m=float("nan")),
            replace(row, x_m=float("inf")),
            replace(row, source_version="invented"),
            replace(row, raw_sha256=b""),
            replace(row, quality="measured"),
            replace(row, adapter_version=""),
            replace(row, adapter_version=None),
            replace(row, assignment_version=None),
            replace(row, floor_id=bytes(16)),
        ]
        for bad in variants:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                batch_validate([bad])
        for batch in ([], [row, row], [row] * 4097):
            with self.assertRaises(ValueError):
                batch_validate(batch)


@unittest.skipUnless(AVAILABLE, "optional real storage engines not installed")
class EngineTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="kyberia-storage-"))
        self.rows = next(generate(101))

    def store(self, engine, suffix=""):
        return Store(self.root / (engine + suffix), engine, create=True)

    def test_batch_commit_reopen_and_exact_parquet_export_roundtrip(self):
        expected = digest_rows(iter(self.rows))
        for engine in ENGINES:
            with self.subTest(engine=engine):
                store = self.store(engine)
                store.append(self.rows[:50])
                store.append(self.rows[50:])
                store.close()
                store = Store(self.root / engine, engine)
                self.assertEqual(digest_rows(store.rows()), expected)
                path = self.root / (engine + ".parquet")
                checksum = export_parquet(store.rows(), path)
                self.assertEqual(list(import_parquet(path, checksum)), self.rows)
                store.close()

    def test_unknown_aggregates_time_and_spatial_semantics_match_oracle(self):
        def oracle(rows):
            result = []
            for floor in sorted({r.floor_id for r in rows}):
                selected = [r for r in rows if r.floor_id == floor]
                known = [r.rssi_dbm for r in selected if r.rssi_dbm is not None]
                result.append(
                    (
                        floor,
                        len(selected),
                        len(known),
                        min(known) if known else None,
                        max(known) if known else None,
                    )
                )
            return result

        for engine in ENGINES:
            store = self.store(engine)
            store.append(self.rows)
            self.assertEqual(store.aggregate(), oracle(self.rows))
            start, end = self.rows[20].utc_ns, self.rows[70].utc_ns
            self.assertEqual(
                store.aggregate("time", start, end),
                oracle([r for r in self.rows if start <= r.utc_ns < end]),
            )
            self.assertEqual(
                store.aggregate("spatial"),
                oracle(
                    [
                        r
                        for r in self.rows
                        if r.floor_id == (601).to_bytes(16, "big")
                        and 20 <= r.x_m < 40
                        and 30 <= r.y_m < 60
                    ]
                ),
            )
            store.close()

    def test_all_unknown_is_not_zero(self):
        for engine in ENGINES:
            store = self.store(engine)
            store.append([self.rows[0]])
            self.assertEqual(
                store.aggregate(), [(self.rows[0].floor_id, 1, 0, None, None)]
            )
            store.close()

    def test_nanosecond_extremes_signed_position_and_finite_signal_roundtrip(self):
        edge = replace(
            self.rows[1],
            utc_ns=2**63 - 1,
            monotonic_ns=2**63 - 1,
            x_m=-123.125,
            z_m=-3.5,
            rssi_dbm=-0.125,
            source_version="original/évidence",
        )
        for engine in ENGINES:
            store = self.store(engine)
            store.append([edge])
            self.assertEqual(list(store.rows()), [edge])
            output = self.root / (engine + "-edges.parquet")
            checksum = export_parquet(store.rows(), output)
            self.assertEqual(list(import_parquet(output, checksum)), [edge])
            identical = self.root / (engine + "-edges-again.parquet")
            self.assertEqual(export_parquet(store.rows(), identical), checksum)
            store.close()

    def test_duplicate_batch_rolls_back_without_silently_discarding_new_row(self):
        for engine in ENGINES:
            store = self.store(engine)
            store.append([self.rows[0]])
            with self.assertRaises(Exception):
                store.append([self.rows[1], self.rows[0]])
            self.assertEqual(list(store.rows()), [self.rows[0]])
            store.close()

    def test_missing_required_provenance_rejects_whole_batch_on_every_engine(self):
        for engine in ENGINES:
            store = self.store(engine)
            store.append([self.rows[0]])
            for field in ("adapter_version", "assignment_version"):
                with self.subTest(engine=engine, field=field):
                    bad = replace(self.rows[2], **{field: None})
                    with self.assertRaises(ValueError):
                        store.append([self.rows[1], bad])
                    self.assertEqual(list(store.rows()), [self.rows[0]])
            store.close()

    def test_cancel_and_failure_preserve_committed_prefix(self):
        for engine in ENGINES:
            for exception in (Cancelled, OSError):
                stages = ("validated", "before_commit") + (
                    ("written", "published") if engine == "parquet" else ()
                )
                for stage in stages:
                    with self.subTest(engine=engine, failure=exception, stage=stage):
                        store = self.store(engine, stage + exception.__name__)
                        store.append([self.rows[0]])

                        def fail(current):
                            if current == stage:
                                raise exception("injected failure")

                        with self.assertRaises(exception):
                            store.append([self.rows[1]], hook=fail)
                        path = store.root
                        store.close()
                        store = Store(path, engine)
                        self.assertEqual(list(store.rows()), [self.rows[0]])
                        store.append([self.rows[1]])
                        self.assertEqual(list(store.rows()), self.rows[:2])
                        store.close()

    def test_chunk_corruption_missing_count_schema_and_path_rejected(self):
        for mutation in ("checksum", "count", "path", "missing", "schema"):
            store = self.store("parquet", mutation)
            store.append(self.rows)
            path, checksum, _ = next(store.chunks())
            if mutation == "checksum":
                with open(path, "ab") as stream:
                    stream.write(b"corrupt")
            elif mutation == "count":
                store.db.execute("UPDATE chunks SET rows=rows+1")
                store.db.commit()
            elif mutation == "path":
                store.db.execute("UPDATE chunks SET filename='../outside.parquet'")
                store.db.commit()
            elif mutation == "missing":
                path.rename(path.with_suffix(".retained"))
            else:
                import pyarrow as pa
                import pyarrow.parquet as pq

                bad = self.root / "wrong-schema.parquet"
                pq.write_table(pa.table({"rssi_dbm": [0.0]}), bad)
                with self.assertRaises(ValueError):
                    list(import_parquet(bad, file_hash(bad)))
                store.close()
                continue
            with self.assertRaises(ValueError):
                list(store.rows())
            store.close()

    def test_future_storage_version_and_unknown_engine_reject(self):
        for engine in ENGINES:
            store = self.store(engine)
            store.db.execute("UPDATE format SET version=999")
            if engine != "duckdb":
                store.db.commit()
            store.close()
            with self.assertRaises(ValueError):
                Store(self.root / engine, engine)
        with self.assertRaises(ValueError):
            self.store("made-up")

    def test_parquet_decoder_validates_values_after_native_decode(self):
        import pyarrow.parquet as pq

        for index, row in enumerate(
            (
                replace(self.rows[1], rssi_dbm=float("nan")),
                replace(self.rows[0], rssi_unknown=None),
                replace(self.rows[0], epoch_id=bytes(16)),
                replace(self.rows[0], adapter_version="x" * 129),
            )
        ):
            path = self.root / f"malformed-value-{index}.parquet"
            pq.write_table(arrow_table([row]), path, version="2.6")
            with self.assertRaises(ValueError):
                list(import_parquet(path, file_hash(path)))

    def test_parquet_byte_row_and_symlink_budgets(self):
        path = self.root / "budget.parquet"
        checksum = export_parquet(iter(self.rows), path)
        with self.assertRaises(ValueError):
            list(import_parquet(path, checksum, maximum_rows=100))
        with self.assertRaises(FileExistsError):
            export_parquet(iter(self.rows), path)
        oversized = self.root / "oversized.parquet"
        with oversized.open("wb") as stream:
            stream.truncate(64 * 1024 * 1024 + 1)
        with self.assertRaises(ValueError):
            list(import_parquet(oversized, "0" * 64))
        symlink = self.root / "symlink.parquet"
        try:
            symlink.symlink_to(path)
        except OSError:
            self.skipTest("symlink creation unavailable on this OS/permission context")
        with self.assertRaises(ValueError):
            list(import_parquet(symlink, checksum))

    def test_additive_migration_is_transactional_and_preserves_evidence(self):
        for engine in ENGINES:
            store = self.store(engine)
            store.append(self.rows)
            hashes = (
                [(p.name, file_hash(p)) for p, _, _ in store.chunks()]
                if engine == "parquet"
                else []
            )

            def fail(stage):
                raise OSError("injected migration interruption")

            with self.assertRaises(OSError):
                store.migrate_metadata_v2(hook=fail)
            self.assertEqual(
                store.db.execute("SELECT version FROM format").fetchall(), [(1,)]
            )
            store.migrate_metadata_v2()
            path = store.root
            store.close()
            store = Store(path, engine)
            self.assertEqual(list(store.rows()), self.rows)
            self.assertEqual(
                store.db.execute("SELECT version FROM format").fetchall(), [(2,)]
            )
            self.assertEqual(
                store.db.execute("SELECT * FROM annotations").fetchall(), []
            )
            if engine == "parquet":
                self.assertEqual(
                    [(p.name, file_hash(p)) for p, _, _ in store.chunks()], hashes
                )
            with self.assertRaises(ValueError):
                store.migrate_metadata_v2()
            store.close()

    def test_abrupt_process_exit_recovers_committed_prefix(self):
        for engine in ENGINES:
            stages = ("before_commit",) + (
                ("written", "published") if engine == "parquet" else ()
            )
            for stage in stages:
                with self.subTest(engine=engine, stage=stage):
                    store = self.store(engine, stage)
                    store.append([self.rows[0]])
                    path = store.root
                    store.close()
                    program = """
import os,sys
sys.path.insert(0, sys.argv[1])
from engines import Store
from model import generate
store = Store(sys.argv[2], sys.argv[3])
def die(stage):
    if stage == sys.argv[4]:
        os._exit(79)
store.append([next(generate(2))[1]], hook=die)
"""
                    result = subprocess.run(
                        [
                            sys.executable,
                            "-c",
                            program,
                            str(ROOT / "research/storage"),
                            str(path),
                            engine,
                            stage,
                        ],
                        capture_output=True,
                        timeout=30,
                    )
                    self.assertEqual(result.returncode, 79, result.stderr)
                    store = Store(path, engine)
                    self.assertEqual(list(store.rows()), [self.rows[0]])
                    store.append([self.rows[1]])
                    self.assertEqual(list(store.rows()), self.rows[:2])
                    store.close()


if __name__ == "__main__":
    unittest.main()
