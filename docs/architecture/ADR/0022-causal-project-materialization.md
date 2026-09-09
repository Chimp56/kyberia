# ADR-0022: Canonical causal project materialization

- Status: Proposed bounded application contract; independent review and durable project publication remain open
- Date: 2026-09-08
- Related: plan §§10.5–10.8, FND-011, ADR-0016, ADR-0020, ADR-0021, `project-materialization-packet.md`

## Context

The operation log provides a validated immutable DAG and typed effects, while
the domain `Project` owns geometry, calibration references, frame locks and
evidence invariants. Applying a replay field map would accept missing sites,
cross-map calibrations or evidence-locked changes and could treat a
presentation-order predecessor as an inverse prior. Reusing the legacy linear
receipt method also rejects equal-Lamport independent branches.

## Decision

Add the pure `kyberia-causal-materializer` application crate. It:

1. binds a validated baseline project and exact operation-set identity;
2. validates each operation's inverse against the state formed by its causal
   ancestors, with resolution priors checked against the common causal
   subgraph of the referenced heads. Its maximal common field frontier must
   be unambiguous;
3. consumes typed V2 calibration effects, including
   `Unknown(NotMeasured)`, and rejects unsupported V1 irreversible binding;
4. rejects unresolved operation conflicts and cross-field floor-lock versus
   calibration conflicts before deterministic effect presentation;
5. applies effects through a separate domain method that clones and validates
   each result; and
6. returns the canonical project plus both input identities without writing
   storage.

The domain adds project-scoped `ProjectSchemaVersion::V2`. V1 project bytes and
linear `Project::execute` behavior remain unchanged. V2 separates the dense
materialized revision from operation logical time and causal depth, while
requiring a positive logical time for nonempty histories and preserving the
maximum applied operation time. This is an explicit versioned migration rather
than silently inflating a Lamport timestamp to a revision counter.

## Alternatives

1. **Apply the operation field map directly.** Rejected because it bypasses
   entity, frame, calibration and evidence invariants.
2. **Reuse `Project::execute` for every effect.** Rejected because its strict
   linear logical-time and expected-revision admission rejects valid equal-time
   independent DAG branches.
3. **Use operation-ID sort order as the merge winner.** Rejected because
   deterministic presentation cannot resolve semantic conflicts.
4. **Allow V1 project JSON to carry DAG counters.** Rejected because it would
   reinterpret legacy receipt semantics and make Lamport time indistinguishable
   from local materialized revision.
5. **Persist materialized projects in this crate.** Rejected because SQLite,
   chunks, transactions and publication recovery belong to an outer adapter.

## Evidence and validation

The validation record covers project/site renames, equal-Lamport independent
branches, causal forged-prior rejection, common-ancestor resolution, typed
unknown-calibration undo, explicit aggregate lock conflicts, baseline identity
binding, V1 byte preservation, V2 round-trip validation and immutable failure
behavior, known-calibration equivalence across typed and legacy effect
representations, exact missing-entity/frame/reference rejection, criss-cross
common-head permutation determinism, and a real causal-chain cumulative
copy-work rejection whose error is permutation-stable. Focused
commands are `cargo test -p kyberia-causal-materializer --locked --offline`
and `cargo test -p kyberia-domain --locked --offline`.

## Consequences and reversibility

The bridge is deterministic, bounded and independently testable. It caps the
invocation-wide causal witness work, estimated per-state serialized growth, and
cumulative serialized-byte copy-work proxy, including repeated ancestor
reconstruction. Those byte values account for allocation/work decisions; they
are not a resident-memory ceiling for Rust collection internals. Larger
operation sets or richer causal histories require a future persistent
causal-state index. Existing V1 receipts and project fixtures remain readable.
V2 project artifacts require a V2-aware reader; reverting an application
binary after publishing them requires retaining that reader or a reviewed
migration. Transactional storage publication, authorization, signatures and a
persistent causal-state index remain later work.
