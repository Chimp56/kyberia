# Normalized observation Parquet export

`kyberia export-observations-parquet <project.rfatlas> <new-directory>` exports
the project's committed normalized observation chunks without a UI. The
destination must not already exist. A successful export contains:

- `manifest.json`, with schema `kyberia.observation-parquet-export/1`, the
  canonical project ID and revision, observation schema version, Parquet
  media type and fixed-schema fingerprint, and the complete ordered chunk
  descriptors, plus a privacy warning that remains visible when the directory
  is shared separately from the command output;
- one `<sha256>.parquet` file for every descriptor, byte-for-byte identical to
  the verified immutable project artifact.

Before a chunk is exported, Kyberia verifies its manifest registration, SQLite
chunk and member indexes, byte length, SHA-256 hash, fixed Arrow schema,
bounded Parquet decode, canonical observation envelopes, and descriptor
ranges. Unknown evidence and provenance therefore retain their canonical
Parquet representation. The export does not convert an unknown value to an
empty numeric value or to zero.

The exporter checks that the project manifest is unchanged after all chunk
reads. If another writer commits during the export, publication fails instead
of labeling a mixed-revision directory with the earlier revision.

The V1 manifest is a closed contract: readers must reject unknown fields and
schema labels other than `kyberia.observation-parquet-export/1`. Its top-level
fields are `schema`, `project_id`, `project_revision`,
`observation_schema_version`, `chunk_media_type`,
`parquet_schema_fingerprint`, `privacy_warning`, and `chunks`. Each chunk has:

The machine-readable contract is
[`schemas/observation-parquet-export-v1.schema.json`](schemas/observation-parquet-export-v1.schema.json).

| Field | Meaning |
|---|---|
| `hash` / `file_name` | Lowercase SHA-256 and exactly `<hash>.parquet`. |
| `bytes` / `media_type` | Exact file length and pinned Parquet media type. |
| `observation_schema_version` / `codec_version` | Canonical envelope and storage codec versions. |
| `row_count` | Nonzero bounded envelope count. |
| `first_observation_id` / `last_observation_id` | Inclusive canonical-ID range. |
| `known_utc_count` / `first_utc_ns` / `last_utc_ns` | Explicit nullable UTC support and bounds. |
| `first_source_id` / `last_source_id` | Inclusive source-ID range. |
| `first_session_id` / `last_session_id` | Inclusive survey-session range. |
| `provenance_id` | Source-qualified publication provenance. |
| `committed_revision` | Project revision that committed the chunk. |

Chunks are strictly ordered by hash. V1 fields cannot change meaning or type;
an additive or semantic change requires a new export schema and decoder.

The command creates the destination before streaming verified chunks. It
flushes every chunk and the directory before it writes and atomically renames
the final `manifest.json` publication marker, then flushes that directory
entry on Unix. If verification or an earlier
I/O operation fails, the command exits nonzero without that marker; a possible
`.manifest.json.pending` file is not a successful export. The incomplete
directory is retained for diagnosis. If the final Unix directory flush fails
after rename, the command still exits nonzero even if `manifest.json` is
visible; only a zero exit status confirms completed publication. Re-run with a
different new destination.

This contract implements the normalized-Parquet portion of the Phase 0
raw-data-without-UI criterion. The complete criterion remains open pending the
CSV subset, Arrow streaming API, retained PCAPNG evidence, and policy-specific
export confirmation UX.

Directory-entry crash durability on Windows remains an explicit portability
gate because the safe Rust standard library does not expose a directory handle
flush there. The files themselves are flushed on every supported platform.
