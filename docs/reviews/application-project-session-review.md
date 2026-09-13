# Application project-session boundary review

- Reviewed source series: `8a5f56b`, `e482736`, `447b7a8`, `4d443de`
- Integration series: `8987783`, `25d9fd8`, `c2516be`, `cb17839`
- Independent reviewer: `/root/application_project_session_review`
- Disposition: **APPROVED**

## Scope

The review covered the application-owned create/open session commands, immutable
current-project query, production `ProjectStorePort` adapter, typed application
errors, cumulative resource budget, schema and feature revalidation, legacy
absence semantics, dependency direction, and real project-store integration
tests. It did not promote project mutation commands, background jobs, or the
desktop product surface.

## Resolved findings

- **BLOCKER:** The first implementation allowed mixed-revision reads and leaked
  storage distinctions through the public boundary. The correction reads one
  canonical snapshot and exposes application-owned types only.
- **MAJOR:** Reopen, concurrent schema advance, cumulative budget, malformed
  manifests, declared-artifact loss, and cancellation paths needed executable
  coverage. Real-bundle integration tests now cover those cases.
- **MAJOR:** Error classification relied too broadly on persisted messages. The
  adapter now classifies typed SQLite and publication failures by operation
  context without exposing `StoreError`.
- **MAJOR:** Legacy handling was ambiguous. An entirely absent optional
  materialization table group is explicit `LegacyAbsent`; a partial group is
  corruption rejected by schema validation.

No blocker, major, minor, or nit findings remain after the final ADR wording
review.

## Validation

- `cargo test -p kyberia-application --locked --offline`: **PASS**, 2 unit and
  16 integration tests.
- `cargo clippy -p kyberia-application --all-targets --locked --offline -- -D warnings`:
  **PASS**.
- `cargo fmt --all -- --check`: **PASS**.
- `python3 tools/architecture.py`: **PASS**.
- `python3 tools/ledger.py check`: **PASS** for the integration candidate before
  this traceability update.

## Remaining gates

The application crate is a bounded accepted foundation. Operation-backed project
mutation commands, cancellable immutable-manifest jobs, authorization, desktop
composition, and end-to-end user workflows remain open and must not inherit this
approval.
