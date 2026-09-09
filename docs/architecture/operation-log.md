# Canonical operation log

`kyberia-operation-log` is the pure command-side contract for project history.
It is an inward crate: it depends on the existing canonical IDs and bounded
text types in `kyberia-domain`, the pure `kyberia-resource-budget` contract,
plus serde and SHA-256 for wire and integrity contracts.
It has no project-store, SQLite, packet, UI, wall-clock, or
platform dependency.

## Truth and dependency direction

The operation log owns immutable project operations and their causal metadata.
An application use case creates a checked operation and a project materializer
later applies the resulting typed mutations. Persistence is an outer adapter;
it must store the canonical bytes and hash without making a storage schema part
of this crate. High-volume observations remain append-only chunks in the raw or
normalized evidence planes. An operation may carry only an
`ImmutableReference` containing a SHA-256 digest, media type, and bounded byte
length.

The dependency direction is:

```text
application commands/queries / storage and materializer adapters
                             |
                             v
                  kyberia-operation-log
                       |           |
                       v           v
                kyberia-domain   kyberia-resource-budget
```

The existing `kyberia-domain::project::OperationRecord` remains a legacy
single-project receipt until an application integration deliberately migrates
to this contract. This increment does not silently make either model the
project-store truth.

## SQLite persistence adapter

`kyberia-project-store` is the outer persistence adapter for this contract.
Each immutable row stores the exact operation wire bytes, the exact unsigned
canonical bytes, the SHA-256 content hash, project and operation identities,
logical time, causal depth, and an operation-only `project_revision`. The
adapter re-parses the wire bytes through `kyberia-operation-log`, verifies the
canonical bytes and digest, validates the complete bounded operation graph,
checks replay semantics before accepting sequential toggles (while retaining
unresolved concurrent conflicts), and only then commits the row. SQLite BLOB
lengths are queried as scalars before canonical or wire bytes are selected into
the adapter.

The adapter's `project_revision` is a local linear count of accepted unique
operation rows. It is persisted in `operation_log_state` and is separate from
the `causal_depth` carried by each operation. `bundle_manifest.revision` is a
third counter for all committed bundle metadata, including artifacts and
survey snapshots. An operation append advances both the operation revision
and bundle revision in one SQLite transaction; artifact or snapshot commits
advance only the bundle revision. No adapter may derive one counter from
another.

Operation tables are an optional validated schema group. New bundles create
the group empty; writable opens add it transactionally to older valid bundles
without rewriting the manifest revision. Read-only opens never migrate. A
projection failure rolls the transaction back; if a process stops after the
redundant projection is written but before SQLite commit, verification detects
the projection-ahead state and `recover_manifest` restores the committed
SQLite projection.

## Operation format

An operation is a closed record. Version 1 is the original immutable format;
version 2 is an additive inverse-metadata format. Both have these envelope
fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Closed semantic format version (`"1"` or `"2"`; future versions fail closed). |
| `operation_id` | Existing canonical `OperationId`; immutable identity. |
| `project_id` | Existing canonical project identity. |
| `actor_id`, `device_id` | Existing canonical actor and `ActorDeviceId` (`DeviceId` is an API alias). |
| `logical_time` | Positive Lamport-style counter supplied by the command layer. |
| `causal_depth` | DAG depth: zero for a root, otherwise one plus the maximum parent depth. It is not a project revision. |
| `parents` | Up to eight sorted causal operation IDs. |
| `payload` | Closed typed `Apply`, `Undo`, `Redo`, `Resolve` (V1), or `ResolveV2` command. |
| `inverse` | V1 typed inverse mutation or toggle reference; V2 typed prior, explicit non-reversible reason, or toggle reference. |
| `content_hash` | SHA-256 of the canonical unsigned operation bytes. |

