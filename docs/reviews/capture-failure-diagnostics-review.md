# Capture process failure diagnostics review

Candidate: `2f1fc3f`, based on `8c94379`.
Reviewer: primary integration agent, independent of the author.

The diagnostic-only patch is acceptable in scope: it evaluates each existing
call once, preserves every expected error variant and deadline, and includes
the actual result when an assertion fails. It changes no production behavior.
Integration is held pending investigation of the reproduced intermittent
failure; this review does not approve capture lifecycle correctness.

Root independently ran `cargo test -p kyberia-observation-pipeline --locked
--offline`: 33 unit tests passed, one real-collector test was ignored, and two
external-port tests passed. The ignored test requires the locally built signed
CoreWLAN collector and is not covered by these synthetic process fixtures.

The author's 15 repeated parallel package runs had two failures. The descendant
case in attempt 3 and escaped-descendant case in attempt 9 returned
`Err(Timeout)` instead of the expected `ProcessIo`. Logs are retained under the
candidate worktree's `.trash/review-logs/`, in
`observation-pipeline-repeat-parallel-20260908.log` and
`observation-pipeline-repeat-parallel-2-20260908.log`. The earlier malformed
fixture failure did not reproduce; its actual error remains unknown.

Required follow-up: establish whether collector startup, parent exit, or pipe
drain crosses the deadline. Distinguish a fixture precondition failure from a
production lifecycle defect with evidence. Do not accept either error
interchangeably merely to make the tests pass, or infer a fix from a single
successful run. Keep the timeout and bounded descendant-drain contracts tested
separately, with retained artifacts and unchanged failure provenance.

## Correction review

Combined candidates `2f1fc3f` and `c8ceb28`: **APPROVED** for test diagnostics
and fixture precondition separation. Root inspected the complete diffs and
independently ran the corrected package suite: 33 unit tests and two external
tests passed, with one real-collector test ignored. The author also reports
five passing parallel package repetitions.

Only the two descendant fixtures use a five-second typed command timeout;
their strict `ProcessIo` expectation and three-second elapsed bound remain.
Dedicated hang and cancellation cases retain their one-second commands.
Production supervision checks its global deadline before child-exit polling,
so the observed `Timeout` identifies that branch rather than post-exit drain
classification. Host scheduling/startup contention remains an inference,
not a traced cause. The earlier malformed-output failure remains unexplained;
its new diagnostic assertion is retained. This approval does not establish
universal freedom from host-load flakes or validate real-radio capture.
