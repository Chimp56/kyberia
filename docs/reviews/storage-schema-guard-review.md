# Independent review: storage schema and write integrity

Verdict: **APPROVED for correction P0-MID-001**. No unresolved BLOCKER or MAJOR findings in this change. This does not approve the complete untrusted-project boundary or storage Gate C.

Reviewer: primary integration engineer, independent of the correction author. The original midpoint audit reproduced a trigger making an artifact write report success without registering its evidence. The correction was reviewed in a separate `review/storage-schema-guard` worktree.

## Findings and verification

P0-MID-001 is resolved: SQLite views/triggers are disabled before the first SQL statement, the built-in schema catalog must match the physical format Kyberia actually writes, and each write validates its singleton/cardinality and exact authoritative readback inside the transaction before publishing the projection. Schema validation and subsequent reads share a transaction. The authorizer limits subsequent SQL operations; cooperative VM/deadline limits provide a separate backstop. The new hooks feature uses the already pinned SQLite library and does not change canonical domain dependencies or disk format.

Independently executed `cargo test -p kyberia-project-store -p kyberia-cli --offline --locked`: **23 storage tests and two CLI workflows PASS**. This includes existing migration/future-version, recovery, corruption, duplicate-artifact and concurrent-writer tests, seven new hostile-schema cases and three guard/readback unit cases. The independent reviewer inspected the test-only authorizer fault injection: it suppresses column updates while SQLite reports a matched row, so the test specifically exercises readback beyond the schema guard.

Four additional independent tests in the review worktree's `crates/project-store/tests/reviewer_integrity.rs` all PASS:

- Four BEFORE/AFTER trigger × read-only/read-write combinations reject on fresh open; stale handles reject writes/recovery, preserve projection bytes and keep authoritative revision zero.
- A writable-schema catalog mutation without a schema-cookie bump is rejected by the existing handle, checking that a cached prepared schema cannot bypass catalog validation.
- Removing the exclusively owned hostile test trigger after a rejected write allows the same handle to recover, commit the retained immutable artifact once and verify after reopen. Failure does not poison later valid work.
- Twelve distinct artifact writes remain readable through an independently opened stale read-only handle; revision, bytes and final verification agree.

The additional command was `cargo test -p kyberia-project-store --offline --locked --test reviewer_integrity`: **4 PASS in 0.48 s**. These are independently constructed probes, retained in the isolated review workspace; they are not claimed as four new main-branch regression tests. Owned temporary fixtures are retained, with no recursive deletion.

## Practical limits

Exact physical DDL validation intentionally rejects extra tables, indexes, views, triggers or alternate spellings of the schema. This preserves the current writer's format and requires explicit reviewed physical migrations for future formats; the existing future logical-version read-only fixture still passes. The 64 MiB main database limit and 2M VM/five-second progress budget do not impose hard native memory, filesystem-call or process-time isolation. SQLite sidecars and hostile concurrent filesystem replacement remain open, as documented before and after this correction. Rejected writes can retain unregistered immutable blobs; successful writes now require authoritative readback.

## Reviewed file hashes

| File | SHA-256 |
|---|---|
| `crates/project-store/Cargo.toml` | `56049c767e11a5987f1300f12e65f2f02f2c93b9d0e9bbca070791d0da842542` |
| `crates/project-store/src/bundle.rs` | `58bba2f526690dc636becfe7494f73bcece19af6c7d76efe86fc0931090dd648` |
| `crates/project-store/src/lib.rs` | `da917befd178c3cf2bc5264cdafd4cfd8a5dad8565803500f341393b2f10e39c` |
| `crates/project-store/src/sqlite_guard.rs` | `883c88db72bad01bf3d7151d62c3683a13d6b653737b81a374ca47ced1eb1002` |
| `crates/project-store/tests/schema_guard.rs` | `16714b0d079c0bc7a863fdf2ff863c9641890779f60b00a0fa415dba4bafd229` |
| `docs/architecture/project-bundle.md` | `9a019f74ed2955d844369d6b8afb053a3e5a4ab823d542cc545ed9c2f5fdf1ea` |
| `docs/validation/storage-schema-guard.md` | `2ba75b4ff94364cd3746d8e3533ef40b517122b17065325b41f97f149b9df1b9` |