The canonical unsigned representation is compact JSON emitted by serde from a
fixed field order and typed enums. It contains no floats, maps, generic JSON
values, wall-clock timestamps, packet bytes, or observation arrays. `to_bytes`
serializes the full record; `canonical_bytes` returns the exact unsigned bytes
hashed by `content_hash`. `from_bytes` verifies the hash and also rejects
non-canonical input rather than normalizing it silently.

The format limits are deliberately executable: at most 8 parents, 100,000
operations in an in-memory set, 32 KiB canonical bytes, 48 KiB wire bytes,
64 MiB per referenced artifact, 8,192 reported merge conflicts, and 2,000,000
ancestry/work steps per validation or replay pass. These are format safety
bounds, not permission to put observation payloads in commands.

V2 reversible applies and resolutions use `InversePrior` with the same field
identity as the forward value. A V2 resolution uses `ResolutionValue`, which
can carry either a mutation or `Evidence::Unknown(NotMeasured)` calibration;
legacy replay cannot manufacture an ID for an unknown value. Floor-evidence
binding uses `NonReversible(FloorEvidenceBinding)` and cannot be an undo/redo
target. V1 constructors and canonical bytes remain unchanged. Cross-version
toggles and resolution references are rejected explicitly. The mutation-only
replay API returns an explicit typed error when a V2 unknown prior or selected
resolution value cannot be represented as a legacy `Mutation`;
`OperationSet::replay_effects` exposes the typed `AppliedEffect::Calibration`
instead. The later project materializer must consume that typed value against a
validated causal baseline.

## Command/query separation and append admission

`Operation::try_apply`, `try_undo`, `try_redo`, and `try_resolve` are the V1
pure command builders; the corresponding `*_v2` constructors admit typed
priors and explicit non-reversible floor bindings. They validate IDs supplied
by domain constructors, logical time, parent shape, payload/inverse pairing,
and canonical size before computing the digest.
Fields are private, so an operation cannot be assembled by struct literal.

`OperationLog::append` is the command-side linear admission boundary. It
requires the project, the expected causal depth for the single current tip,
and logical counter to match the local context. Its `revision` is a separate
linear materialized-project revision that increments on accepted appends and
is returned in `AppendOutcome`; adapters must never serialize or compare it as
an operation's `causal_depth`. A repeated operation ID with the same digest
returns an idempotent `Duplicate` outcome without advancing revision. The same
ID with different bytes returns `TamperedDuplicate`.

`OperationLog` accessors and `OperationSet::ordered`, `replay`,
`replay_effects`, and `replay_state` are query-side operations. They do not
erase, mutate, or replace history. The application layer decides how a
replayed typed effect changes a project aggregate.

## Causality, total order, and merge

Each parent must be present and have a lower logical timestamp. Root
operations use causal depth zero; a child uses one plus the maximum causal
depth among its parents. A multi-parent operation is a join of concurrent
heads, so redundant parent pairs where one parent is an ancestor of another
are rejected. This permits unequal-length offline branches to join while
preserving a deterministic causal version. Toggle targets must be an ancestor
and must reference an `Apply` operation with the exact digest. A toggle is
also checked against replay state at append admission, so repeated undo/redo
cannot enter the durable log.

`OperationSet::ordered` performs a topological sort. Among ready operations it
uses `(logical_time, actor_id, device_id, operation_id)`, in that order, as the
deterministic total tie-break. Causality always wins over the tie-break.

`OperationSet::merge` performs content-addressed set union and validates the
combined DAG. For the same semantic `FieldKey`, a bounded causal frontier
tracks only maximal effective edits. Two different effective typed mutations
are a conflict only when neither operation is an ancestor of the other. The
result retains both immutable operations and emits a `MergeConflict`
containing both operation references and both complete `AppliedEffect` values.
An unknown calibration undo remains an explicit `AppliedEffect::Calibration`
and is never converted to a sentinel mutation. For merge identity only,
`Evidence::Known(id)` in a typed calibration effect is canonicalized to the
equivalent `ActivateCalibration` value, so equal known undo/activation branches
converge despite their replay representation. Mutation-only callers can use
the optional `left_mutation()`/`right_mutation()` compatibility views.
Conflict and frontier limits, plus a deterministic ancestry work budget, fail
closed before unbounded merge work. `MergeOutcome::into_applyable` returns an
error while any conflict exists; there is no last-writer-wins or arbitrary
sorting-based semantic resolution.

