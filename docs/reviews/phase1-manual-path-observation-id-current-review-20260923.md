# Phase 1 manual-path observation-ID accessor review

1. **Disposition: APPROVE.** The accessor-only candidate is correct within
   scope and introduces no scoped blocker. Approval covers exposing retained
   observation IDs from the pure model; it does not approve persistence,
   capture/session association, SUR-002 completion, or Phase 1 acceptance.

2. **Exact candidate, base, and trees.** Reviewed candidate
   `78ac926e35ff1d4f765a833aa42a66be72039e81`, whose parent/base is
   `dc3832b7a3dac827220ff78b57918dbea7b4c505`. The detached review worktree is
   `/private/tmp/kyberia-manual-path-observation-id-review-20260923`. The author
   worktree is `/private/tmp/kyberia-phase1-manual-path-persistence-20260923`;
   it and `/Users/vincent/code/kyberia` were not edited.

3. **Plan scope.** `plan.md` §6.3, SUR-002, specifically its manual-path sample
   ordering and timestamp requirements; MAPB-004 is tracked at §18.4. The
   candidate is a small model-boundary prerequisite only. The broader Phase 1
   roadmap and SUR-002 exit requirements remain open.

4. **Files and behavior reviewed.** Inspected the exact candidate diff and
   `ManualPathSurvey::observation_ids()` in
   `crates/survey/src/manual_path.rs`, the focused regression in
   `crates/survey/tests/manual_path.rs`, and related validation in
   `record_observation()` and `validate_snapshot()`. The accessor maps the
   existing observation slice to copied `ObservationId` values, so iteration
   does not allocate and its borrow is limited to the survey. The returned
   `ExactSizeIterator` reports the retained count, including zero for an empty
   survey. Input insertion sorts by `(captured_at.nanoseconds, observation_id)`;
   IDs have deterministic total ordering, duplicate IDs are rejected, and
   snapshot deserialization revalidates ordering and uniqueness. Thus the
   accessor correctly exposes existing stored order rather than silently
   sorting or changing model state. The regression inserts out-of-order times
   and tied times with IDs in non-sorted order, then checks the resulting
   sequence and iterator length.

5. **Methodology and checks.** The codebase-memory MCP tools were unavailable in
   this review context, so I used exact-source inspection; no graph or index
   coverage claim is made. I copied the offline Cargo registry cache into this
   review worktree and retained it and all build output there. Independent
   checks passed:

   - `cargo test -p kyberia-survey --locked --offline` — 48 passed, 1 existing
     release benchmark ignored (3 unit, 9 association, 12 manual-path, 18 point,
     and 6 migration tests passed).
   - `cargo clippy -p kyberia-survey --all-targets --locked --offline -- -D warnings`
     — pass.
   - `cargo fmt --all -- --check`, `python3 tools/architecture.py check`,
     `python3 tools/source_inventory.py check` (522 locked packages), and
     `python3 tools/ledger.py check` (5,396 source blocks, 438 explicit ID
     occurrences, 447 headings) — pass.
   - `git diff --check dc3832b7a3dac827220ff78b57918dbea7b4c505 HEAD` — pass.

6. **Findings and severity.** No scoped findings at any severity. In
   particular, the method does not claim to authenticate evidence, verify
   membership in a capture session, or persist anything; it only exposes the
   IDs already retained by this survey model. The API and test match that
   bounded contract.

7. **Evidence and tracking.** Verified SHA-256 values match the validation
   note: `manual_path.rs`
   `1449945c1afbc412967b68ceeb6e9df73dc12f837ac48ca695c7a84653319e98`,
   `tests/manual_path.rs`
   `f39155a32eeb293fcbb5982525d6b049bfd7e3fc2d8946e08034d44b69d1640e`,
   `survey-state.md`
   `db0f2a2898abf52a2794423b9d96960da8aad4f88b9cbfa82ef371049a173fae`, and
   `manual-continuous-path.md`
   `ada69a096ca6c944737a99ce492af91302f5ca35543a54d7c1b39c02e1e14a98`.
   The SUR-002 and MAPB-004 ledger entries remain `IN_PROGRESS`; the validation
   and status text keep independent review and product acceptance distinct.
   `TRACEABILITY.md`, `execution-dag.json`, and `Cargo.lock` are unchanged by
   the candidate.

8. **Residual limitations.** This change does not reconcile IDs with canonical
   stored envelopes or collector sessions, establish multi-clock alignment,
   persist survey state, or connect a live capture source. It does not satisfy
   map HUD, desktop/app wiring, field usability, or broader Phase 1/SUR-002
   acceptance. Those limits are accurately retained in the candidate docs.

9. **Integration recommendation.** The root may consider this commit for
   integration as the reviewed accessor-only increment. Keep all broader
   persistence and product gates open.

10. **Report artifact.** This report is the reviewer's only tracked change and
    is committed report-only in the isolated review worktree. Review-tree cache
    and target output are ignored and retained; no worktree or cache was
    removed.
