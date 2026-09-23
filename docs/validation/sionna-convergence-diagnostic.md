# Sionna CPU convergence support diagnostic

This is a bounded analysis helper for repeated ordinary Sionna `radio_map`
worker calls. It is a Phase 7 support diagnostic, not a new worker operation or
product acceptance result. It does not close Phase 7, establish P2 superiority
over P1 on held-out measurements, calibrate uncertainty, characterize
cross-host CPU/LLVM parity, or compute Kyberia Wi-Fi SINR/capacity.

## Use

With a provisioned, pinned CPU/LLVM worker runtime, call the helper with an
existing valid radio-map request, strictly increasing sample budgets, and at
least two distinct seeds:

```python
import sys

from rfatlas_sionna.convergence import run_sweep
from rfatlas_sionna.examples import request

report = run_sweep(request("radio_map"), [10_000, 50_000, 100_000],
                   [11, 29, 47], sys.executable)
```

The default runner delegates every point to the existing `client.run` path.
Each derived request preserves the complete base request except for its
deterministic request ID, `solver.samples`, and `solver.seed`. The regular
request/result contract validates the per-transmitter grid, shape, axis order,
cell centers, units, checksum, and finite nonnegative gains before statistics
are calculated. Runtime and result provenance are retained per run, including
the derived request, request/result hashes, process status/return code, runtime
pins, timestamps, log checksum, and resource limits.
Full raw gain maps are not copied into the report; their `data_sha256` values
are retained.

An optional event-like `cancel` token with `is_set()` may be passed to
`run_sweep`. The same token is forwarded to each existing `client.run` call;
the helper checks it before the first call, before and after each worker call,
and prevents subsequent requests after cancellation. A clean cancellation is
reported as `ConvergenceCancelled`; failed envelopes carrying cleanup or
cancel diagnostics are instead raised as `ConvergenceWorkerFailure` with the
original envelope attached. Exceptions raised by the client are propagated
unchanged, even when cancellation is simultaneously set. Each individual
worker request retains its original timeout and CPU limit.

For each sample budget the report contains the per-cell mean path gain and the
standard error across its distinct seed runs (sample standard deviation divided
by the square root of the replicate count). Adjacent-budget output contains the
signed cell-wise mean delta and symmetric relative magnitude
`abs(new-old)/max(abs(old),abs(new))`. Per-budget runtime summaries report min,
median, max, and sample standard deviation for the worker client's wall time.
There is no pass threshold and no claim that these numbers demonstrate
convergence; interpretation remains an explicit scientific review task.

The helper fails closed on invalid requests/options, worker failures,
contract-invalid or inconsistent results, runtime-provenance changes, and any
zero/no-data cell. No-data zeros are sentinels, not physical zero path gain, so
they are not included as observations in a mean. The report is bounded to 8
budgets, 8 seeds, 32 runs, 4096 cells per map, 100,000 summary scalars, 32
million aggregate transmitter-samples, and 8 MiB serialized output.

## Evidence boundary

`tests/test_sionna_convergence.py` uses constructed in-memory envelopes only to
exercise request mutation, contract rejection, aggregation, provenance
consistency, and resource bounds. These fixtures do not call or emulate the
Sionna engine and do not establish numerical worker behavior. The current
author worktree has no pinned `.venv-audited` Sionna runtime, so actual repeated
CPU/LLVM execution is not claimed. A future runtime gate must run this helper
in the pinned environment and retain its real inputs/results, then separately
provide held-out P1 comparison, convergence interpretation, cross-runtime
variance, and RF Atlas Wi-Fi composition evidence.

## Local author check

On 2026-09-23, `python3 -m unittest discover -s tests -p 'test_sionna_convergence.py' -v`
passed all 14 synthetic algorithm tests, including cancellation before a run,
token propagation during a run, stopping before the next run, and preserving
cleanup/containment diagnostics and client exceptions. They exercise the
ordinary-client dispatch seam through a synthetic runner; no worker process,
Sionna package, radio-map field result, or Phase 7 acceptance gate was run.
This is contract/algorithm evidence only. Source hashes are recorded in the
source-qualified ledger.
