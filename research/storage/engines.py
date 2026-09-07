"""Research adapters only. No project-store or domain runtime dependency."""

from dataclasses import astuple, asdict
import hashlib
import os
from pathlib import Path
import sqlite3
import uuid

from model import Row, NAMES, ID_FIELDS, TEXT_FIELDS, batch_validate, VERSION


class Cancelled(Exception):
    pass


def checkpoint(hook, stage):
    if hook:
        hook(stage)


def file_hash(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def sync_file(path):
    with open(path, "rb") as stream:
        os.fsync(stream.fileno())


def sync_directory(path):
    # Windows directory durability requires the production native port.
    if os.name != "nt":
        descriptor = os.open(path, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)


def arrow_schema():
    import pyarrow as pa

    columns = []
    nullable = {
        "rssi_dbm",
        "noise_dbm",
        "source_version",
        "rssi_unknown",
        "noise_unknown",
        "source_version_unknown",
    }
    for name in NAMES:
        if name in ID_FIELDS:
            kind = pa.binary(16)
        elif name == "raw_sha256":
            kind = pa.binary(32)
        elif name in TEXT_FIELDS:
            kind = pa.string()
        elif name in ("utc_ns", "monotonic_ns", "frequency_hz"):
            kind = pa.int64()
        else:
            kind = pa.float64()
        columns.append(pa.field(name, kind, nullable=name in nullable))
    return pa.schema(
        columns,
        metadata={
            b"kyberia.research.storage": b"1",
            b"time_units": b"nanoseconds",
            b"coordinate_units": b"meters",
            b"signal_units": b"dBm",
        },
    )


def arrow_table(rows):
    import pyarrow as pa

    return pa.Table.from_pylist([asdict(r) for r in rows], schema=arrow_schema())


def import_parquet(path, expected_hash, maximum_rows=4096):
    """Bounded trusted-local research import; not a hostile-file sandbox."""
    import pyarrow.parquet as pq

    path = Path(path)
    if (
        path.is_symlink()
        or not path.is_file()
        or path.stat().st_size > 64 * 1024 * 1024
    ):
        raise ValueError("invalid Parquet file or byte budget")
    if file_hash(path) != expected_hash:
        raise ValueError("Parquet checksum mismatch")
    file = pq.ParquetFile(
        path, thrift_string_size_limit=1024 * 1024, thrift_container_size_limit=65536
    )
    if not file.schema_arrow.equals(arrow_schema(), check_metadata=True):
        raise ValueError("unexpected Parquet schema")
    if not 0 <= file.metadata.num_rows <= maximum_rows <= 1_000_000:
        raise ValueError("Parquet row budget")
    for batch in file.iter_batches(batch_size=4096, use_threads=False):
        rows = [Row(**r) for r in batch.to_pylist()]
        batch_validate(rows)
        yield from rows


def export_parquet(rows, path):
    import pyarrow.parquet as pq

    path = Path(path)
    if path.exists():
        raise FileExistsError(path)
    with pq.ParquetWriter(
        path, arrow_schema(), compression="zstd", version="2.6"
    ) as writer:
        batch = []
        for row in rows:
            batch.append(row)
            if len(batch) == 4096:
                batch_validate(batch)
                writer.write_table(arrow_table(batch))
                batch = []
        if batch:
            batch_validate(batch)
            writer.write_table(arrow_table(batch))
    sync_file(path)
    return file_hash(path)


class Store:
    def __init__(self, root, engine, create=False):
        if engine not in ("sqlite", "parquet", "duckdb"):
            raise ValueError("unknown engine")
        self.root, self.engine = Path(root), engine
        if create:
            self.root.mkdir(parents=True, exist_ok=False)
        name = "data.duckdb" if engine == "duckdb" else "data.sqlite"
        self.path = self.root / name
        if not create and not self.path.is_file():
            raise ValueError("missing database")
        if engine == "duckdb":
            import duckdb

            self.db = duckdb.connect(
                str(self.path),
                config={
                    "threads": "1",
                    "memory_limit": "512MB",
                    "enable_external_access": "false",
                    "allow_unsigned_extensions": "false",
                    "autoinstall_known_extensions": "false",
                    "autoload_known_extensions": "false",
                },
            )
        else:
            self.db = sqlite3.connect(self.path)
            self.db.execute("PRAGMA synchronous=FULL")
            self.db.execute("PRAGMA journal_mode=DELETE")
            self.db.execute("PRAGMA trusted_schema=OFF")
        if create:
            self.db.execute("CREATE TABLE format (version INTEGER NOT NULL)")
            self.db.execute("INSERT INTO format VALUES (?)", [VERSION])
            if engine == "parquet":
                self.db.execute(
                    "CREATE TABLE chunks (ordinal INTEGER PRIMARY KEY, filename TEXT UNIQUE NOT NULL, sha256 TEXT NOT NULL, rows INTEGER NOT NULL)"
                )
                self.db.execute(
                    "CREATE TABLE identities (id BLOB PRIMARY KEY, chunk INTEGER NOT NULL)"
                )
            else:
                types = []
                for n in NAMES:
                    typ = (
                        "BLOB"
                        if n in ID_FIELDS or n == "raw_sha256"
                        else "VARCHAR"
                        if n in TEXT_FIELDS
                        else "BIGINT"
                        if n in ("utc_ns", "monotonic_ns", "frequency_hz")
                        else "DOUBLE"
                    )
                    types.append(
                        f"{n} {typ}" + (" PRIMARY KEY" if n == "observation_id" else "")
                    )
                self.db.execute("CREATE TABLE observations (" + ",".join(types) + ")")
                self.db.execute("CREATE INDEX obs_time ON observations(utc_ns)")
                self.db.execute(
                    "CREATE INDEX obs_space ON observations(floor_id,x_m,y_m)"
                )
            if engine != "duckdb":
                self.db.commit()
        if self.db.execute("SELECT version FROM format").fetchall() not in (
            [(VERSION,)],
            [(2,)],
        ):
            self.close()
            raise ValueError("unsupported storage experiment schema")

    def migrate_metadata_v2(self, hook=None):
        """Experimental additive migration, not the Kyberia project migration.

        Raw rows/chunks remain unchanged. An annotation relation is added in the
        same transaction as the format version. Caller owns backup policy.
        """
        self.db.execute("BEGIN TRANSACTION")
        try:
            if self.db.execute("SELECT version FROM format").fetchall() != [(1,)]:
                raise ValueError("migration requires version one")
            self.db.execute(
                "CREATE TABLE annotations (observation_id BLOB PRIMARY KEY, note VARCHAR NOT NULL)"
            )
            checkpoint(hook, "migration_created")
            self.db.execute("UPDATE format SET version=2")
            checkpoint(hook, "migration_before_commit")
            self.db.execute("COMMIT")
        except BaseException:
            self.db.execute("ROLLBACK")
            raise

    def append(self, rows, hook=None):
        batch_validate(rows)
        checkpoint(hook, "validated")
        self.db.execute("BEGIN TRANSACTION")
        try:
            if self.engine == "parquet":
                ordinal = self.db.execute(
                    "SELECT coalesce(max(ordinal),-1)+1 FROM chunks"
                ).fetchone()[0]
                self.db.executemany(
                    "INSERT INTO identities VALUES (?,?)",
                    [(r.observation_id, ordinal) for r in rows],
                )
                pending = self.root / (str(uuid.uuid4()) + ".pending")
                checksum = export_parquet(iter(rows), pending)
                checkpoint(hook, "written")
                final = self.root / (checksum + ".parquet")
                # Never replace existing evidence. Orphans are retained.
                if final.exists():
                    if file_hash(final) != checksum:
                        raise ValueError("existing content address corrupt")
                else:
                    os.link(pending, final)
                sync_directory(self.root)
                checkpoint(hook, "published")
                self.db.execute(
                    "INSERT INTO chunks VALUES (?,?,?,?)",
                    (ordinal, final.name, checksum, len(rows)),
                )
            elif self.engine == "duckdb":
                table = arrow_table(rows)
                self.db.register("incoming_batch", table)
                try:
                    self.db.execute(
                        "INSERT INTO observations SELECT * FROM incoming_batch"
                    )
                finally:
                    self.db.unregister("incoming_batch")
            else:
                self.db.executemany(
                    "INSERT INTO observations VALUES ("
                    + ",".join("?" for _ in NAMES)
                    + ")",
                    [astuple(r) for r in rows],
                )
            checkpoint(hook, "before_commit")
            self.db.execute("COMMIT")
        except BaseException:
            self.db.execute("ROLLBACK")
            raise

    def chunks(self):
        for _, filename, checksum, rows in self.db.execute(
            "SELECT * FROM chunks ORDER BY ordinal"
        ).fetchall():
            if (
                filename != checksum + ".parquet"
                or len(checksum) != 64
                or any(c not in "0123456789abcdef" for c in checksum)
            ):
                raise ValueError("invalid chunk reference")
            yield self.root / filename, checksum, rows

    def rows(self):
        if self.engine == "parquet":
            for path, checksum, count in self.chunks():
                loaded = list(import_parquet(path, checksum))
                if len(loaded) != count:
                    raise ValueError("chunk row count mismatch")
                yield from loaded
        else:
            cursor = self.db.execute(
                "SELECT * FROM observations ORDER BY observation_id"
            )
            while batch := cursor.fetchmany(4096):
                yield from (Row(*r) for r in batch)

    def aggregate(self, scope="report", start=0, end=2**63 - 1):
        """Count/known count/min/max per floor; NULL never becomes zero dBm."""
        if scope not in ("report", "time", "spatial"):
            raise ValueError("unknown query")
        if self.engine != "parquet":
            where, args = "", []
            if scope == "time":
                where, args = " WHERE utc_ns >= ? AND utc_ns < ?", [start, end]
            elif scope == "spatial":
                where, args = (
                    " WHERE floor_id=? AND x_m>=? AND x_m<? AND y_m>=? AND y_m<?",
                    [(601).to_bytes(16, "big"), 20.0, 40.0, 30.0, 60.0],
                )
            return self.db.execute(
                "SELECT floor_id, count(*), count(rssi_dbm), min(rssi_dbm), max(rssi_dbm) FROM observations"
                + where
                + " GROUP BY floor_id ORDER BY floor_id",
                args,
            ).fetchall()
        import pyarrow.dataset as ds

        paths = [str(p) for p, _, _ in self.chunks()]
        if not paths:
            return []
        predicate = None
        if scope == "time":
            predicate = (ds.field("utc_ns") >= start) & (ds.field("utc_ns") < end)
        elif scope == "spatial":
            predicate = (
                (ds.field("floor_id") == (601).to_bytes(16, "big"))
                & (ds.field("x_m") >= 20.0)
                & (ds.field("x_m") < 40.0)
                & (ds.field("y_m") >= 30.0)
                & (ds.field("y_m") < 60.0)
            )
        # Dictionary-sized 4-floor reduction over a bounded streaming scanner.
        totals = {}
        scanner = ds.dataset(paths, format="parquet", schema=arrow_schema()).scanner(
            columns=["floor_id", "rssi_dbm"],
            filter=predicate,
            batch_size=4096,
            use_threads=False,
            batch_readahead=1,
            fragment_readahead=1,
        )
        for batch in scanner.to_batches():
            for floor, value in zip(
                batch.column(0).to_pylist(), batch.column(1).to_pylist()
            ):
                count, known, low, high = totals.get(floor, (0, 0, None, None))
                if value is not None:
                    known += 1
                    low = value if low is None else min(low, value)
                    high = value if high is None else max(high, value)
                totals[floor] = (count + 1, known, low, high)
        return [(floor, *totals[floor]) for floor in sorted(totals)]

    def close(self):
        self.db.close()
