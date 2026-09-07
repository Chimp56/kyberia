# Evidence-backed runtime validation

`tools/validation/gates.json` is the versioned, machine-readable acceptance catalog. Its 20 gate records contain individual required check IDs, evidence kinds, required software/hardware metadata, applicable plan requirements and repeatable procedure steps. Catalog status is always `NOT_RUN`: it describes obligations and makes no claim about the current machine. Generate it with `python3 tools/validation/generate_catalog.py`; the tests reject catalog drift.

The evidence checker is runnable today. It validates submitted acceptance evidence; it does not execute unfinished collector/worker implementations, auto-approve scientific conclusions, download dependencies, or run privileged capture. Each subsystem's acceptance driver and controlled physical setup must produce the evidence required by its procedure. Missing implementation remains `NOT_RUN`; it is not an external blocker.

From the repository root:

```sh
python3 tools/validation/runtime_gates.py list
python3 tools/validation/runtime_gates.py template sionna-cuda > /tmp/kyberia-sionna-cuda-result.json
python3 tools/validation/runtime_gates.py hash /tmp/acceptance.log
python3 tools/validation/runtime_gates.py check /tmp/kyberia-sionna-cuda-result.json
```

`template` emits an intentionally incomplete result. The final `check` exits **2**, with `valid: true, status: NOT_RUN`. Only a complete `PASS` record exits **0**; invalid evidence exits **1**, and any valid `FAIL`, `NOT_RUN`, or `BLOCKED_EXTERNAL` exits **2**. Callers must inspect both exit status and the JSON, never treat syntactic validity as runtime acceptance.

To complete a gate:

1. Select its catalog procedure and execute the relevant pinned implementation acceptance driver on the stated OS/backend/hardware. Save its exact argument vector, exit code, UTC time, operator, versions, device/driver/firmware details, numeric results, and redacted logs. Do not put credentials or raw private client identifiers in logs or command arguments.
2. Place acceptance artifacts under the result manifest's directory. Record each artifact's relative path and SHA-256 (`hash` prints it). Attach at least one supporting artifact to each executed check. Files must be nonempty, at most 16 MiB, and ordinary files inside that directory; symlinks and traversal are rejected. Large raw captures remain separately checksummed source artifacts referenced by a small reviewed result report.
3. Fill every individual check. `PASS` requires all checks passed, exact pinned upstream versions, the required hardware metadata, correct evidence kind, timestamp, command and exit 0. Archive the input/job/fixture hashes and numerical tolerance justification in the supporting report. Native scan and measured-field evidence require their real device/calibration context.
4. Have a reviewer independently inspect the report's contents and scientific meaning. A checksum proves bytes, not truth: the checker cannot detect a human falsely labeling a mock log as a hardware measurement. Review is mandatory before changing implementation traceability or release status.
5. Run `check RESULT.json`. Retain the result and its artifacts in the release evidence archive, and link that result from the implementation ledger. Do not edit catalog defaults to imply execution.

A runtime contract pass is different from a field/hardware pass:

| Evidence kind | What may satisfy it |
|---|---|
| `synthetic_contract` | Original fixture, schema and deterministic-replay tests |
| `runtime` | Actual pinned executable/worker/backend execution |
| `hardware_runtime` | Actual supported OS/device/radio/GPU/analyzer execution |
| `measured_field` | Consent-based measured data with calibrated reference and held-out evaluation |
| `behavioral` | Licensed competitor runs with permitted output and independent reference |
| `review` | Review of the actual package/SBOM/notices and distribution obligations |

The checker rejects a different evidence kind. Runtime Sionna CPU and hardware CUDA are separate gates; absent CUDA cannot block CPU work. The audited Sionna baseline is source `bc0549155c7b782c7614a0ec06a0ac4e32b979ae`, package `2.0.1`; Kismet is source `2d25ad004e9216ac963c4f156e9077331717959c`. These are plan pins, **not tested version claims**. Pin changes need source/license and numerical compatibility review. Kismet file parity additionally requires two distinct exact compatibility revisions before it can pass; only one audited baseline is presently specified.

For an external blocker, leave unaffected work runnable and set only the affected gate/result to `BLOCKED_EXTERNAL`. Add `external_blocker` with `category` (`hardware`, `credentials`, `legal`, `proprietary_data`, `os_access`, or `field_site`), `requirement`, exact `dependency`, observed `reason`, a runnable `resume_procedure`, and hashed `evidence: {path, sha256}`. For example, CUDA unavailability needs an actual capability probe/inventory report; inability to finish code is not evidence of unavailable hardware. Partial execution checks retain their own PASS/FAIL/NOT_RUN statuses and artifacts.

Kismet procedures cover authenticated source discovery; local/remote observations; time, channel, hopping/dwell, optional noise/per-chain, source identity; reconnect/hotplug/permission/errors; duplicates/drops/backpressure; malformed and unknown schemas; same-capture live/KismetDB/PCAPNG parity; read-only corruption handling; at least two-version upgrades; normalized replay without Kismet; benchmarks and distribution review.

Sionna procedures cover fresh pinned CPU/CUDA installations and upstream tests; canonical geometry/material/antenna transforms; per-transmitter path gain and radio maps; all interaction flags; deterministic seeds/convergence/tolerances; cancellation/timeout/crash/OOM/device loss; cache inputs and artifact integrity; remote authentication/round trips; desktop worker absence; measured holdouts and identifiability; and Kyberia-owned Wi-Fi SINR recomposition. Reflection/diffraction fixture geometry is available, but high-fidelity numerical baselines remain open until actual worker execution and independent validation.

Native, mobile, lab, spectrum, cross-platform, and licensed competitor gates retain the plan's separate OS access, physical calibration, pose-drift, active-path attribution, performance, and clean-room obligations. No gate in this initial catalog has been executed merely by adding its procedure. The harness's own test results establish only that it rejects missing or mismatched evidence.
