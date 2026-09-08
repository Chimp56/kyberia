# ADR-0014: Transactional, immutable point-survey snapshots

Status: Accepted for the bounded transactional snapshot boundary after
independent review. Broader project-store migration, raw-observation storage
and release validation remain open.

## Context

Plan §§7.1, 10.6–10.7 and 11 require raw, normalized and derived evidence to
remain distinct, project writes to be transactional, and survey results to
survive decoder changes. A point survey contains strict admitted observations
and, for managed-mode sources, receipt-based spatial associations. Replacing a
JSON state file would lose history, make partial writes authoritative, and
provide no integrity boundary for imported legacy bytes.

## Decision

Persist each point-survey snapshot as an immutable content-addressed artifact in
the bundle's existing `artifacts/` directory. SQLite remains the metadata and
revision authority. Two append-only metadata tables record the snapshot's
project/session/point/source/collector identity, artifact hash, input/output
wire schema, decoder/source versions, commit revision and timestamp, plus one
history row per new snapshot revision. The history row repeats the complete
index identity and schema metadata so replay can verify the relationship in both
directions.

The write path publishes the bounded artifact bytes first, then commits the
artifact registration, snapshot index and history row in one immediate SQLite
transaction. The transaction verifies the manifest and inserted row before
atomically updating the redundant projection. A failed transaction can leave an
unreferenced content blob for explicit later garbage collection, but it cannot
advance the manifest revision or publish a partial index.

Loads bound the artifact read, verify its exact media/provenance declaration,
length and SHA-256, decode through the versioned survey decoder, compare its
semantic state with the indexed project, session, point and source identity,
collector identity and source schema, enforce the manifest creation/update
timestamp interval, and return the decoder/migration receipt. History queries
use a bounded SQL inventory and verify every index/history pairing and every
artifact, including rows excluded by a session filter. Writable opens
preflight the manifest through a read-only handle before any legacy migration
or write-capable SQLite open. Future manifest-only bundles remain byte-exact;
valid WAL content remains byte-exact while SQLite may refresh volatile `-shm`
lock state, and an unreadable hot rollback journal fails before recovery.
The read-only probe applies the same 4 MiB SQLite value limit as normal
metadata operations before reading the manifest, keeping an oversized value
bounded in the Rust process even when the enclosing database remains below its
64 MiB limit.
Legacy untagged V1 bytes remain unchanged; the loader reports their migration
to current V2 semantics. Optional expected-revision checks provide optimistic
concurrency for stale application handles. Reusing a snapshot ID with identical
evidence is idempotent and creates no new history row.

## Alternatives

1. Store the complete survey as a mutable SQLite JSON column. This gives a
   simple transaction but overwrites evidence and has no stable content
   boundary or artifact export.
2. Store only a JSON file next to the project. This cannot atomically commit
   metadata and bytes and makes crash recovery ambiguous.
3. Rewrite every legacy snapshot to V2 on import. This destroys the original
   evidence bytes and weakens provenance; retaining the input artifact with an
   explicit decoder receipt is safer.
4. Add a separate database per snapshot. This multiplies recovery and locking
   surfaces and breaks the project-level revision authority.

## Evidence

The project-store integration tests exercise current V2 and legacy V1 bytes,
association round trips, checksum and missing-blob failures, future/malformed
and oversized inputs, duplicate identity, multiple revisions/history, explicit
session checks, source/collector tamper detection, manifest timestamp bounds,
bidirectional inventory corruption, filtered replay, bounded SQL listing,
projection rollback, stale handles, and WAL/hot-journal preflight semantics.
The survey crate's decoder tests independently validate state invariants and
migration receipts.

## Consequences

The bundle keeps a small SQLite index while snapshot payloads remain portable
and checksummed. Unreferenced blobs are retained after a failed write and need a
future, explicitly authorized garbage-collection operation. Snapshot history is
bounded to 4096 entries. Read-only V1 bundles remain inspectable; writable open
adds the two snapshot tables transactionally without changing their manifest
revision.

The implementation stores point snapshots as JSON because the current survey
wire contract is JSON and already has strict typed decoding. High-volume raw
observations remain outside this path and must use the storage split described
by [ADR-0004](0004-storage-split.md).

## Reversibility

The content store and public `Bundle` methods form an inward storage port. A
future binary or columnar snapshot representation can add a new media type and
decoder while retaining old artifacts and history. The SQLite tables are
additive and can be migrated forward without rewriting immutable bytes.

## Validation plan

Run the focused snapshot integration tests, all project-store tests, survey
decoder tests, workspace format/check/type/lint/test suites, architecture and
source/license inventory checks. Add crash/power-loss and full release-bundle
tests before declaring the storage gate complete. Runtime validation must also
verify migration backups and recovery on each supported desktop platform.
