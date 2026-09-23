# Independent rereview — Phase 7 convergence cancellation and failure handling

**Candidate:** `432427520c957aa2b5a608a4782b6d836f09a0c3`

**Correction base:** `f25667aa224a2c5113fab71fe5ba59d56defdac3`

**Original feature:** `7bdcda30f0eeaee425e870758cf78dce396a9c31`
**Verdict:** APPROVE WITH FOLLOW-UP — F1/F2 are resolved in this correction; reconcile the diverged mainline documentation/ledger before integration.

## Ten-field handoff

1. **Objective and verdict:** Independently assess the cancellation propagation MAJOR F1 and cleanup-diagnostic masking MAJOR F2, plus newly introduced cancellation/failure-classification, dispatch, timeout, bound, or traceability BLOCKER/MAJOR findings in the correction from `f25667a` to `4324275`. Both prior findings are resolved by the reviewed source and tests. No new scoped BLOCKER/MAJOR was found. This approves only the bounded cancellation correction, not Sionna runtime or Phase 7 acceptance.

2. **Plan and ledger anchors:** Read the relevant authoritative portions of `plan.md`: §8.5 (`convergence_sweep`), §16.13 Sionna runtime gate, Phase 7 deliverables/exit criteria, Gate I, and Appendix I required job types. Verified ledger/traceability anchors `source:heading:66df6bddc47f2e0c0973:1`, `source:list_item:5269a81e5d04ce55d10d:1`, `source:list_item:5fa9c2753492416e5529:1`, `source:list_item:eb7936ab43f4560d972d:1`, `backlog:PREB-011:1`, and `audit:PRE-012:1`; they remain `IN_PROGRESS`.

3. **Base and head:** Reviewed the exact correction diff from `f25667aa224a2c5113fab71fe5ba59d56defdac3` to `432427520c957aa2b5a608a4782b6d836f09a0c3`. The feature ancestry begins at `bc51e80`. Current integration main `eef8d89` shares that base but is not an ancestor of this candidate; this review did not validate a merge or cherry-pick onto main.

4. **Worktree and branch:** Assigned isolated worktree `/private/tmp/kyberia-phase7-convergence-final-rereview-20260923`, detached at candidate `4324275` (the local feature ref points there). The worktree was clean at review start. No author or integration worktree was edited.

5. **Inspected and changed paths:** Inspected the exact `f25667a..4324275` diff, complete `workers/sionna/rfatlas_sionna/convergence.py`, relevant existing cancellation/failure behavior in `workers/sionna/rfatlas_sionna/client.py`, `workers/sionna/rfatlas_sionna/contract.py`, `tests/test_sionna_convergence.py`, validation note, `STATUS.md`, ledger, generated traceability, and the two prior review reports at `17fefd3` and `935873f`. This report is the only file changed by this review.

6. **F1/F2 invariant disposition and evidence:**

   - **F1 — resolved:** `run_sweep` accepts an event-like token, passes the same token to each existing client call, checks before the first and each subsequent dispatch, and classifies cancellation after a completed response. `test_cancellation_before_first_run_dispatches_nothing`, `test_cancellation_during_run_reaches_existing_client_token`, and `test_cancellation_between_runs_prevents_next_request` verify token propagation and no follow-on dispatch.
   - **F2 — resolved:** `ConvergenceWorkerFailure` retains the original failed envelope and exposes worker, cleanup, and cancel diagnostics. `_validate_envelope` only emits `ConvergenceCancelled` for a metadata-valid `error="cancelled"` envelope with no result and no cleanup/cancel errors when the caller token is set. Failed envelopes with cleanup/cancel diagnostics are raised as `ConvergenceWorkerFailure` before cancellation can mask them. Exceptions thrown by the client are no longer caught or remapped, so lifecycle failures propagate unchanged.
   - **Independent combined-event reproduction:** I injected a client-shaped envelope while setting the event in the same call: `status="failed"`, `error="process_error"`, `cleanup_error="Windows containment unknown"`, `cancel_error="terminate failed"`, valid request identity/timestamps/log metadata, and return code `-9`. The observed exception was `ConvergenceWorkerFailure`; its worker/cleanup/cancel attributes preserved `process_error`, `Windows containment unknown`, and `terminate failed`, the original failed envelope remained attached, and the client dispatch count was exactly one. This addresses the prior risk that a child/Job Object may remain live without a visible diagnostic. The focused regression also checks cleanup/cancel fields and one-call termination; a separate test verifies a simultaneous raised lifecycle exception is the identical exception object at the caller and does not dispatch another run.
   - **Timeouts and bounds:** The correction diff does not change the existing per-request timeout/CPU limits or preflight caps: 8 budgets, 8 seeds, 32 runs, 4,096 cells/map, 100,000 summary scalars, 32,000,000 aggregate transmitter-samples, and 8 MiB serialized output. A 32-run sweep can still use the sum of individual request timeouts when not cancelled; it is now caller-cancellable through the ordinary client path.

7. **Checks:** `python3 -m unittest discover -s tests -p 'test_sionna_convergence.py' -v` — 14 passed. `python3 tools/ledger.py check` — PASS (5,396 source blocks, 438 explicit ID occurrences, 447 headings). `git diff --check f25667aa224a2c5113fab71fe5ba59d56defdac3..HEAD` — PASS. SHA-256 for helper, test, and validation-note sources matches the ledger. The direct combined-event `process_error` reproduction also passed as described above.

8. **Status, traceability, and evidence limits:** Phase 7, Gate I, PREB-011, PRE-012, and the convergence obligations remain `IN_PROGRESS`. Validation docs accurately limit checks to synthetic fixtures; no Sionna runtime/engine, field result, calibrated uncertainty, held-out P2/P1 improvement, Phase 7 exit, or Wi-Fi SINR/capacity result is claimed. Generated traceability still points at the implementation/tests/validation. Graph MCP was unavailable; no graph or coverage claim is made.

9. **Report commit and cleanliness:** Report-only commit is recorded in the reviewer handoff. Candidate source/tests/docs/ledger were not changed by this review; the reviewer worktree is verified clean after the report commit.

10. **Remaining risks and integration caveat:** No F1/F2 or new scoped cancellation/failure-classification blocker remains in this candidate. Do not promote the feature branch wholesale onto current main: both share `bc51e80`, while main is now `eef8d89`, so the `STATUS.md`, ledger, validation, and generated traceability records need careful reconciliation/regeneration on main before integration. No Sionna runtime or product/Phase 7 completion claim is established.
