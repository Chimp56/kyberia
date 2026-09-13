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

The queue remains bounded and is sized from the protocol's capped stdout and
stderr payloads plus bounded reader EOF, stdin completion and pipe-error
events. The reader chunk size is a named constant used by both the queue
sizing calculation and the reader. The active-process supervisor applies the
same bounded terminal-event accounting. No output cap, timeout, cancellation,
process ownership, or cleanup failure semantics are relaxed.

## Validation

Executed from the repository root with the existing pinned local Python
environment:

```text
PYTHONDONTWRITEBYTECODE=1 .tools/venv/bin/python -m unittest -v \
  tests.test_sionna_worker.LifecycleTests.test_windows_pipe_drain_handles_early_exit_and_exact_limit \
  tests.test_sionna_worker.LifecycleTests.test_windows_pipe_readers_fit_capped_output_without_consumer \
  tests.test_sionna_worker.LifecycleTests.test_windows_pipe_reader_stops_after_consumer_shutdown \
  tests.test_sionna_worker.LifecycleTests.test_windows_cleanup_reports_lingering_blocked_reader
PASS: 4 tests
```

The focused early-exit/exact-limit test passed in 100 repetitions before the
correction. A retained adversarial test that starts both capped readers
without a consumer fails with the former eight-event queue and passes with
the corrected queue, proving the backpressure condition independently of
runner timing. The complete Sionna worker suite passes 40 tests with two
platform-specific Windows execution tests skipped locally. The active-process
suite passes 38 tests with one native Windows test skipped. The complete root
Python suite passes 267 tests with 24 optional tests skipped.

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
* the cooperative cancellation test gives the suspended child a bounded
  startup margin to install its `SIGBREAK` handler before the request.

The focused correction suites pass locally: Sionna 40 tests with two native
Windows tests skipped, and active-process 38 tests with one native Windows test
skipped. These are contract results on macOS; they do not close the native
Windows runtime gate. The next hosted run must show the five named tests
passing and retain the descendant, cancellation, and cleanup assertions.

The corrected source evidence is content-addressed for review:

| Path | SHA-256 |
|---|---|
| `workers/sionna/rfatlas_sionna/client.py` | `a9828e90e9619b18a8378807407b77ceb65cf8b9e0a3bf0d913faf6a2c406a3f` |
| `tests/test_sionna_worker.py` | `316e3b5aee7216834677dfde2ce4f6387f77cef0a9c5a175d9b2d4ec8dd2c8db` |
| `research/active/process.py` | `f2052de6814e83954d4d74816e8221297a2256794f9c523232e8c06afc1fb3ef` |
| `tests/test_active_process.py` | `b251617b7b3897209352cb9414f89bb96d85466b26332d614e854ad2824b5e71` |
