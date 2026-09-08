# Canonical operation log

`kyberia-operation-log` is the pure command-side contract for project history.
It is an inward crate: it depends on the existing canonical IDs and bounded
text types in `kyberia-domain`, plus serde and SHA-256 for wire and integrity
contracts. It has no project-store, SQLite, packet, UI, wall-clock, or
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
application commands/queries
            |
            v
  kyberia-operation-log  --->  kyberia-domain IDs and bounded value types
            |
            v
project-store / collaboration / materializer adapters (future)
```

The existing `kyberia-domain::project::OperationRecord` remains a legacy
single-project receipt until an application integration deliberately migrates
to this contract. This increment does not silently make either model the
project-store truth.

## Operation format

An operation is a version-1 closed record with these fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Closed semantic format version (`"1"`; future versions fail closed). |
| `operation_id` | Existing canonical `OperationId`; immutable identity. |
| `project_id` | Existing canonical project identity. |
| `actor_id`, `device_id` | Existing canonical actor and `ActorDeviceId` (`DeviceId` is an API alias). |
| `logical_time` | Positive Lamport-style counter supplied by the command layer. |
| `causal_depth` | DAG depth: zero for a root, otherwise one plus the maximum parent depth. It is not a project revision. |
| `parents` | Up to eight sorted causal operation IDs. |
| `payload` | Closed typed `Apply`, `Undo`, `Redo`, or `Resolve` command. |
| `inverse` | Typed inverse mutation, or an immutable target reference for a toggle. |
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

## Command/query separation and append admission

`Operation::try_apply`, `try_undo`, `try_redo`, and `try_resolve` are pure command builders.
They validate IDs supplied by domain constructors, logical time, parent shape,
payload/inverse pairing, and canonical size before computing the digest.
Fields are private, so an operation cannot be assembled by struct literal.

`OperationLog::append` is the command-side linear admission boundary. It
requires the project, the expected causal depth for the single current tip,
and logical counter to match the local context. Its `revision` is a separate
linear materialized-project revision that increments on accepted appends and
is returned in `AppendOutcome`; adapters must never serialize or compare it as
an operation's `causal_depth`. A repeated operation ID with the same digest
returns an idempotent `Duplicate` outcome without advancing revision. The same
ID with different bytes returns `TamperedDuplicate`.

`OperationLog` accessors and `OperationSet::ordered`, `replay`, and
`replay_state` are query-side operations. They do not erase, mutate, or replace
history. The application layer decides how a replayed typed mutation changes a
project aggregate.

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
containing both operation references and both typed effects. Conflict and
frontier limits, plus a deterministic ancestry work budget, fail closed before
unbounded merge work. `MergeOutcome::into_applyable` returns an error while
any conflict exists; there is no last-writer-wins or arbitrary sorting-based
semantic resolution.

`Resolve` is an explicit typed operation. It references two exact,
concurrent, causally joined operations on one field in canonical operation-ID
order; both references must be the operation's current direct parents. It
records the selected mutation and its inverse, and clears only that exact
current conflict pair during merge. Stale ancestor pairs, reversed pairs,
invalid fields, non-concurrent targets, and attempts to resolve equal effects
are rejected. The references remain in the immutable operation for
auditability. Unrelated heads retain their own conflict records.

Undo and redo are operations in this same DAG. An undo points to the original
operation ID and hash and applies its stored inverse mutation during replay. A
redo points to that same immutable original and reapplies its forward mutation.
The target operation is never removed, rewritten, or replaced. Sequential
repeated toggles are rejected at append admission. Concurrent duplicate
same-target, same-direction toggles are one semantic toggle: the first emits
the mutation and later duplicates are deterministic replay no-ops. Undo versus
redo remains a conflict, and toggles of another toggle are rejected during DAG
validation and replay.

## Open integration decisions

Phase 0 intentionally leaves these decisions to the application and
persistence increments:

- mapping each typed mutation to the full `Project` aggregate and checking
  entity-reference existence;
- durable SQLite/bundle tables and crash-safe append transactions;
- server or file exchange for operation DAGs and content-addressed chunks;
- authorization, actor enrollment, signatures, and encrypted project keys;
- migration from the existing linear `ProjectCommand` receipt model;
- UI conflict presentation and authoring of the explicit resolving operation.

Those open pieces do not weaken this crate's admission, digest, causal, merge,
or provenance contracts.