`Resolve` and `ResolveV2` are explicit typed operations. They reference two exact,
concurrent, causally joined operations on one field in canonical operation-ID
order; both references must be the operation's current direct parents. It
records the selected mutation/value and its inverse, and clears only that exact
current conflict pair during merge. Stale ancestor pairs, reversed pairs,
invalid fields, non-concurrent targets, and attempts to resolve equal semantic
values are rejected, except that a same-value conflict between distinct toggle
intents may be explicitly acknowledged by a resolution. Validation uses the
typed effect path, so a known/unknown calibration conflict can be resolved to
either a known mutation or an explicit unknown value. The references remain in
the immutable operation for auditability. Unrelated heads retain their own
conflict records. Resolution operations remain ineligible as undo/redo targets
under this contract.

Undo and redo are operations in this same DAG. An undo points to the original
operation ID and hash and applies its stored inverse mutation during replay;
V2 typed priors are retained and unknown priors are emitted as typed effects by
`replay_effects` while the legacy mutation boundary fails explicitly. A V2
non-reversible target is rejected before admission. A redo points to that same
immutable original and reapplies its forward mutation.
The target operation is never removed, rewritten, or replaced. Sequential
repeated toggles are rejected at append admission. Concurrent duplicate
same-target, same-direction toggles are one semantic toggle: the first emits
the mutation and later duplicates are deterministic replay no-ops. Undo versus
redo remains a conflict, and toggles of another toggle are rejected during DAG
validation and replay.

## Cumulative merge resources

`merge_with_budget(other, &mut budget)` shares the caller's resource counters
across set union, graph validation and conflict inspection. It charges every
retained operation before cloning: canonical bytes use `OperationBytes`, while
a canonical-size proxy for the decoded payload plus a structural entry uses
`WorkingSetBytes`. Equal duplicates are checked without creating another
retained operation. Cancellation is checked during both input traversals and
the graph/conflict work. Inputs remain immutable if any stage fails; no partial
`MergeOutcome` is returned.

`ordered_with_budget` precharges 512 bytes per node for its two maps, heap,
result pointers and capacity overhead, plus 64 bytes per edge for child vectors
and growth overhead. It polls cancellation during admission, map construction,
ready-queue construction, node processing and child traversal. Admission,
replay and conflict inspection all use this budget-aware ordering path.

`merge` uses the same path with finite defaults: 256 MiB of retained canonical
operation bytes, 512 MiB of cumulative working-copy estimates and the existing
two-million ancestry-work limit. These are safety admission policies, not
performance promises. Other legacy convenience APIs retain their existing
format/count/ancestry limits; callers requiring cumulative byte limits use the
budget-aware APIs. Callers verifying
several related merges must reuse a budget rather than calling this convenience
wrapper repeatedly. Counters accumulate attempted work and are not rolled back
when a later stage fails. They are deterministic allocation/work proxies, not a
measurement of live heap or resident memory. Graph errors retain the existing
`MergeError::Operation` wrapper, including nested cancellation/resource errors;
copy and conflict-stage failures use direct merge error variants.

## Open integration decisions

Phase 0 intentionally leaves these decisions to the application and later
collaboration increments:

- mapping each typed mutation to the full `Project` aggregate and checking
  entity-reference existence;
- server or file exchange for operation DAGs and content-addressed chunks;
- authorization, actor enrollment, signatures, and encrypted project keys;
- migration from the existing linear `ProjectCommand` receipt model;
- UI conflict presentation and authoring of the explicit resolving operation.

Those open pieces do not weaken this crate's admission, digest, causal, merge,
or provenance contracts.
