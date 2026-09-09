# ADR-0020: Versioned inverse states for project operations

- Status: Accepted bounded contract; independent review pending
- Date: 2026-09-08
- Supersedes: none
- Related: [ADR-0016](0016-operation-log.md), plan §10.7, FND-011

## Context

ADR-0016 delivered a closed version-1 operation format. Its apply inverse is
another `Mutation`, which is sufficient for known text and calibration values
but cannot represent an explicitly unknown calibration prior without inventing
an identifier. The existing `BindFloorEvidence` domain command also returns an
unknown/non-reversible prior: treating it as an executable inverse would claim
that a content-addressed evidence binding can be safely removed.

The operation-log contract must retain those states for audit and later
materialization while preserving already persisted V1 bytes and SHA-256 hashes.
The causal aggregate baseline and full project materializer are separate work;
this decision does not prove that a recorded prior matches a reconstructed
`Project`.

## Decision

Add operation schema V2 as an additive, explicit inverse representation:

- V2 reversible apply/resolve operations use `InverseMetadata::ApplyV2` and a
  closed `InversePrior` (`ProjectName`, `SiteName`, or `MapCalibration`).
- `MapCalibration` carries `Evidence<CalibrationId>`. The only admitted unknown
  prior is `Unknown(NotMeasured)`, matching the domain's calibration admission
  semantics. Legacy replay conversion returns `TypedPriorRequired` for that
  state rather than manufacturing an ID or dropping the inverse.
- A V2 floor-evidence apply uses `NonReversible { reason:
  FloorEvidenceBinding }`. A V2 undo or redo targeting it is rejected during
  operation-set graph validation and linear append admission.
- V2 toggle and resolution references remain V2 operations. Cross-version
  toggles and resolution references fail with `VersionMismatch`; callers must
  use an explicit migration/application bridge when combining old history with
  new commands.
- V1 constructors, canonical unsigned bytes, content hashes, and decoding
  rules remain unchanged. V1 remains readable; no stored operation is rewritten
  in place. Pre-V2 readers continue to reject V2 bytes as unsupported or
  malformed.

The V2 types are bounded by the existing operation limits, closed serde enums,
and domain bounded identifiers/text. Unknown serde variants and forged
field/entity identity combinations fail before canonical hashing.

## Alternatives considered

1. **Keep V1 and encode an unknown calibration as a zero/sentinel ID.**
   Rejected: it destroys the distinction between unknown and known state and
   would make undo scientifically false.
2. **Mark every apply as executable and use an empty inverse for floor evidence.**
   Rejected: it silently permits an irreversible evidence binding to be undone.
3. **Rewrite V1 metadata in storage when reading it.** Rejected: it changes
   content hashes and invalidates immutable history. A future reviewed bridge
   may create a new V2 operation with independently validated prior state.
4. **Make the operation log depend on the project materializer.** Rejected:
   the pure inward contract must remain independent of storage and aggregate
   reconstruction; baseline validation belongs to the later application layer.

## Consequences

V2 can faithfully carry the existing domain's known and unknown calibration
states and can make irreversible floor-evidence operations auditable. Existing
V1 artifacts retain exact wire compatibility. Consumers of the current
`OperationSet::replay` mutation-only API receive an explicit error when a V2
unknown prior cannot be represented as a legacy mutation; they never receive a
made-up mutation or a silently skipped event. A later materializer must consume
`InversePrior` directly (or consume `OperationSet::replay_effects`) and apply it
against a validated causal baseline.

`MergeConflict` now retains both complete `AppliedEffect` values. A concurrent
unknown calibration prior is therefore an inspectable conflict arm rather than
an attempted conversion to a fabricated mutation. Mutation-only callers can
use `left_mutation()` or `right_mutation()`, which return `None` for an unknown
typed calibration. Merge identity canonicalizes `Evidence::Known(id)` typed
calibration effects to the equivalent `ActivateCalibration` value, while
preserving the typed event for replay and audit. This prevents a conflict from
being manufactured solely by the V2 representation used by one branch.

V2 resolution validation compares these typed semantic identities, so a
known/unknown divergence can be resolved by a new selected mutation. The
resolution still records a typed prior and exact operation references. The
causal aggregate baseline and full project materializer remain separate work;
this contract does not claim that either recorded prior matches reconstructed
project state.

The existing outer `project-store::replay_operations` endpoint is also
mutation-only and will return the same explicit error for a persisted V2
unknown-prior undo until its adapter is migrated to `replay_effects`. This
increment does not alter that outer crate.

This increment intentionally does not implement aggregate baseline hashes,
causal inverse validation against arbitrary concurrent branches, authorization,
signatures, or project-store migration. A V1 floor-evidence operation remains
structurally readable for historical audit, but its legacy mutation inverse is
not retroactively reclassified; a materializer must apply an explicit reviewed
migration policy before treating that history as executable.

## Evidence and validation

The focused suite in
[`crates/operation-log/tests/operation_log.rs`](../../../crates/operation-log/tests/operation_log.rs)
checks a fixed V1 canonical byte/hash fixture, V2 known and unknown calibration
round trips, typed unknown/known concurrent conflict inspection and
resolution, representation-independent known-effect convergence,
domain-identity and unknown-reason validation, explicit non-reversible
binding, cross-version toggle rejection, forged shapes, and deterministic
decoding. The architecture test remains at
[`crates/operation-log/tests/architecture.rs`](../../../crates/operation-log/tests/architecture.rs).

Validation commands for this increment are:

```text
cargo test -p kyberia-operation-log --locked --offline
cargo clippy -p kyberia-operation-log --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
python3 tools/architecture.py
git diff --check
```

## Reversibility

The additive V2 code can be disabled before any V2 operation is persisted;
existing V1 bytes and hashes do not change. Once V2 operations are published,
older readers must retain their explicit V2 rejection behavior and an
application must preserve those immutable rows until a reviewed reader or
migration is available. No rollback deletes or rewrites operation history.
