# ADR-0023: Transactional publication of canonical materialized projects

- Status: Accepted bounded publication adapter; product commands and cross-platform runtime gates remain open
- Date: 2026-09-09
- Requirements: plan §§10.5–10.8, FND-011, ADRs 0016/0021/0022

## Context

The pure materializer validates causal operations against a canonical baseline.
Persisting its output needs a separate outer adapter: an immutable operation log
is not itself a usable project aggregate. Baseline revision, operation-store
revision, materialized revision, Lamport time and bundle revision have different
meanings. Concurrent appends must not publish stale computation as current.

## Decision

Register one explicit canonical baseline per project, independently of publication.
The registration is immutable; identical retries return its identity, while a
same-project replacement is rejected. Loading a baseline validates its canonical
artifact, registration metadata and project ownership. Legacy absence remains
explicit and migrations never invent project geometry or starting state.

Publish results under an immediate SQLite transaction, binding baseline identity,
exact operation-prefix identity, protocol/result schema versions, output artifact
and operation revision. Validate current inputs inside that transaction. Exact
historical retries validate the existing immutable publication before applying a
current-revision precondition, so a lost reply can be retried after later appends.
A retry does not advance any revision or replace a newer current pointer.

Content-addressed artifacts precede metadata publication. SQLite remains the
committed authority; the redundant JSON manifest is recoverable. A fault between
projection replacement and SQLite commit leaves a detectable stale projection.
An orphan artifact never becomes current merely by existing on disk.

Reads validate metadata and artifact hashes and compare the result with canonical
materialization of the verified baseline and historical operations. Self-consistent
checksums alone cannot establish that result fields follow from the operation log.
Whole-history verification reuses one validated inventory. A caller-owned cumulative deterministic work/copy budget spans the historical
verification transaction, including baseline decoding, operation inventories,
replay and result checks. Scalar metadata lengths and actual artifact sizes are
checked before owned allocation. Resource exhaustion must remain distinct from corrupt
evidence and must not yield a successful partial verification.

## Alternatives

- Implicitly trust the first publication's baseline: rejected because computation
  would select its own authoritative starting state.
- Store only a current JSON project: loses immutable input/result bindings and
  cannot support deterministic historical retry or corruption diagnosis.
- Accept checksum-consistent results without replay: rejected by the semantic
  substitution regression, which changed a valid project name and all its hashes.
- Derive current state automatically from any operation field map: bypasses domain
  geometry and causal-prior validation established by ADR0022.

## Evidence and validation plan

See [draft validation](../../implementation/materialized-publication-draft-validation.md)
and the executable publication tests. Coverage includes two-handle registration,
read-only/legacy access, stale revisions, exact retry after append, corrupted files,
metadata tampering, semantic result substitution, and projection/commit faults.
Independent review confirmed replay comparison and inventory reuse. Shared
budgets and the subsequent metadata/artifact preflight correction resolve the
aggregate Rust-side findings; see the [correction review](../../reviews/materialized-publication-correction-review.md).
Current-main integration passes 615 workspace tests with nine ignored runtime
gates, plus two focused file-growth/truncation tests. Full Windows crate build
and runtime validation remain open; the exact Windows file-open branch compiles
against the pinned dependency in a retained cross-target harness.

## Consequences

The adapter imports inward domain/materializer contracts; pure crates never import
SQLite. Replaying historical results costs computation and needs explicit limits.
This design establishes internal consistency, not authenticated authorship or
protection against an attacker who replaces all evidence consistently. Product
command/query integration, conflict UI and authenticated collaboration remain open.

## Reversibility

Publication tables form an optional additive group in the directory bundle. Older
bundles remain readable with absent materialization. Future baseline replacement
or compaction needs a new versioned migration preserving original evidence; it
must not silently rewrite this immutable registration. This increment is unreleased;
its schema is not yet a compatibility promise to external users.
