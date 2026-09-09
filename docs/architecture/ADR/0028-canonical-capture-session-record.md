# ADR-0028: Canonical acquisition-session provenance record

Status: Proposed bounded increment; review corrections applied and awaiting independent rereview. Native collector runtime, identity-mapping and product workflow gates remain open.

Date: 2026-09-09

Related: plan §§7.1–7.4, 10.4–10.6, 11, Phase 0 exit criteria, Phase 1 acquisition workflow; ADRs 0001, 0003, 0005, 0018.

## Context

The native process boundary produces an opaque `NativeCaptureSession` whose
process-session UUID, source clock UUID, terminal result and normalized capture
come from one validated stream. The capture publication path already stores a
canonical manifest, an optional observation chunk and a recoverable publication
index. That index does not preserve the immutable application identity mapping
that admitted the stream, the exact foreign process and source-clock UUIDs, the
registry version, or the privacy and terminal evidence when an empty capture
has no observation chunk.

Reconstructing those values later from a manifest, a chunk, or a caller's
current registry would permit mixed snapshots and would erase capability-only
source mappings on empty captures. The plan requires separate raw, normalized
and derived evidence planes; monotonic and wall-clock evidence must retain their
uncertainty; foreign schemas must stop at adapters; and a failed or partial
capture must remain diagnosable without being labeled complete.

## Decision

Add a versioned `kyberia-domain::capture_session` contract and a dedicated
`kyberia-project-store` table.

`CaptureSessionRecordV1` contains the canonical `SessionId`, `CollectorId` and
`ClockEpochId` plus the exact lowercase foreign process-session and source-clock
UUID strings. It also contains the canonical capture-manifest hash, privacy
state, registry version, sorted source and observation mapping evidence,
terminal status/reason/partial flag, observation count and closed exit-code
mapping. Foreign UUIDs remain evidence strings; they are never converted into
canonical IDs. The domain module has no adapter or storage dependency.

The record constructor rejects an unsupported schema, malformed or zero native
UUID, duplicate mapping key or canonical ID, an unproved known transmitter,
oversized mapping, an invalid terminal/partial/exit combination, or an
observation count above the capture bound. Canonical decoding rejects unknown
fields, noncanonical JSON bytes and invalid mapping evidence. A separate
`validate_against_manifest` step binds the record to the exact manifest hash,
completion fields, privacy disposition, collector capability identity and
observation envelopes. When an envelope provides a monotonic timestamp or clock
model, its `ClockEpochId` must equal the record epoch. Every observed source
mapping must match the envelope's sensor and adapter evidence, and every
observation mapping must match its radio, BSS and grouping-artifact evidence.
Known raw-source references must equal a complete manifest source-record
reference (hash, media type and byte length). Extra source mappings are allowed
because a capability listing can contain sources that produced no observations;
observation mappings must exactly match the envelopes.

The store accepts a `CaptureSessionRegistration` only after the caller has
published the manifest and, when nonempty, the observation chunk through their
existing owning paths. It stores one immutable row in `capture_sessions` with
the canonical record BLOB, record hash, manifest hash and indexed projections.
The manifest hash is unique, so one immutable manifest cannot silently acquire
two session identities. Registration uses an immediate transaction, exact
retry identity, conflicting-evidence rejection and authoritative readback.
Every read and bounded keyset page rehashes and decodes the canonical BLOB,
compares every indexed projection, reloads the manifest and chunk, and repeats
the domain closure checks.

The table is an additive schema group. New bundles create it; writable opening
of an older compatible bundle adds the empty table without inventing historical
session records; read-only opening remains side-effect free. Registration,
single-record reads and keyset listing have cancellation-aware variants. A
cancellation before commit rolls back the row. A cancellation after commit can
return a retryable cancellation while retaining the complete immutable row for
the next exact retry.

`Bundle::verify` inventories the capture-session table through the same
authoritative read path, one row at a time, so a corrupt canonical session BLOB
cannot be hidden by a healthy SQLite quick check or a valid table shape. V1
rows use the fixed revision marker `1`; other positive revisions are rejected
until a reviewed schema version defines them.

The public API shape is:

```rust
let process_uuid = NativeUuid::new(session.process_session().to_owned())?;
let source_clock_uuid = NativeUuid::new(session.clock_epoch().to_owned())?;
let mapping = MappingEvidenceV1::new(
    registry_version,
    source_rows_sorted_by_foreign_key,
    observation_rows_sorted_by_foreign_key,
)?;
let record = CaptureSessionRecordV1::new(
    mapping_context.session_id,
    mapping_context.collector_id,
    mapping_context.clock_epoch,
    process_uuid,
    source_clock_uuid,
    manifest_hash,
    mapping,
    mapping_context.privacy.clone(),
    session.terminal().into(),
    completion.reason,
    completion.partial,
    completion.observation_count,
    session.exit_code(),
)?;
let registration = CaptureSessionRegistration::new(
    record,
    manifest,
    observations,
    published_utc_ms,
)?;
bundle.register_capture_session_with_cancel(registration, cancel)?;
```

The application layer supplies `registry_version` explicitly because the
current `MappingContext` does not contain registry provenance. This record does
not manufacture pose, capture time, clock synchronization, or capability
measurements that the source did not provide.

## Alternatives

