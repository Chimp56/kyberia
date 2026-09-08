# Operation-store review

Status: **APPROVED**

Scope: transactional SQLite persistence, deterministic replay, duplicate and
tamper handling, projection recovery, and additive migration for the canonical
operation log.

The first independent review found two MAJOR issues: persistence admitted an
operation graph without executing replay-semantic validation, and untrusted
SQLite BLOBs were materialized before their size limits were checked. The first
correction added a shared replay-admission check and scalar `length(...)`
preflight queries. A second review then found that an unrelated unresolved field
conflict could cause replay to return before detecting an invalid repeated undo
or redo. The final correction added pure
`OperationSet::validate_replay_semantics()` and executes that toggle state
machine before accepting explicit conflicts or writing SQL.

The final reviewer, who did not author the implementation or corrections,
confirmed that sequential invalid toggles cannot be masked by unrelated
conflicts; unresolved and mixed toggle conflicts remain explicit; concurrent
duplicate toggles remain idempotent; and canonical/wire BLOB lengths are checked
before retrieving their contents. Regression tests verify rejection leaves the
operation revision, bundle manifest, and row count unchanged.

Validated commands included:

```text
cargo test -p kyberia-operation-log --locked --offline
cargo test -p kyberia-project-store --locked --offline
cargo test --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The final review reported no BLOCKER, MAJOR, MINOR, or NIT findings. Canonical
project materialization, authorization, coordinator transport, signatures, and
product conflict UX remain separate planned capabilities.
