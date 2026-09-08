# Point-survey snapshot storage

`kyberia-project-store` persists a validated `PointSurvey` through the
`Bundle::save_survey_snapshot` and `Bundle::import_survey_snapshot` commands.
The survey state is an immutable JSON artifact; the SQLite database stores its
content hash and the identity/provenance needed to verify replay.

The artifact is bounded to 8 MiB before decoding and must also satisfy the
bundle's 64 MiB artifact limit. The survey decoder enforces its own record and
window limits. Input bytes are never rewritten during import, so an untagged V1
artifact remains available byte-for-byte while a load returns a V2 survey and a
`PointSnapshotDecodeReceipt` identifying `kyberia-point-snapshot/2.0.0` and
whether migration occurred.

SQLite tables are created for new bundles and added transactionally when a
legacy manifest-only bundle is opened read-write:

* `survey_snapshots` is an immutable index keyed by `snapshot_id`. It binds the
  project, session, point, source and collector IDs, artifact hash,
  input/output schema, decoder version, source adapter version, creation time
  and project revision.
* `survey_snapshot_history` is append-only and records each committed snapshot
  revision, the complete point/session/project/source/collector identity, schema
  and source metadata, artifact hash, operation version and commit time. Every
  index row must have exactly one matching history row, and every history row
  must have exactly one matching index row.

The content blob is durably written before the SQLite transaction. The
transaction then verifies the current manifest, applies an optional optimistic
revision precondition, inserts the artifact registration and both metadata
rows, reads the manifest and row back, and publishes `manifest.json` only after
the authoritative values are verified. Projection failure rolls back all
SQLite metadata. An orphaned content blob from a failed transaction is not
authoritative and is retained for a future explicit garbage-collection policy.

Loading verifies the project binding, optional caller session binding, the
matching history row, artifact registration and exact media/provenance
semantics, nonsymlink regular file, byte bound, length and SHA-256 before
semantic replay. Index fields are compared with the replayed survey config,
including session, point and source adapter version. History listing uses a
`MAX_SURVEY_SNAPSHOTS + 1` SQL limit and validates both directions of the
index/history relationship plus artifact bytes for each returned row.
Unknown/future wire schemas, malformed state, duplicate fields, truncated data,
corrupted/missing blobs, stale timestamps and cross-project/session records fail
closed. Writable opening preflights manifest compatibility before the additive
legacy-table migration, so future manifest-only bundles remain byte-for-byte
unchanged on rejection. The preflight uses a read-only SQLite handle before any
write-capable handle is opened. A valid WAL is read without checkpointing; the
main database and WAL bytes remain unchanged, while SQLite may refresh volatile
lock state in the `-shm` sidecar. If a rollback journal is present but cannot be
opened read-only (for example, a malformed hot journal), the probe returns the
SQLite error before a writable recovery or migration is attempted, and the
database envelope remains unchanged. SQLite's defensive 4 MiB value limit is
applied before the manifest query, so an oversized manifest fails without
materializing the value into Rust or opening the writable migration path. See
[ADR-0014](ADR/0014-transactional-survey-snapshots.md).

Use `save_survey_snapshot_if_revision` from a UI or command handle that has
read a manifest revision. Saving the same snapshot ID with the same artifact,
session, point and decoder semantics returns the original record without
advancing the project revision or appending history. A different payload under
an existing ID is rejected.
