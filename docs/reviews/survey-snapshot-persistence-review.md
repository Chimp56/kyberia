# Survey snapshot persistence review

Status: **APPROVED for the bounded transactional snapshot increment**

Reviewer: `/root/survey_store_review_luna` (independent of the implementation author)

## Reviewed boundary

The review covered immutable point-survey snapshot publication, SQLite index and
history integrity, artifact replay, legacy migration, manifest preflight,
resource limits, optimistic revisions, source identity and failure atomicity.
It does not promote snapshot storage into complete raw-observation persistence
or close the broader crash, disk-full and power-loss validation gates.

## Findings and resolutions

The first review required future-schema preflight before mutation, a complete
bidirectional index/history invariant, SQL-bounded reads, nondecreasing manifest
timestamps, exact artifact metadata, listing integrity, a purpose-specific
`PointId` database encoding and a unique ADR number. The implementation added
each invariant and adversarial regression coverage.

The second review found that a requested load did not audit unrelated snapshot
rows, matching SQL timestamps could escape the manifest interval, filtered
history could conceal corruption in another session, verification lacked a SQL
`MAX + 1` bound, and storage records omitted canonical source and collector IDs.
The implementation now validates and replays the complete bounded inventory
before load or filter projection, checks the manifest time interval, and stores
typed source/collector identities in both index and history.

The final major finding was that read-only manifest preflight queried SQLite
before defensive connection initialization. The preflight now installs the
authorizer, 4 MiB SQLite value limit and VM/time budget before its first schema
or manifest statement. A database containing a `zeroblob(4 MiB + 1)` manifest
fails boundedly before writable open and retains its database envelope.

No BLOCKER or MAJOR finding remains.

## Validation

- `cargo test -p kyberia-project-store --all-targets --locked --offline` — PASS,
  including 20 focused survey snapshot tests.
- `cargo test -p kyberia-survey --locked --offline` — PASS.
- `cargo test --workspace --locked --offline` — PASS.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/architecture.py` — PASS.
- `.tools/venv/bin/python tools/source_inventory.py check` — PASS, 97 packages.
- `git diff --check` — PASS.

## Residual validation boundary

SQLite may refresh volatile `-shm` lock metadata while probing a valid WAL;
authoritative main/WAL bytes remain unchanged. Malformed hot journals fail
before writable recovery. Power-loss, disk-full, backup/recovery policy, orphan
artifact reclamation and filesystem TOCTOU behavior remain broader storage and
release validation work.
