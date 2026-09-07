# Isolated Sionna CPU proof

This is an optional Phase 0 research worker, not a supported P2 product tier. It
executes the actual Sionna PathSolver and RadioMapSolver on the LLVM CPU backend.
The core and desktop do not import Python or native engine objects. The only
implemented scene is original empty space with declared bounds, isotropic
single-element antennas and LOS. Bounds describe the evaluation extent; they do
not create walls, a floor, a ceiling, or an enclosed room.

## Audited source and reproducible installation

The authoritative pin is `NVlabs/sionna-rt@bc0549155c7b782c7614a0ec06a0ac4e32b979ae`,
version `2.0.1`. Its [package manifest](https://github.com/NVlabs/sionna-rt/blob/bc0549155c7b782c7614a0ec06a0ac4e32b979ae/pyproject.toml)
requires Python >=3.10 and exact Mitsuba 3.8.0/Dr.Jit 1.3.1. The tested environment
uses CPython 3.12.12, LLVM 18.1.8, macOS 26.6.2 ARM64, and the
`llvm_ad_mono_polarized` variant. [Dr.Jit's backend documentation](https://drjit.readthedocs.io/en/v1.3.1/what.html)
describes the separate LLVM dependency. Set `DRJIT_LIBLLVM_PATH` explicitly.

**Version strings alone are insufficient.** The PyPI 2.0.1 wheel differs from the
audited commit in `radio_materials/itu.py`, `radio_materials/itu_material.py`,
`radio_materials/radio_material.py`, and `utils/electromagnetics.py`. The
[source ledger](../licenses/sionna-sources.json) retains both hashes for each file,
the original wheel hash, and the exact-source resolution. This is source
equivalence evidence, not a security finding. The worker now checks all 49 audited
Python files before importing the engine and refuses that differing PyPI wheel.

The checked-in platform lock pins 45 installed Python distributions and wheel
hashes. Sionna is built from the exact archive with a fixed `SOURCE_DATE_EPOCH`.
Two independent builds produced the same wheel hash. Its relative wheel path in
`requirements.lock` is intentional: run installation from the repository root.
Other OS/Python architectures need separately reviewed wheel locks and runtime
evidence; this lock does not promise cross-platform installation.

With CPython 3.12.12 available, run from the repository root:

```sh
python3.12 -m venv workers/sionna/.venv-build
workers/sionna/.venv-build/bin/python -m pip install --require-hashes --only-binary=:all: -r workers/sionna/build-requirements.lock
mkdir -p .tools
curl -L --fail https://github.com/NVlabs/sionna-rt/archive/bc0549155c7b782c7614a0ec06a0ac4e32b979ae.tar.gz -o .tools/sionna-audited.tar.gz
workers/sionna/.venv-build/bin/python workers/sionna/build_wheel.py --archive .tools/sionna-audited.tar.gz --python workers/sionna/.venv-build/bin/python
python3.12 -m venv workers/sionna/.venv-audited
workers/sionna/.venv-audited/bin/python -m pip install --require-hashes --no-cache-dir -r workers/sionna/requirements.lock
```

`build_wheel.py` verifies the archive hash before extraction and wheel hash after
building. It leaves build directories in ignored `.tools/`; it does not clean
them. One documentation-only symlink in the upstream archive is excluded from
extraction; it is not needed by the wheel. The reviewed build uses pip 25.0.1 and
setuptools 80.9.0. An existing different wheel is preserved and causes failure.
The ledger includes installed metadata, license-file hashes, native library
hashes, and Homebrew LLVM/lz4/xz/zstd receipts. Python standalone bundled libraries,
packaged upstream assets and distribution notices still need release SBOM review.
Installed metadata does not confer rights to third-party scene assets.

## Process and protocol

`workers/sionna/worker.py` accepts one JSON document on stdin and writes one JSON
result on stdout. It starts one engine subprocess per request. A caller chooses
the interpreter using the trusted `--python` launch argument, never through
scene data. Capability requests have only `schema_version: 1`, a bounded
`request_id`, and `operation: "capabilities"`.

To construct a complete example without copying a stale checksum:

```sh
PYTHONPATH=workers/sionna python3 -c 'import json; from rfatlas_sionna.examples import request; print(json.dumps(request()))' > .tools/path-request.json
DRJIT_LIBLLVM_PATH=/opt/homebrew/opt/llvm@18/lib/libLLVM.dylib MPLCONFIGDIR=.tools/matplotlib python3 workers/sionna/worker.py --python workers/sionna/.venv-audited/bin/python < .tools/path-request.json
```

Operations are `capabilities`, `validate_scene`, `path_query`, and `radio_map`.
Use `request("radio_map")` for the original 4x4 grid. Unsupported fields,
operations, backend choices, materials and interaction flags fail closed. The
request contains a scene revision/hash, profile revision, explicit Hz/K/m units,
radio IDs, grid/receiver positions, seed, samples, depth, all interaction flags,
array/loop configuration, and limits. Scene hashes and request hashes use sorted,
ASCII JSON with compact separators and finite numbers. JSON numeric spelling
is significant (`1` and `1.0` produce different hashes); a future Rust client must
follow the byte convention or transport the exact canonical request bytes.

Inputs are capped at 64 KiB with a five-second input deadline, results at 1 MiB,
and logs at 64 KiB. Scene coordinates
are limited to +/-10 km; frequency is 1–10 GHz; bandwidth is 1–320 MHz. Jobs allow
1–4 transmitters, up to 16 point receivers or 4,096 cells, and at most 1,000,000
samples per transmitter. The wall deadline is 0.1–120 s and CPU limit 1–120 s.
Path separation and map-plane vertical separation must be at least 1 m for this
far-field proof. Solver depth is zero and all interactions except LOS are false.

The supervisor uses nonblocking pipes, drains output under fixed limits, kills
the dedicated process group on timeout/cancel/overflow, and reaps the worker.
The CLI maps SIGINT/SIGTERM to cancellation; Python callers may pass a
`threading.Event` to `run`. SIGKILL of the supervisor itself is not handled.
Crashes and missing dependencies return explicit failures with no prediction or
P0/P1 fallback. Each job uses a fresh process, so subsequent jobs can recover.
The CPU limit uses POSIX `RLIMIT_CPU`; this launcher is not yet a Windows worker.
There is **no hard memory quota or filesystem/network sandbox**. Sampling and
output are bounded, but full OOM containment and least-privilege grants remain
open Gate I/SEC-002 work. Dr.Jit uses `~/.drjit`; sandboxed execution can require
host permission for cache creation/writes. The runtime evidence retains cache
diagnostics instead of hiding them.

Results have correlated request/scene hashes, exact engine/source/backend
provenance, solver settings, timing, resource limits and output checksums.
Completed results require a Python 3.10+ three-component version, nonempty OS and
machine strings, and a 64-digit LLVM library SHA-256. The launcher must provide
`DRJIT_LIBLLVM_PATH`; an unspecified native build cannot produce a completed
result. These fields identify the reported environment; their presence is not
an independent runtime attestation. Point
coefficients use axes `[receiver, rx_antenna, transmitter, tx_antenna, path, time]`;
delay axes are `[receiver, transmitter, path]`; power axes omit path/time. Map
axes are `[transmitter, y, x]`, with explicit cell centers in meters. Result
validation checks identities, units, finite values, array shapes, coordinates,
no-data masks and checksums before exposing data. Zero sampled power is unknown,
not evidence of physical absence. No field uncertainty is asserted. No Sionna
generic SINR, Wi-Fi PHY, airtime, association, goodput or capacity is calculated.
In this restricted empty-space LOS contract, every delay must be strictly
positive and agree with transmitter/receiver distance divided by c (1e-5 relative
tolerance plus 2 mm/c absolute tolerance for float32 coordinates within +/-10 km).
This rule does not apply to future multipath or normalized-delay schemas.

## Executed evidence and remaining gates

The [CPU proof](../../workers/sionna/evidence/cpu-proof.json) preserves real
requests and subprocess results, with explicit seeds and independent analytic
comparisons. At 1/5/10 m and 2.4/5.2/6.5 GHz, path power and absolute delay are
checked against Friis and distance/c. The path tolerance is 1e-5 relative,
appropriate to single-precision numerical validation in empty space; it is not
a building prediction accuracy claim. Maps compare each cell with independent
100x100 midpoint integration of Friis power over its area. The 5% relative
tolerance applies at 100,000 samples. The 10,000-sample case is deliberately
retained as an undersampling diagnostic after one cell missed that threshold.
LOS map outputs were unchanged across seeds 42/43/44 in this scene; this does not
establish stochastic multipath convergence or backend parity.

```sh
python3 -m unittest discover -s tests -p test_sionna_worker.py -v
DRJIT_LIBLLVM_PATH=/opt/homebrew/opt/llvm@18/lib/libLLVM.dylib MPLCONFIGDIR=.tools/matplotlib workers/sionna/.venv-audited/bin/python workers/sionna/acceptance.py --output workers/sionna/evidence/cpu-proof.json
DRJIT_LIBLLVM_PATH=/opt/homebrew/opt/llvm@18/lib/libLLVM.dylib MPLCONFIGDIR=.tools/matplotlib workers/sionna/.venv-audited/bin/python workers/sionna/upstream_tests.py --upstream .tools/upstream/sionna-rt-bc0549155c7b782c7614a0ec06a0ac4e32b979ae --output workers/sionna/evidence/upstream-subset.json
```

The last command requires the audited archive extracted under ignored `.tools/upstream`.
The unmodified [upstream subset](../../workers/sionna/evidence/upstream-subset.json)
covers the CIR suite and LOS radio-map test. It is not the full upstream suite.
The early [PyPI undersampling failure](../../workers/sionna/evidence/initial-low-sample-failure.json)
is historical diagnostic evidence from the package that failed source equivalence;
the final CPU proof uses the exact audited source.

All repository `sionna-*` aggregate runtime gates remain open: neither a schema
test nor this narrow runtime proof fulfills their complete check lists. CUDA is
unavailable on this host (actual backend probe false and no compiled CUDA
variant), independently of CPU progress. Reflection/refraction/diffraction,
materials/frequency updates/thin-wall limitations, geometry/antenna transforms,
multi-floor scenes, representative performance, hard memory limits/OOM,
remote artifacts, native product integration and measured holdouts remain
unvalidated. Source-qualified traceability is §8.15, §16.13/Sionna, §17/Phase 0,
§18.7/PREB-010–013, §18.11/OSS-004–006 and OSS-011, §20/Gate I,
Appendix I/PRE-007–009 and TST-003/006/007/008. Root integration owns STATUS and
the implementation ledger; this change must not mark those product features done.
