# Phase 0 desktop map current-main integration review

Disposition: **APPROVE** for this bounded assembled integration candidate. This
is not Phase 0/1 acceptance, does not close MAP-002/MAP-003 or MAPB-001/2, and
does not authorize promotion. No product files were changed by this review.

1. **Scope.** Independent review of the assembled desktop PNG map workflow and
   current-main compatibility: MAP-002/MAP-003, MAPB-001/2, plan §§5.3, 6.2,
   and Phase 0 §17. Focus included coexistence of the Phase 1 point-snapshot
   API with Phase 0 map-intent API, desktop/application/store wiring, tests,
   dependency inventory, tracking and validation claims.

2. **Plan and records.** Reviewed the plan scope above and the corresponding
   catalog/backlog records in `docs/implementation/ledger.json`, plus
   `docs/implementation/TRACEABILITY.md`, `STATUS.md`, ADR 0035, and the
   current-main validation packet. The four scoped catalog/backlog records
   remain `IN_PROGRESS`; the separate audit MAP-002/MAP-003 records remain
   `NOT_STARTED`. No graph/coverage claim is made; graph tools were unavailable
   in the parent review context.

3. **Base and head.** Candidate `6bddfe701e026418398c9f1ee9253cb4593485aa`
   is a direct child of base
   `77347586dd94a494684a073d9a0756a78757b22d`. Reviewed the exact detached
   candidate in `/private/tmp/kyberia-phase0-current-main-integration-review-6bddfe7-20260923`;
   no rebase or merge was performed.

4. **Inspected paths.** `crates/application/src/session.rs`, `port.rs`,
   `map_mutation.rs`, `command.rs`, `error.rs`, `lib.rs`,
   `crates/application/tests/project_session.rs`,
   `crates/application/tests/point_survey_snapshot.rs`, domain project
   hierarchy sources, `apps/desktop/src-tauri/src/{lib.rs,main.rs}` and
   `tests/ipc_boundary.rs`, desktop contracts/session/UI/e2e tests,
   `Cargo.lock`, `apps/desktop/src-tauri/Cargo.toml`,
   `docs/licenses/cargo-sources.json`, `docs/implementation/{ledger.json,TRACEABILITY.md}`,
   `STATUS.md`, ADR 0035, and
   `docs/validation/desktop-map-workflow-current-main.md`. Also checked the
   prior source and ledger review artifacts referenced by the packet.

5. **Verdict and invariants.** `ProjectSession` retains snapshot save/load/history
   and adds import/calibration intent methods. Both legacy map mutation
   methods, both map-intent methods, and snapshot save retain their ReadWrite
   checks. Tauri map commands call the intent methods, and the
   application exports, error categories, port methods and current-snapshot
   access compile together. The fresh-floor test verifies revision zero and
   floor projection; the map test verifies derived causal metadata, strict
   out-of-bounds rejection without revision advance, durable receipt/current
   projection, and exact import/calibration retry after reopen with nine
   concurrent DAG heads. The candidate leaves the PNG parser container-only:
   it does not claim IDAT inflation, pixel decoding, or rendered-image proof.
   No integration regression was found.

6. **Checks.** Independently reran successfully:

   - `cargo test --locked --offline --quiet -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application -- --test-threads=1` — pass, including application groups of 13 unit, 5 snapshot, and 19 project-session tests; pre-existing ignored tests stayed ignored.
   - `cargo test --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --quiet` — pass: 23 library, 6 binary, 2 IPC boundary tests.
   - `python3 tools/ledger.py check` — pass: 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
   - `python3 tools/source_inventory.py check` — pass: 522 locked external packages.
   - `python3 tools/architecture.py check`, `cargo fmt --all -- --check`, and
     `git diff --check BASE HEAD` — pass.

   The validation packet also records author-run strict application/Tauri
   Clippy, TypeScript, Vitest, a focused Playwright recovery test, and macOS
   Cargo check as passing. I did not rerun those frontend/Clippy checks.

7. **Tracking and hashes.** The validation packet SHA-256 is
   `70af664f6795a7a177e85d4c446b0531c649473da77a88664810cefd3782ff4c`,
   matching its revision/hash entries in the scoped ledger records. `Cargo.lock`
   is `8a7ecc9cf44dbc9e98ca6743123d8780e4f9d58def147de125c199ed1c1a8c09`;
   the source inventory check passes and the candidate adds only the pinned
   `serde_json = 1.0.149` Tauri dev dependency, without package-version
   updates. Ledger IDs and statuses are intact; no Phase 0/1 completion is
   claimed.

8. **Report commit and cleanliness.** This Markdown report is the only change
   made in the reviewer worktree. It is committed report-only; the commit ID is
   returned in the handoff. Candidate files were not edited, and the worktree
   is clean after the report commit.

9. **Findings and residual risks.** No scoped integration finding. The
   validation packet accurately bounds the implementation to PNG-container
   admission, a numeric two-point workflow, mocked/synthetic desktop fixtures,
   and candidate-local evidence. The packet's “fresh independent review
   pending” language describes the candidate snapshot before this separate
   report commit.

10. **Blockers and limits.** Full Playwright suite, production frontend build,
    native-picker runtime, Windows adapter build/runtime, two-platform
    deterministic fixture, live capability matrix, IDAT/pixel decoding and
    display, field calibration, raw export, and Phase 0/1 exit remain open as
    the packet states. This review does not verify graph completeness or
    runtime/hardware behavior.
