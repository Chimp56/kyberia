# ADR-0021: Canonical materialization input identities

- Status: Accepted bounded prerequisite; materializer integration remains open
- Date: 2026-09-08
- Related: plan §§10.5–10.8, FND-011, ADR-0016, `project-materialization-packet.md`

## Context

Materialization must bind a validated canonical project baseline to the exact
operation set being applied. A project revision, operation-log
`ProjectVersion`, Lamport logical time, and operation `CausalDepth` describe
different things and cannot serve as interchangeable identities. The existing
domain project is serializable and validated, while the operation log exposes
validated immutable operations but does not own storage or materialization.

## Decision

Introduce the pure `kyberia-materialization-identity` crate with two immutable
content-addressed records:

- `BaselineIdentity` uses a versioned, domain-separated binary envelope that
  repeats project ID, domain revision, domain logical time, and exact canonical
  `Project` JSON bytes.
- `OperationSetIdentity` uses a versioned, domain-separated binary envelope
  containing project ID and operation entries sorted by `OperationId`. Each
  entry includes its ID, content hash, and complete canonical operation wire
  bytes. Decoding validates operations and reconstructs the bounded DAG before
  accepting the identity.
- `MaterializationIdentity::bind` checks the project IDs and exposes both
  hashes plus baseline metadata. It does not replay or apply effects.

Each identity is bounded to 64 MiB. SHA-256 is computed over an explicit
identity domain and the complete envelope. Existing domain and operation bytes
remain unchanged.

## Alternatives

1. Use domain revision or operation-log `ProjectVersion` as the baseline/set
   identity. Rejected because both are counters, do not bind bytes, and have
   different semantics.
2. Hash only sorted operation IDs or content hashes. Rejected because the
   identity must bind the exact immutable operation bytes and membership.
3. Use generic JSON maps or an external storage manifest as the canonical
   identity. Rejected because generic maps weaken the closed contract and a
   storage adapter must not become the inward source of truth.
4. Add baseline fields to the operation-log schema immediately. Deferred to the
   operation/materializer integration review; this prerequisite can bind an
   externally supplied baseline without changing V1 operation bytes.

## Evidence and validation

Tests cover same-revision baseline changes, canonical round trips, trailing and
tampered bytes, operation input permutations, membership/content changes,
empty sets, graph validation on decode, project mismatch, and separation of
baseline counters from operation-set count. Focused commands are recorded in
the implementation handoff and must pass before integration.

## Consequences

Materializers and storage adapters have stable opaque hashes for exact input
binding and can detect stale or substituted baselines. The identity layer does
not claim causal-prior verification, conflict resolution, replay, migration, or
transactional publication; those remain separate responsibilities.

## Reversibility

The crate is additive and can be removed before persisted identity artifacts are
published. Once published, the versioned envelopes and hashes are immutable;
future semantic changes require a new identity version rather than rewriting
existing artifacts.
