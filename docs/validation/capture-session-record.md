# Canonical acquisition-session record validation

Status: corrections in `32ce777` and `27ca746` are awaiting independent
rereview; this document describes a bounded domain/storage increment. It
does not claim a real native scan, production identity registry, durable spool
workflow, UI command, or Phase 0/Phase 1 completion.

## Boundary and API

`NativeCaptureSession` remains an opaque outward process result. It carries the
validated process-session UUID, source clock UUID, terminal status/exit code and
one normalized capture. The application mapping callback retains the exact
validated `MappingContext`. It then supplies an explicit registry-version
`Text` because `MappingContext` currently has no registry-version field.

The application builds `MappingEvidenceV1` from the context's source and
observation maps. The constructors sort rows by their foreign keys, reject
duplicate keys or canonical IDs, and reject known transmitter IDs without
known identity evidence. Source rows may include capability-only receivers;
observation rows must close exactly over the manifest's envelope IDs.

The production composition entry point is
`kyberia_observation_pipeline::NativeAcquisitionBatch::from_session`. It accepts
only the opaque `NativeCaptureSession` and the explicit registry-version text;
it takes the normalized capture and immutable mapping context from that same
session, builds the batch and record, and validates the record against the
batch's manifest and envelope order before exposing either view. The current
bounded implementation clones the normalized capture and envelope references
for composition; it is not a durable spool or a streaming memory claim.

The session record is constructed only after the caller has the canonical
manifest hash:

```rust
let mapping = MappingEvidenceV1::new(
    registry_version,
    source_rows,
    observation_rows,
)?;
let record = CaptureSessionRecordV1::new(
    context.session_id,
    context.collector_id,
    context.clock_epoch,
    NativeUuid::new(native.process_session().to_owned())?,
    NativeUuid::new(native.clock_epoch().to_owned())?,
    manifest_hash,
    mapping,
    context.privacy.clone(),
    native.terminal().into(),
    completion.reason,
    completion.partial,
    completion.observation_count,
    native.exit_code(),
)?;
record.validate_against_manifest(&manifest, observations)?;
```

`NativeUuid` accepts exactly the same 36-byte lowercase hexadecimal and hyphen
shape as the current macOS decoder and rejects the all-zero value. The source
clock UUID remains a foreign string beside the canonical `ClockEpochId`; it is
not converted or interpreted as a host timestamp.

The storage adapter then binds the record to already-published capture output:

```rust
let registration = CaptureSessionRegistration::new(
    record,
    manifest,
    observations,
    published_utc_ms,
)?;
let receipt = bundle.register_capture_session_with_cancel(registration, cancel)?;
```

`CaptureSessionRegistration::new` checks the domain manifest/envelope closure
before the SQLite call. `Bundle::register_capture_session` is the default
non-cancellable wrapper. The cancellation-aware method is the composition
entry point when a user stops acquisition or the pipeline loses its caller.

Reads are explicit and bounded:

```rust
let record = bundle.read_capture_session(session_id)?;
let page = bundle.list_capture_sessions(128, after_session_id)?;
let page = bundle.list_capture_sessions_with_cancel(128, after_session_id, cancel)?;
```

The list cursor is an exclusive canonical `SessionId`, ordered lexicographically
by its lowercase encoded form. It is not a SQLite rowid or project revision.
The receipt's `revision` is the positive V1 row marker (`1`), not a measurement
clock or project revision.

## Invariants exercised

The domain contract rejects or preserves the following conditions:

- canonical session, collector and clock IDs remain distinct from foreign UUIDs;
- foreign process/source-clock UUIDs are retained exactly and lowercase;
- terminal status and exit code use the closed native mapping (`0`, `2`, `77`,
  `69`, `70`, `124`, `130`/`143`), and `partial=true` is accepted only with
  `Partial` terminal status;
- observation count is bounded and must match both manifest completion and the
  envelope list during closure validation;
- source and observation mapping vectors are bounded and deterministic;
- duplicate mapping keys/IDs and unsupported mapping fields fail closed;
- a known transmitter radio/BSS requires known identity evidence;
- a capability document's collector identity must equal the record collector;
- known envelope monotonic and synchronization epochs must equal the record
  `ClockEpochId`;
