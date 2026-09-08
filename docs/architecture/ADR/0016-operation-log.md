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
The current scope does not apply mutations to the full project aggregate,
persist operation rows, sign actor claims, or render conflicts; those remain
open integration work and must use this contract rather than weaken it.

## Evidence

The focused executable evidence is recorded in the
[operation-log validation document](../../validation/operation-log.md). Its
[operation-log test suite](../../../crates/operation-log/tests/operation_log.rs)
covers unequal-length joins, equal-effect frontier collapse at width 100,000,
bounded ancestry failure, concurrent undo/redo races, exact conflict
resolution, strict encoding, tamper detection, and replayability. The
[architecture test](../../../crates/operation-log/tests/architecture.rs)
checks that the crate remains inward and free of storage, packet, wall-clock,
and generic JSON-value dependencies.

## Reversibility

This Phase 0 increment is reversible before downstream integration: it adds a
pure crate and documentation, without changing project-store schemas,
materialization, or the legacy `kyberia-domain::project::OperationRecord`.
Reverting the crate and its workspace, architecture, and source-inventory
entries restores the prior implementation boundary. Durable persistence and
application adoption must first pass the reserved ADR-0014 persistence and
ADR-0015 channel decisions, with migration and recovery fixtures reviewed at
those boundaries.

## Validation plan

The current contract checks are the focused command, query, merge, encoding,
and replay suite in the
[validation record](../../validation/operation-log.md), run with
`cargo test -p kyberia-operation-log --locked --offline`, together with
`cargo fmt --all -- --check`, workspace clippy with `-D warnings`, the
architecture check, source-inventory check, ledger check, and
`git diff --check`. Before product promotion, the remaining gates are crash-safe
operation-row persistence and reopen/recovery, source-hash verification and
migration fixtures, typed mutation materialization with entity-reference
checks, authorization/signatures, reordered and duplicated delivery with
replay protection, content-chunk availability, and bounded coordinator merge
stress. The concrete follow-up scope is maintained in the validation
document; these gates are outside this pure crate.
