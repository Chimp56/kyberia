"""Fresh-process Gate C measurements; generated data and incomplete runs retained."""

import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path
import platform
import resource
import sqlite3
import subprocess
import sys
import time
from datetime import datetime, timezone

from engines import Store, file_hash, export_parquet, import_parquet
from model import generate, encode_row, digest_rows, BASE_NS


def timed(function):
    wall, cpu = time.perf_counter(), time.process_time()
    value = function()
    return value, {
        "wall_seconds": time.perf_counter() - wall,
        "cpu_seconds": time.process_time() - cpu,
    }


def json_result(value):
    return json.loads(json.dumps(value, default=lambda b: b.hex()))


def run_case(engine, count, root, seed=42, batch_size=4096):
    report = {
        "engine": engine,
        "rows": count,
        "seed": seed,
        "batch_size": batch_size,
        "capture_duration_seconds": (count - 1) * 25_000_001 / 1e9,
        "started_utc": datetime.now(timezone.utc).isoformat(),
    }
    store, report["create"] = timed(lambda: Store(root, engine, create=True))
    expected, batch_times, generation = hashlib.sha256(), [], 0.0
    iterator = iter(generate(count, seed, batch_size))
    while True:
        before = time.perf_counter()
        try:
            batch = next(iterator)
        except StopIteration:
            break
        generation += time.perf_counter() - before
        for row in batch:
            expected.update(encode_row(row))
        _, elapsed = timed(lambda: store.append(batch))
        batch_times.append(elapsed)
    report["generation_wall_seconds"] = generation
    report["ingest"] = {
        "batches": len(batch_times),
        "wall_seconds": sum(t["wall_seconds"] for t in batch_times),
        "cpu_seconds": sum(t["cpu_seconds"] for t in batch_times),
        "batch_wall_max_seconds": max(t["wall_seconds"] for t in batch_times),
        "batch_wall_p95_seconds": sorted(t["wall_seconds"] for t in batch_times)[
            int((len(batch_times) - 1) * 0.95)
        ],
    }
    report["ingest"]["rows_per_second"] = count / report["ingest"]["wall_seconds"]
    print(f"{engine} {count}: ingestion complete", file=sys.stderr, flush=True)
    _, report["close"] = timed(store.close)
    store, report["reopen"] = timed(lambda: Store(root, engine))
    actual, report["full_semantic_verification"] = timed(
        lambda: digest_rows(store.rows())
    )
    expected_result = {"rows": count, "semantic_sha256": expected.hexdigest()}
    if actual != expected_result:
        raise ValueError("full row parity failed")
    report["parity"] = actual
    report["queries"] = {}
    for scope in ("time", "spatial", "report"):
        trials, result = [], None
        for _ in range(3):
            result, elapsed = timed(
                lambda: store.aggregate(
                    scope,
                    BASE_NS + (count // 3) * 25_000_001,
                    BASE_NS + (count // 3 + max(1, count // 100)) * 25_000_001,
                )
            )
            trials.append(elapsed)
        report["queries"][scope] = {"trials": trials, "result": json_result(result)}
    print(
        f"{engine} {count}: queries and verification complete",
        file=sys.stderr,
        flush=True,
    )
    # Export all rows, then validate every typed value through the same schema.
    export_path = root / "portable-export.parquet"
    checksum, report["export"] = timed(
        lambda: export_parquet(store.rows(), export_path)
    )
    imported, report["export_import"] = timed(
        lambda: digest_rows(import_parquet(export_path, checksum, maximum_rows=count))
    )
    if imported != actual:
        raise ValueError("portable export parity failed")
    report["portable_export"] = {
        "sha256": checksum,
        "bytes": export_path.stat().st_size,
        **imported,
    }
    _, report["additive_metadata_migration"] = timed(store.migrate_metadata_v2)
    store.close()
    store = Store(root, engine)
    if digest_rows(store.rows()) != actual:
        raise ValueError("migration changed observation evidence")
    report["migration_preserves_evidence"] = "PASS"
    store.close()
    artifacts, physical, seen = [], 0, set()
    for path in sorted(root.iterdir()):
        if not path.is_file() or path == export_path:
            continue
        stat = path.stat()
        key = (stat.st_dev, stat.st_ino)
        if key not in seen:
            physical += stat.st_size
            seen.add(key)
        artifacts.append(
            {
                "name": path.name,
                "bytes": stat.st_size,
                "sha256": file_hash(path),
                "retained_temporary": path.suffix == ".pending",
            }
        )
    report["storage"] = {
        "unique_file_logical_bytes": physical,
        "files": artifacts,
        "excludes_portable_export": True,
        "hard_links_counted_once": True,
    }
    usage = resource.getrusage(resource.RUSAGE_SELF)
    report["peak_process_rss_bytes"] = (
        usage.ru_maxrss if sys.platform == "darwin" else usage.ru_maxrss * 1024
    )
    report["environment"] = {
        "python": sys.version,
        "platform": platform.platform(),
        "machine": platform.machine(),
        "sqlite_runtime": sqlite3.sqlite_version,
        "pyarrow": importlib.metadata.version("pyarrow"),
        "duckdb": importlib.metadata.version("duckdb"),
        "duckdb_threads": 1,
        "duckdb_memory_limit": "512MB",
        "parquet_compression": "zstd",
        "parquet_version": "2.6",
        "sqlite_journal": "DELETE",
        "sqlite_synchronous": "FULL",
        "cache_state": "OS cache not flushed; three query trials after full verification",
    }
    report["completed_utc"] = datetime.now(timezone.utc).isoformat()
    return report


def main():
    source_hashes = {
        p.name: file_hash(p) for p in sorted(Path(__file__).parent.glob("*.py"))
    }
    for package, expected in (("pyarrow", "25.0.1"), ("duckdb", "1.5.5")):
        if importlib.metadata.version(package) != expected:
            raise ValueError(f"benchmark requires pinned {package} {expected}")
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--data-root", type=Path, required=True)
    parser.add_argument(
        "--sizes", type=int, nargs="+", default=[10000, 100000, 1000000]
    )
    parser.add_argument("--case", choices=["sqlite", "parquet", "duckdb"])
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--batch-size", type=int, default=4096)
    args = parser.parse_args()
    if args.output.exists():
        raise FileExistsError("evidence output already exists; choose a new run")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.case:
        report = run_case(
            args.case, args.sizes[0], args.data_root, args.seed, args.batch_size
        )
    else:
        args.data_root.mkdir(parents=True, exist_ok=False)
        reports = []
        for count in args.sizes:
            for engine in ("sqlite", "parquet", "duckdb"):
                output = args.data_root / f"{engine}-{count}.json"
                subprocess.run(
                    [
                        sys.executable,
                        __file__,
                        "--case",
                        engine,
                        "--sizes",
                        str(count),
                        "--seed",
                        str(args.seed),
                        "--batch-size",
                        str(args.batch_size),
                        "--data-root",
                        str(args.data_root / f"{engine}-{count}"),
                        "--output",
                        str(output),
                    ],
                    check=True,
                    timeout=900,
                )
                reports.append(json.loads(output.read_text()))
        for count in args.sizes:
            group = [r for r in reports if r["rows"] == count]
            if len({r["parity"]["semantic_sha256"] for r in group}) != 1:
                raise ValueError("cross-engine semantic parity failed")
            for scope in ("time", "spatial", "report"):
                if any(
                    r["queries"][scope]["result"]
                    != group[0]["queries"][scope]["result"]
                    for r in group
                ):
                    raise ValueError("cross-engine query parity failed")
        report = {
            "schema": "kyberia.storage-research/1",
            "cases": reports,
            "cross_engine_parity": "PASS",
            "source_hashes": source_hashes,
        }
    if source_hashes != {
        p.name: file_hash(p) for p in sorted(Path(__file__).parent.glob("*.py"))
    }:
        raise ValueError(
            "research source changed during benchmark; rerun frozen source"
        )
    with args.output.open("x") as output:
        json.dump(report, output, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()
