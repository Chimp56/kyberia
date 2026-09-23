# Independent rereview — Phase 7 convergence cancellation correction

**Candidate:** `f25667aa224a2c5113fab71fe5ba59d56defdac3`

**Base:** `7bdcda30f0eeaee425e870758cf78dce396a9c31`
**Verdict:** APPROVE WITH FOLLOW-UP / NOT APPROVED FOR INTEGRATION

## Ten-field handoff

1. **Objective and verdict:** Independently rereview the prior cancellation MAJOR only, plus any new cancellation, timeout, bound, or traceability BLOCKER/MAJOR in the correction. The original stop-on-cancel finding is resolved, but a new MAJOR means this candidate is not approved for integration.

2. **Plan and ledger anchors:** Checked `plan.md` §8.5, §16.13 Sionna gate, Gate I, Phase 7 deliverables/exit criteria, Appendix I `convergence_sweep`, and ledger records for `source:heading:66df6bddc47f2e0c0973:1`, `source:list_item:5269a81e5d04ce55d10d:1`, `source:list_item:5fa9c2753492416e5529:1`, `source:list_item:eb7936ab43f4560d972d:1`, `backlog:PREB-011:1`, and `audit:PRE-012:1`.

3. **Base and head:** Base `7bdcda30f0eeaee425e870758cf78dce396a9c31`; reviewed head `f25667aa224a2c5113fab71fe5ba59d56defdac3`, a direct descendant. The only candidate changes are `STATUS.md`, the implementation ledger, the validation note, the focused tests, and `workers/sionna/rfatlas_sionna/convergence.py`.

4. **Worktree and branch:** Reviewed in the assigned isolated worktree `/private/tmp/kyberia-phase7-convergence-rereview-20260923`, initially clean and detached at the candidate. No author or product worktree was edited.

5. **Inspected and changed paths:** Inspected the relevant plan sections, `AGENTS.md`, `workers/sionna/rfatlas_sionna/convergence.py`, `workers/sionna/rfatlas_sionna/client.py`, `tests/test_sionna_convergence.py`, `docs/validation/sionna-convergence-diagnostic.md`, `STATUS.md`, `docs/implementation/ledger.json`, `docs/implementation/TRACEABILITY.md`, the candidate diff, and the prior review at `17fefd3c8fa4740c6827f274689e9b874031475d`. This report is the only file changed by this review.

6. **Cancellation, timeout, and bound invariants / findings:** F1 from the first review is resolved: `run_sweep` now accepts an event-like token, passes that exact token to the existing client on each run, checks before dispatch and after return, and will not dispatch another run after observing cancellation (`convergence.py:226-273`). The three focused regression tests cover pre-cancel, cancellation during a call, and cancellation between runs. The existing per-request timeout and sweep preflight caps remain unchanged: 8 budgets, 8 seeds, 32 runs, 4,096 cells/map, 100,000 summary values, 32,000,000 aggregate transmitter-samples, and 8 MiB serialized output.

   **F2 — MAJOR, unresolved: cancellation masks worker cleanup/containment failure.** In `convergence.py:264-273`, every exception raised by the client is converted to `ConvergenceCancelled` whenever the caller token is set, and a returned envelope is discarded by `_check_cancel` at line 272 before its status or cleanup fields are examined. The existing client deliberately reports cancellation cleanup failures: `client.py:678-728` changes the status to `process_error` and returns `cleanup_error`/`cancel_error` when process termination, job closure, pipe drain, or thread cleanup fails; `client.py:830-845` exposes that as a failed envelope. Such errors can explicitly mean “Windows containment unknown” or that termination/reap failed. On POSIX, an `_stop` lifecycle exception can reach the helper's `except` block and be masked the same way. A caller therefore receives the ordinary `ConvergenceCancelled` signal even when the existing client says containment or cleanup is unconfirmed, losing the diagnostic needed to know a worker may remain live.

   **Exact reproduction:** This focused injected-client scenario reproduces the behavior:

   ```python
   from threading import Event
   from unittest.mock import patch
   from rfatlas_sionna.convergence import run_sweep
   from rfatlas_sionna.examples import request

   cancel = Event()
   def failed_cleanup(run_request, python, token):
       token.set()
       return {"schema_version": 1, "request_id": run_request["request_id"],
               "request_sha256": "0" * 64, "status": "failed",
               "error": "process_error", "cleanup_error": "Windows containment unknown",
               "cancel_error": "TerminateJobObject failed"}
   with patch("rfatlas_sionna.convergence.run_worker", side_effect=failed_cleanup):
       run_sweep(request("radio_map"), [10, 20], [1, 2], "unused", cancel=cancel)
   # Raises only: ConvergenceCancelled("sweep cancelled during a worker call")
   ```

   Neither cleanup field nor failure status reaches the caller. The existing client code confirms these are real result fields, not a speculative schema. Preserve lifecycle/cleanup failures when cancellation is set, and add a regression proving cancellation does not hide them.

7. **Tests and checks:** `python3 -m unittest discover -s tests -p 'test_sionna_convergence.py' -v` — 12 passed. `python3 tools/ledger.py check` — PASS (5,396 source blocks, 438 explicit ID occurrences, 447 headings). `git diff --check 7bdcda30f0eeaee425e870758cf78dce396a9c31..HEAD` — PASS. SHA-256 values for the helper, tests, and validation note match the implementation ledger. No repository Python formatter/configuration was found; no formatter was run.

8. **Status and traceability claims:** Phase 7, Gate I, PREB-011, PRE-012, and the delegated convergence obligations remain `IN_PROGRESS`; no Phase 7 exit, production acceptance, calibrated uncertainty, held-out P2/P1 result, or Wi-Fi SINR/capacity claim is made. The validation note correctly limits its evidence to synthetic tests and says no worker/Sionna execution occurred. The generated traceability entries continue to point at the diagnostic and its tests. No graph/MCP coverage claim is made.

9. **Report commit and cleanliness:** A report-only commit is recorded in the reviewer handoff. The reviewed candidate remains unchanged; the reviewer worktree is verified clean after committing this report.

10. **Remaining risks, limitations, and blockers:** F2 blocks integration until cleanup/containment errors survive cancellation and focused regressions cover both returned failure envelopes and raised lifecycle errors. No Sionna runtime or engine execution was performed; this review does not establish convergence, field accuracy, performance, or Phase 7 completion. Graph tools were unavailable, so no graph coverage was assessed.
