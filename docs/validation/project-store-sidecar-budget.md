# Project-store SQLite sidecar budget validation

Date: 2026-09-07

Environment: Apple Silicon macOS; Rust workspace locked and offline

Scope: repository audit finding `STORE-ITER4-001`

The project-store bundle now counts `project.sqlite`, `project.sqlite-wal`, `project.sqlite-journal`, and `project.sqlite-shm` against one 64 MiB pre-open and per-operation budget. Every present sidecar must be a nonsymlink regular file. This preserves bounded SQLite crash-recovery inputs while preventing a small main database from hiding an arbitrarily large WAL or journal.

Validation commands and results:

```text
cargo test -p kyberia-project-store --test schema_guard --locked --offline
PASS — 12 integration tests, including sparse WAL/journal/SHM limits, sidecar symlink rejection, accepted bounded WAL/non-hot rollback inputs, and an independently constructed valid WAL larger than 64 MiB whose final database state is canonical. A private unit regression separately covers an exact 64 MiB combined boundary and checked-addition overflow.

cargo test -p kyberia-project-store --locked --offline
PASS — project-store unit and integration regression suite.

cargo clippy -p kyberia-project-store --all-targets --locked --offline -- -D warnings
PASS
```

The valid-WAL regression keeps its raw SQLite writer open, disables automatic checkpoints, alternates a large temporary manifest value with the original canonical bytes, and verifies that the final WAL exceeds the budget before `Bundle::open` rejects it. Its final visible manifest is valid, so schema rejection cannot accidentally satisfy the assertion.

This is a size and file-type boundary, not a claim that SQLite will accept every bounded crash residue. Existing schema, manifest-size, authorizer, checksum, and integrity validation still run after the envelope check.
