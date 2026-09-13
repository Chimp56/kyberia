# Sionna worker bounded Windows pipe drain

This increment hardens the Windows threaded pipe transport at the
`PropagationRequest` worker boundary described by plan §§8.15, 16.13 and
the `OSS-006`/`PREB-013`/`TST-007` backlog entries. It does not claim a native
Windows execution gate or a complete Sionna productionization gate.

## Failure and correction

Hosted Foundation run
[34735305250](https://github.com/Chimp56/kyberia/actions/runs/34735305250)
reached the Python regression suite on macOS but failed
`test_windows_pipe_drain_handles_early_exit_and_exact_limit`. The Windows
transport uses reader threads and a bounded event queue because anonymous
Windows pipes cannot be polled by the POSIX selector path. The previous queue
held eight 8 KiB events, so a scheduler pause could leave a full 1 MiB result
or its terminal EOF event blocked past the 0.5 second drain bound.

Each pipe now has its own bounded stdout/stderr data queue, sized from the
respective protocol cap, and an independent terminal queue for EOF, completion
and pipe-error events. A hostile stdout stream can therefore block only its own
data reader; it cannot consume stderr or terminal capacity. The reader chunk
size is a named constant used by both queue sizing calculations and readers.
The active-process supervisor applies the same per-stream terminal accounting.
Both consumers use fixed round-robin drain budgets, so a producer that refills
stdout continuously cannot postpone stderr, cancellation or timeout checks.
No output cap, timeout, cancellation, process ownership, or cleanup failure
semantics are relaxed.

## Validation

Executed from the repository root with the existing pinned local Python
environment:

```text
PYTHONDONTWRITEBYTECODE=1 .tools/venv/bin/python -m unittest -v \
  tests.test_sionna_worker.LifecycleTests.test_windows_pipe_drain_handles_early_exit_and_exact_limit \
  tests.test_sionna_worker.LifecycleTests.test_windows_hostile_stdout_cannot_starve_stderr_or_terminal_events \
  tests.test_sionna_worker.LifecycleTests.test_windows_pipe_reader_stops_after_consumer_shutdown \
  tests.test_sionna_worker.LifecycleTests.test_windows_cleanup_reports_lingering_blocked_reader
PASS: 4 tests
```

The focused early-exit/exact-limit test passed in 100 repetitions before the
correction. A retained adversarial test that starts both capped readers
without a consumer fails with the former eight-event queue and passes with
the corrected queue, proving the backpressure condition independently of
runner timing. The complete Sionna worker suite passes 52 tests with two
platform-specific Windows execution tests skipped locally. The active-process
suite passes 39 tests with one native Windows test skipped. An earlier pinned
environment run of the complete root Python suite passed 274 tests with 24
optional tests skipped. The current follow-up's system Python 3.9 run reached
269 tests with 26 skips but could not import `test_source_inventory` because
that environment lacks both `tomllib` and `tomli`; it is not recorded as a
complete root-suite pass. The current locked Python environment passes 280
tests with 24 optional tests skipped.

Native Windows job-object and anonymous-pipe execution remains an external
runtime gate. It must be rerun on a supported Windows runner, including
descendant containment, cancellation, timeout, crash recovery, exact output
limits, and handle cleanup. The macOS and Linux hosted jobs must also complete
the replacement workflow before the CI validation status is advanced.

## Follow-up lifecycle correction

Foundation run
[34736703085](https://github.com/Chimp56/kyberia/actions/runs/34736703085),
Windows job `103669296874`,
identified five Windows lifecycle failures after the earlier pipe correction:
cooperative Sionna cancellation, Sionna descendant containment, the Sionna
engine-absence fixture, active-process descendant containment, and the active
process zombie-group contract. The correction keeps all runtime assertions and
addresses their platform assumptions:

* the owned Windows Job Object is terminated even when the direct child has
  already exited, closing the descendant race before pipe cleanup;
* the Windows zombie fallback test explicitly models a child without an
  attached Kyberia job, matching a real `subprocess.Popen` instance;
* the engine-absence fixture selects `Scripts/python.exe` on Windows and
  `bin/python` on POSIX;
* the private engine defers the POSIX-only `resource` import and `setrlimit`
  call, so an engine-less Windows interpreter reaches the canonical
  `engine_unavailable` response;
* the cooperative cancellation test waits for a bounded localhost readiness
  handshake after the suspended child installs its `SIGBREAK` handler instead
  of racing a timer.
* the Windows Job Object configures `JOB_OBJECT_LIMIT_JOB_TIME` in 100-ns
  units before assigning and resuming the suspended child;
* the result envelope records requested CPU seconds separately from whether
  enforcement was confirmed, not confirmed, or unsupported. POSIX resource
  import failure is reported as unsupported rather than enforced.
* the Sionna supervisor handles Darwin's zombie-only process-group `EPERM`
  only after reaping its direct child and proving the group absent with an
  `ESRCH` probe, without risking a signal to a reused group ID.

The focused correction suites pass locally: Sionna 52 tests with two native
Windows tests skipped, and active-process 39 tests with one native Windows test
skipped. These are contract results on macOS; they do not close the native
Windows runtime gate. The next hosted run must show the five named tests
passing and retain the descendant, cancellation, engine-absence, CPU-limit,
pipe-quota and cleanup assertions.

The pipe-starvation and bounded round-robin contracts in both supervisors, the
exact-limit lifecycle contract and the zombie-group contract passed 600
repeated executions with zero failures, errors or skips. Independent patch
review and a hosted Windows rerun remain required before integration.

Independent review `92c8d345` identified that an exit-zero worker response
could be mistaken for CPU-limit confirmation when POSIX `setrlimit` had failed,
and that this document's ledger references needed to follow its final content.
The corrected protocol now requires an explicit typed acknowledgement for each
supported mechanism. The POSIX engine acknowledges `posix_setrlimit` only after
the call succeeds; import absence is `false`/`unsupported`, while `OSError` or
`ValueError` is `null`/`not_confirmed` and fails closed with
`resource_limit_unavailable` before backend execution. The Windows supervisor
acknowledges `windows_job_object` only after the configured job has successfully
assigned and resumed the suspended child. Process exit, decoded output and log
text are never treated as proof. Missing, mismatched or malformed
acknowledgements remain `null`/`not_confirmed`.

Correction rereview `018d209` found that acknowledgement shape alone did not
bind the claim to the process boundary that installed the limit. Authority is
now platform-specific: Windows accepts only the supervisor execution record
created after Job Object assignment and resume, while POSIX accepts only the
engine response created after `setrlimit`. A perfectly shaped claim from the
Windows worker or POSIX supervisor is ignored as `null`/`not_confirmed`.

The correction reran the complete Sionna suite (52 tests, two native Windows
skips), the complete active-process suite (39 tests, one native Windows skip),
and 100 repetitions of four adversarial acknowledgement and fair-drain tests
(400 tests total), all without failures. The authority correction additionally
passed 100 repetitions of the Windows worker-spoof, POSIX supervisor-spoof and
malformed/mismatched acknowledgement tests (300 tests). Native Windows evidence
is still limited to the failing hosted baseline run/job
`34736703085`/`103669296874`;
these corrected acknowledgements and the five baseline lifecycle cases require
a new hosted rerun before the gate can advance.

The corrected source evidence is content-addressed for review:

| Path | SHA-256 |
|---|---|
| `workers/sionna/rfatlas_sionna/engine.py` | `91fbe5270501ac1505853f70a364949971d6f66edb8b4b9581d493edb0ceffe3` |
| `workers/sionna/rfatlas_sionna/client.py` | `6157d299176592d019f1aafe055ab6a505db00072bf332366a6dcef9969919be` |
| `tests/test_sionna_worker.py` | `b7767b211f5f283eb999d22cf8f6bfd03a8a4ae079e662bed3d0510d50132e43` |
| `research/active/process.py` | `c04484875d4a7b82d54276e58fd9eb5b831ad95c4f15714c09ffa456c3f1e1d9` |
| `tests/test_active_process.py` | `1822082e9ff8772c4fff300481b0032254163743f9bfbcd4134d19536600e14f` |
