"""Run the same bounded fixture through both proof implementations."""
from __future__ import annotations

import json
import hashlib
import os
import pathlib
import platform
import statistics
import subprocess
import sys
import time

from shapely_proof import source_hashes, source_revision


ROOT = pathlib.Path(__file__).resolve().parents[2]
GEOMETRY = ROOT / "research" / "geometry"
RESULTS = GEOMETRY / "results"
WORK = ROOT / ".tools" / "geometry-benchmark"
RUST_BINARY = GEOMETRY / "target" / "release" / "kyberia-geometry-proof"
PYTHON = ROOT / ".tools" / "geometry-venv" / "bin" / "python"
SHAPELY_SCRIPT = GEOMETRY / "shapely_proof.py"
ITERATIONS = 20


def run_case(name, command):
    internal = []
    process = []
    for iteration in range(ITERATIONS):
        output = WORK / f"{name}-{iteration:02d}.json"
        started = time.perf_counter()
        environment = os.environ.copy()
        environment["KYBERIA_GEOMETRY_SOURCE_REVISION"] = source_revision()
        subprocess.run([*map(str, command), str(output)], cwd=ROOT, check=True, env=environment)
        process.append((time.perf_counter() - started) * 1000.0)
        internal.append(json.loads(output.read_text())["elapsed_ms"])
    return {
        "iterations": ITERATIONS,
        "internal_timing_boundary": {
            "rust": "from before embedded fixture decode and bounded import through operations, before result serialization",
            "shapely": "from before fixture/raw GeoJSON file reads and decode through operations, before result serialization",
        }[name],
        "internal_elapsed_ms": {
            "min": min(internal),
            "median": statistics.median(internal),
            "max": max(internal),
        },
        "process_elapsed_ms": {
            "min": min(process),
            "median": statistics.median(process),
            "max": max(process),
        },
    }


def main():
    RESULTS.mkdir(parents=True, exist_ok=True)
    WORK.mkdir(parents=True, exist_ok=True)
    if not RUST_BINARY.is_file():
        raise SystemExit("build the desktop release proof before benchmarking")
    result = {
        "harness": "geometry-proof-benchmark",
        "fixture": "research/geometry/fixtures/geometry-proof.json",
        "fixture_sha256": source_hashes()["research/geometry/fixtures/geometry-proof.json"],
        "source_revision": source_revision(),
        "source_hashes": source_hashes(),
        "rust_binary_path": str(RUST_BINARY.relative_to(ROOT)),
        "rust_binary_sha256": hashlib.sha256(RUST_BINARY.read_bytes()).hexdigest(),
        "workload": "same bounded 2-D GeoJSON import (finite 2-D positions, CRS/Z rejection, cardinality/depth/count limits) + WKT validation, polygon booleans, rounded buffer, floor-filtered line intersection, hole clipping, and explicit repair diagnostics",
        "iterations": ITERATIONS,
        "environment": {"platform": platform.platform(), "python": sys.version, "machine": platform.machine()},
        "implementations": {
            "rust_geo": run_case("rust", [RUST_BINARY]),
            "geos_shapely": run_case("shapely", [PYTHON, SHAPELY_SCRIPT]),
        },
        "internal_timing_comparison": {"status": "DESCRIPTIVE_ONLY", "reason": "The measured operations and timer endpoints are aligned, but Rust consumes compile-time embedded bytes while Shapely reads files; internal medians are retained per backend and are not used as a direct performance ranking."},
        "comparison_note": "Process timings include startup and are retained for this invocation shape. Internal timings have aligned operation boundaries but remain descriptive because Rust uses embedded bytes and Shapely performs file reads. These are observations from one developer machine, not release thresholds.",
    }
    (RESULTS / "benchmark.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
