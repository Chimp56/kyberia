# Phase 1 point-survey application/session increment

This validation covers the isolated application-boundary candidate based on
`afc5070fadac46411f02d355a0d00351f388205f`. It exposes the existing typed
`PointSurvey` snapshot persistence through `ProjectSession`: save with an
optional bundle-revision precondition, replay-validated load with an optional
expected survey-session ID, and bounded session-filtered history. Application
receipts and views use typed identities/content hashes and do not expose
`Bundle`, `StoreError`, or project-store record types.

The integration tests use complete valid point-survey fixture state and real
retained bundles under `.trash/test-runs/`. They cover create/write/reopen/load,
read-only rejection, stale revision conflict mapping without a commit, unknown
snapshot and session-mismatch errors, and filtered/unfiltered history. Loading
preserves the survey decoder receipt. The API accepts any validated
`PointSurvey`; this increment does not add a UI, capture adapter, live
collection loop, or a new quality-gate policy.

## Validation

Commands run from the assigned worktree, with Cargo registry/index and build
outputs scoped there:

```sh
cargo fmt --all -- --check
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo test --locked --offline -p kyberia-application
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo test --locked --offline -p kyberia-project-store
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo test --locked --offline -p kyberia-survey
CARGO_HOME=/private/tmp/kyberia-phase1-application-survey-session-20260923/.trash/cargo-home-20260923 CARGO_TARGET_DIR=/private/tmp/kyberia-phase1-application-survey-session-20260923/target cargo clippy --locked --offline -p kyberia-application --all-targets -- -D warnings
python3 tools/architecture.py
python3 tools/source_inventory.py check
python3 tools/ledger.py check
```

Application tests: 36 passed (13 unit, 5 new snapshot integration, 18 existing
session integration). Project-store tests: 147 passed, one benchmark ignored.
Survey tests: 35 passed, one benchmark ignored. Strict application Clippy and
format checks pass. Architecture and source-inventory checks pass, with all
522 locked external packages represented. Ledger generation/check passes with
5,396 source blocks, 438 explicit-ID occurrences, and 447 headings.

This is evidence for an application/session persistence foundation only.
Phase 1, `SUR-001`, point-capture orchestration, manual continuous survey,
desktop flow, and product acceptance remain open; this candidate awaits fresh
independent review and is not integrated into `main`.
