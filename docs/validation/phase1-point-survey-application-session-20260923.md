# Phase 1 point-survey application/session increment

This bounded follow-up is based on author candidate
`a6ee85d0fa098049279c043c7f93925bf3d571f9` and the unchanged authoritative
`plan.md` digest `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`.
It corrects the history replay availability issue and save-error classification
in the isolated application/store candidate; it is not integrated and awaits
fresh independent review.

`ProjectSession` exposes application-owned typed point-survey snapshot history
pages with optional session filtering and a cooperative-cancellation entry
point. It does not expose `Bundle`, `StoreError`, or project-store page/record
types. Each page is all-or-error. Every call validates the complete bounded
index/history inventory, exact index↔history correspondence, and artifact
registry metadata for every indexed snapshot before applying the session and
cursor filters. Artifact bytes are read, hash-checked, decoded, and semantically
replayed only for artifacts returned on that page; excluded/off-page artifact
bytes are not read or claimed valid. Thus a filtered call still detects
inventory and artifact-metadata tampering outside its result, but byte
corruption outside its result is detected only when a page includes that
snapshot or a broader bundle verification reads it.

Page ceilings are 64 items, 16 MiB aggregate artifact bytes, and 79,697 work
units. The work ceiling is exactly 10,000 artifact-registry entries + 4,096 ×
(2 SQL row visits + 13 binary-search comparisons + 1 pair-validation unit + 1
page-selection visit) + 1 overflow-sentinel row + 64 page artifact replays.
The search allowance is `ceil(log2(4,096 + 1))`; both row inventories stop at
4,097, and the sentinel unit covers detecting an over-limit inventory. A
single immutable survey artifact is capped at 8 MiB. Callers may lower these
ceilings; insufficient byte/work limits return a resource error, never a
partial page. Full histories remain traversable across calls.

The first cursor pins the project identity, session filter, and starting bundle
revision as a high-water mark. Continuation is strictly after the last returned
`(revision, snapshot_id)` key in history order and at/below that high-water
revision, so later writes do not appear mid-traversal and filtered pages do not
skip intervening history entries. The application keeps the legacy
`ProjectSession::list_point_survey_snapshot_history` `Result<Vec<_>, _>`
signature as a strict one-default-page convenience: if another page exists it
returns `ResourceLimit` without exposing the first page's entries. Cursor,
limits, and cancellation are available through the explicitly named
`ProjectSession::list_point_survey_snapshot_history_page` API. The store-level
`list_survey_snapshot_history` compatibility method has the same single-page
all-or-error behavior. `Bundle::verify` does not use that compatibility
method; it explicitly drains the bounded page API.

Application tests use valid typed survey fixtures and retained real bundles in
`.trash/test-runs/`. They cover round-trip/reopen/load, read-only rejection,
optimistic conflict, unknown identity, session-filtered pagination, cursor
high-water behavior, invalid limits/resource mapping, cancellation, and
application-owned API types. A 65-entry application test confirms the legacy
Vec wrapper returns all-or-error and the new page method traverses 64 + 1
entries. Write mapping tests confirm reused immutable IDs with different
evidence become `Conflict`, caller timestamp regression becomes
`InvalidRequest`, and stale revisions stay conflicts. Store tests cover exact
byte/work exhaustion, cancellation after artifact read with no returned page,
metadata-versus-off-page-byte integrity semantics, full traversal, and the
single-page compatibility cap.

## Validation

Commands run from the assigned worktree. Cargo registry/index cache is retained
at `.trash/cargo-home-20260923/` (916 MiB); build outputs are in `target/`.

```sh
rustfmt --edition 2024 crates/project-store/src/survey_snapshot.rs crates/project-store/src/bundle.rs crates/project-store/src/lib.rs crates/project-store/tests/survey_snapshot.rs crates/application/src/survey_snapshot.rs crates/application/src/port.rs crates/application/src/session.rs crates/application/src/lib.rs crates/application/tests/point_survey_snapshot.rs
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo fmt --all -- --check
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo test --locked --offline -p kyberia-application
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo test --locked --offline -p kyberia-project-store
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo test --locked --offline -p kyberia-survey
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo test --locked --offline -p kyberia-observation-pipeline
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo clippy --locked --offline -p kyberia-application -p kyberia-project-store -p kyberia-observation-pipeline --all-targets -- -D warnings
python3 tools/architecture.py
python3 tools/source_inventory.py check
python3 tools/ledger.py generate
python3 tools/ledger.py check
git diff --check
```

Results: application 39 passed (including the compatibility follow-up);
project-store 150 passed with one throughput benchmark ignored; survey 35
passed with one benchmark ignored; observation pipeline 63 passed with one
platform-dependent test ignored. Strict Clippy and format checks, architecture
check, 522-package source inventory, ledger generation/check, and diff check
all pass.

This remains an application/storage foundation only. `SUR-001`, Phase 1,
capture orchestration, manual continuous survey, desktop flow, and product
acceptance remain open. Hardware, GUI, cross-platform runtime, and whole-workspace
integration checks were not run; author work awaits independent review.