1. **Put session identity into `capture_publications`.** Rejected because that
   table describes a recoverable manifest/chunk/snapshot lifecycle. Its mutable
   links and publication status are different evidence from immutable process,
   mapping, privacy and terminal provenance. Combining them would make empty
   terminal captures and exact process retries ambiguous.

2. **Store the record as a generic annotation artifact.** Rejected because an
   annotation is the wrong semantic type, has no dedicated uniqueness/index
   contract, and would permit a generic import path to bypass the manifest and
   envelope closure checks.

3. **Derive provenance from current manifests, chunks and registries.**
   Rejected because process/source-clock UUIDs and mapping decisions are not
   recoverable from all existing artifacts, especially an empty capture. A
   current registry can also differ from the registry used for admission.

4. **Let the domain own `MappingContext` or the foreign collector DTO.**
   Rejected by the dependency boundary. The outward composition layer must
   translate validated adapter data into the domain mapping rows and bind the
   actual stream to the record.

5. **Write a second JSON or sidecar file.** Rejected because a separately
   committed file can diverge from SQLite during a crash and would duplicate
   the existing canonical artifact authority. A future export may project this
   record after validating the SQLite row.

## Evidence

The implementation is split for review:

- `b3b8124` adds the domain contract; `32ce777` closes terminal and
  manifest/envelope evidence after independent review findings.
- `ed28ba2` adds the store table, migration and APIs; `27ca746` pins the V1
  row revision and makes `Bundle::verify` inventory every session row. The
  project-store suite has 33 passing tests covering corruption, retry, empty,
  cancellation, migration, read-only, concurrent-writer and
  verification-inventory cases. The domain suite has 11 passing tests.
- The project-store schema guard has 14 passing tests after adding the new
  table to the current schema inventory.
- `cargo test --workspace --exclude kyberia-kismet-adapter --locked --offline`
  passes, including domain and project-store doc tests.
- Affected-package Clippy with warnings denied, `cargo fmt --all -- --check`,
  the architecture dependency check and the 241-entry source-inventory check
  pass.

The exact retained logs are under
`.trash/test-runs/capture-session-record-20260909/` in the implementation
worktree. The Kismet live HTTP fixtures remain outside the reproducible
workspace command because this sandbox cannot create their local sockets;
that environment limitation is not evidence about the session record.
Independent rereview of these corrections is still pending, so this ADR does
not claim acceptance or product feature completion.

## Consequences

The record makes process identity, source-clock identity, canonical identity
mapping, privacy and terminal outcomes durable together, including empty
captures. A read can detect a changed BLOB, stale projection, missing manifest,
wrong publication kind, missing chunk, reordered/duplicate observation or
manifest/envelope mismatch before returning a record. `Bundle::verify` reports
the same failures for every stored session row, including rows not discovered
through a caller's cursor. Resource bounds are explicit: one record is at most
1 MiB, verification loads one row at a time, mapping vectors are bounded, a
public list page is at most 128 rows, and SQLite schema/read limits remain in
force.

The store row is immutable and uses a fixed positive revision marker (`1`) for
the V1 row. The stable list cursor is canonical `SessionId`; this revision is
not a project revision or capture clock. Raw capture bytes are not copied into
the session row. An `ArtifactReference` in identity evidence is preserved as
typed evidence; this increment does not invent an identity-artifact registry
or claim authenticated authorship.

The application must provide an explicit registry version and must derive all
rows from the same immutable mapping context and `NativeCaptureSession`. The
record layer cannot prove a native hardware scan, operator consent, or the
semantic correctness of an external identity registry by itself.

## Reversibility

The domain record is versioned and isolated behind constructors and accessors.
The SQLite table is an additive optional group, so older bundles can be opened
read-only and upgraded transactionally without historical guesses. A future
store can project the same canonical record into another physical layout after
revalidating the BLOB and hashes; removing or changing V1 semantics requires a
new schema version, migration fixtures and independent review. No existing
manifest, observation chunk or survey snapshot bytes are rewritten by this
increment.

## Validation plan

Before integration, an independent reviewer must inspect both commits and
exercise the domain and store tests, including malformed canonical input,
oversized mapping evidence, lowercase UUID policy, terminal/exit consistency,
empty capability-only mappings, missing/corrupt manifest and row data, exact
retry/conflict, read-only/reopen, keyset cursors, cancellation and concurrent
writers. Root integration must then test the real glue against an opaque
`NativeCaptureSession`, including an empty terminal capture and a nonempty
capture whose observation and source mappings are bound to one context.

The following commands are the bounded implementation checks:

```text
cargo test -p kyberia-domain --lib --locked --offline
cargo test -p kyberia-project-store --lib --locked --offline
cargo test -p kyberia-project-store --test schema_guard --locked --offline
cargo test --workspace --exclude kyberia-kismet-adapter --locked --offline
cargo clippy -p kyberia-domain -p kyberia-project-store --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
/Users/vincent/code/kyberia/.tools/venv/bin/python tools/architecture.py
/Users/vincent/code/kyberia/.tools/venv/bin/python tools/source_inventory.py check
git diff --check
```

Native collector execution, registry-backed mapping, cross-platform behavior,
durable spool orchestration, UI/CLI commands, authenticated provenance and
the broader Phase 0/Phase 1 exit criteria remain open gates.
