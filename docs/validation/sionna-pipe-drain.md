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
runner timing. The complete Sionna worker suite passes 47 tests with two
platform-specific Windows execution tests skipped locally. The active-process
suite passes 38 tests with one native Windows test skipped. The complete root
Python suite passes 274 tests with 24 optional tests skipped.

Native Windows job-object and anonymous-pipe execution remains an external
runtime gate. It must be rerun on a supported Windows runner, including
descendant containment, cancellation, timeout, crash recovery, exact output
limits, and handle cleanup. The macOS and Linux hosted jobs must also complete
the replacement workflow before the CI validation status is advanced.

## Follow-up lifecycle correction

Foundation run
[34735594226](https://github.com/Chimp56/kyberia/actions/runs/34735594226)
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

The focused correction suites pass locally: Sionna 47 tests with two native
Windows tests skipped, and active-process 38 tests with one native Windows test
skipped. These are contract results on macOS; they do not close the native
Windows runtime gate. The next hosted run must show the five named tests
passing and retain the descendant, cancellation, engine-absence, CPU-limit,
pipe-quota and cleanup assertions.

The corrected source evidence is content-addressed for review:

| Path | SHA-256 |
|---|---|
| `workers/sionna/rfatlas_sionna/engine.py` | `9a4fecb24117a862e9772d4cd17818eb602e6578aec2a28d4ba25db1b7631345` |
| `workers/sionna/rfatlas_sionna/client.py` | `28aa8d4800660cc7d5ca10fe157769f290cd1ecf4be0770f615bb9c85d7a1154` |
| `tests/test_sionna_worker.py` | `977040da36e694bd434d0ad00a7a116ec49a469034ae22279d554f2906bd420f` |
| `research/active/process.py` | `fda7deefd72b357006534e40f36bef17ce843f857d3c6b0ab01721cf05752479` |
| `tests/test_active_process.py` | `50f7f9b52b4f9cf0642f4eb77f13be815b23a6a235a51e9529ec7e5dbe98ca34` |
