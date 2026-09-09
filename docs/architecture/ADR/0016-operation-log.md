# ADR-0016: Canonical immutable operation log and offline merge

- Status: Accepted for Phase 0 contract work
- Date: 2026-09-07
- Supersedes: none
- Reserved neighbors: ADR-0014 persistence and ADR-0015 channel are reserved by the roadmap

## Context

Section 10.6 of `plan.md` requires commands to mutate through explicit use
cases and queries to read versioned views. Section 10.7 requires project
operations to support undo/redo, offline merge, audit history, and reproducible
state while keeping high-volume sensor observations in append-only chunks.
`kyberia-domain` already provides canonical 128-bit `OperationId`, `ActorId`,
and `ActorDeviceId` types, but its current project receipt is a linear,
application-specific model. It has no operation-set merge, explicit semantic
conflicts, canonical content digest, or immutable toggle references.

## Decision

Create `crates/operation-log` as a pure inward crate. Reuse the domain ID and
bounded text types and expose `ActorDeviceId` as the readable `DeviceId` alias;
do not create duplicate identity types. Define one closed version-1 typed
operation format with:

- positive Lamport-style logical time and explicit causal parent IDs;
- explicit DAG causal depth and deterministic total ordering;
- typed project mutations (`SetProjectName`, `SetSiteName`, calibration, and
  floor-evidence reference binding);
- typed inverse metadata validated against the mutation's field key;
- immutable `Undo`/`Redo` references containing target ID and content hash;
- typed `Resolve` references to two concurrent heads, a selected mutation, and
  an executable inverse;
- canonical unsigned bytes plus SHA-256 content address;
- strict serde, future-version rejection, and bounded resource admission;
- an append validator with idempotent exact duplicates and tamper errors;
- DAG union/ordering/replay with explicit incompatible-concurrent-edit records;
- multi-parent joins use one plus the maximum parent causal depth, allowing
  unequal offline branch lengths while rejecting redundant ancestor parents;
- conflict detection uses a bounded per-field causal frontier, and a valid
  resolving descendant clears only the conflict pair it references;
- concurrent duplicate toggles of one exact target and direction collapse to
  one deterministic replay effect; mixed undo/redo intent remains a conflict;
- content-addressed artifact references with no raw observation bytes.
- the outer `project-store` adapter persists exact wire/canonical bytes and
  hashes transactionally, with a separate operation-only project revision;
  this adapter does not make SQLite types part of the inward contract.

The immutable operation's `causal_depth` is separate from the linear
`ProjectVersion` revision maintained by `OperationLog`; adapters must keep
those fields distinct in persistence and APIs. Semantic conflicts remain
unresolved until an application creates a new,
explicit `Resolve` operation that references both conflicting heads. The merge
API exposes deterministic inspection order but `into_applyable` fails while
unresolved conflict records exist.

## Alternatives considered

1. **Extend `kyberia-domain::project::OperationRecord`.** Rejected for this
   increment because the current model couples operations to one aggregate's
   linear revision and receipt event. A later application adapter can bridge
   the models without making domain storage-aware.
2. **Put operations in `project-store`.** Rejected: storage would become the
   canonical command model, violating the inward dependency direction and
   preventing deterministic in-memory validation and replay.
3. **Use a generic JSON command envelope or packet event stream.** Rejected:
   closed typed variants, strict fields, bounded inverse metadata, and separate
   raw/normalized observation planes are required by the architecture.
4. **Resolve merge conflicts by logical timestamp.** Rejected: total order is
   for deterministic presentation/replay only; incompatible concurrent values
   must remain explicit conflicts.

## Consequences

The core contract can be tested on every platform and persisted by multiple
adapters without importing UI or storage types. Offline branches can be merged
deterministically while retaining provenance and refusing silent data loss.
The project-store increment persists immutable operation rows and validates
their exact bytes, hashes, project identity, parent graph, and replay before
commit. The current scope does not apply mutations to the full project
aggregate, sign actor claims, or render conflicts; those remain open
integration work and must use this contract rather than weaken it.

## Evidence

The pure-contract executable evidence is recorded in the
[operation-log validation document](../../validation/operation-log.md). Its
[operation-log test suite](../../../crates/operation-log/tests/operation_log.rs)
covers unequal-length joins, equal-effect frontier collapse at width 100,000,
bounded ancestry failure, concurrent undo/redo races, exact conflict
resolution, strict encoding, tamper detection, and replayability. The
[architecture test](../../../crates/operation-log/tests/architecture.rs)
checks that the crate remains inward and free of storage, packet, wall-clock,
and generic JSON-value dependencies.
The outer persistence evidence is recorded in the
[operation-store validation document](../../validation/operation-store.md),
whose focused tests cover exact duplicate retry, canonical/hash tamper
detection, bounded scalar BLOB-length reads, sequential toggle admission,
additive migration, read-only access, transactional rollback, projection-ahead
recovery, reopen/replay, and the outward typed replay of V2 unknown calibration
effects and resolved unknown conflicts. The legacy mutation-only replay API
remains available for V1-compatible consumers and reports an explicit typed
error for an unrepresentable unknown effect.

## Reversibility

The pure contract and the persistence adapter can be disabled before
downstream materialization without changing project state. The adapter's
schema change is additive: new bundles create empty operation tables, and
older bundles gain those tables only through a writable migration that leaves
the manifest revision unchanged. After a bundle has migrated or accepted
operation rows, reverting binaries alone is unsafe because an older schema
guard would reject the additional tables; recovery requires retaining the
operation-aware reader or a reviewed migration, and operation rows must never
be deleted as rollback. The legacy
`kyberia-domain::project::OperationRecord` remains untouched.

## Validation plan

The pure contract checks remain in the
[operation-log validation record](../../validation/operation-log.md). The
SQLite adapter checks in the
[operation-store validation record](../../validation/operation-store.md) run
with `cargo test -p kyberia-project-store --locked --offline`. The combined
increment is formatted and linted with `cargo fmt --all -- --check` and
workspace clippy with `-D warnings`, then checked by the architecture,
source-inventory, ledger, and `git diff --check` gates. Before product
promotion, the remaining gates are typed mutation materialization with
entity-reference checks, authorization/signatures, reordered and duplicated
delivery with replay protection, content-chunk availability, and bounded
coordinator merge stress. Persistence crash/recovery, migration, and source
hash checks are covered by the current adapter fixtures; production power-loss
and cross-platform validation remain open.