- source sensor/adapter and observation radio/BSS/grouping evidence must equal
  the corresponding mapping rows;
- a known envelope raw-source reference must exactly belong to the manifest
  source-record inventory, including hash, media type and byte length;
- privacy payload retention must equal the manifest raw-source disposition;
- empty captures may retain source mappings from capability evidence but cannot
  link an observation chunk;
- nonempty captures require an exact published chunk and observation mapping;
- canonical bytes are limited to 1 MiB and strict decode rejects noncanonical
  bytes and unknown fields.

The store rehashes the canonical BLOB and compares every indexed projection on
each read. It checks the manifest artifact kind/media type/hash, publication
row, chunk linkage, chunk observation IDs, and the full domain closure again.
`Bundle::verify` walks every capture-session row through this read path and
reports a failure for any unreadable or contradictory row. V1 rows have the
fixed indexed revision marker `1`; an unsupported positive revision is corrupt
until a reviewed schema defines it.
The indexed fields are query projections; they are never used as an
independent cache or authority when the canonical BLOB disagrees.
The session row is immutable; the unique manifest hash prevents one manifest
from being registered under two session IDs.

Cancellation before a metadata commit rolls back the SQLite transaction. A
cancellation after commit may return `StoreError::Cancelled`, but the complete
immutable row remains available for an exact retry. A cancelled list never
returns a validated prefix as if it were a complete page.

## Validation evidence

Commands were run from the capture-session worktree with the locked dependency
graph and no network access:

```text
cargo test -p kyberia-domain --lib --locked --offline
PASS — 11 tests

cargo test -p kyberia-project-store --lib --locked --offline
PASS — 33 tests

cargo test -p kyberia-project-store --test schema_guard --locked --offline
PASS — 14 tests

cargo test --workspace --exclude kyberia-kismet-adapter --locked --offline
FAIL — the existing descendant-drain timing assertion exceeded its 3 s
per-case bound under the full workspace run (`3.261677250 s`); see the retained
`workspace-exclude-kismet-final3.log`. The same test passed in isolation once
(`descendant-drain-repro-final2.log`, 1 passed), so this is a flaky sandbox
timing gate and not evidence against the session record.

cargo test -p kyberia-observation-pipeline --lib --locked --offline
FOCUSED COMPOSITION PASS — 7 tests (see the retained composition log). The
full 42-test invocation is timing-sensitive in this sandbox: one run passed
and a later full-workspace-load run failed the pre-existing descendant-drain
3 s assertion; see the retained `observation-pipeline-lib-final2.log`.

cargo clippy -p kyberia-domain -p kyberia-project-store -p kyberia-observation-pipeline --all-targets --locked --offline -- -D warnings
PASS

cargo fmt --all -- --check
PASS

/Users/vincent/code/kyberia/.tools/venv/bin/python tools/architecture.py
PASS — reviewed dependency directions and external package boundaries

/Users/vincent/code/kyberia/.tools/venv/bin/python tools/source_inventory.py check
PASS — 241 locked external packages

git diff --check
PASS
```

Retained command logs are in
`.trash/test-runs/capture-session-record-20260909/` in the implementation
worktree. The Kismet live HTTP fixture tests are excluded from the reproducible
workspace command because this sandbox cannot create their local sockets. This
is an environment limitation, not evidence about the session record; no native
collector runtime claim is made here.

## Review and remaining gates

The domain and store corrections are separate so the root composition layer can
consume the contract before wiring durable registration. Russell's independent
rereview of the corrected implementation is still required before integration.
Root must add glue tests that use the real opaque session object and the exact
immutable mapping context for both an empty terminal capture and a nonempty
capture. Those tests must prove the explicit registry version is recorded and
that process UUID, source clock UUID, canonical session/collector/clock IDs,
manifest hash, privacy, terminal state and mapping evidence all survive
reopen.

This increment does not validate CoreWLAN authorization or scan fidelity, a
hardware-backed collector, clock offset/pose uncertainty measurement, a
registry's external truth, Windows/Linux runtime behavior, spool durability,
CLI/UI cancellation, authenticated authorship, or the full storage Gate C and
native capture gates. No historical session records are guessed during
migration, and no raw packet bytes are copied into the session table.
