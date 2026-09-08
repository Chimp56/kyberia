# Canonical project materialization task

Status: IN_PROGRESS (architecture investigation; bridge implementation remains open).
Canonical name-command prerequisite integrated in `4482b36`.
Requirements: plan §§10.5–10.8, source-qualified `backlog:FND-011:1`,
and ADR 0016's explicitly open application bridge.

## Existing boundaries and missing behavior

`kyberia-domain::project::Project` owns geometry and validates linear command
receipts. `kyberia-operation-log` owns immutable operation DAGs and produces
typed replay effects. `project-store` persists those exact operation bytes.
The replay field map is not a canonical project: it does not validate site
existence, calibration ownership, or evidence-locked coordinate frames.

The operation schema currently admits project/site renames, calibration
activation, and floor-evidence binding. The canonical project command enum
now includes reviewed project/site rename variants with exact inverse receipts. Calibration activation and evidence binding have
additional aggregate invariants, including frame migration restrictions and
non-reversible evidence binding. A bridge must preserve these invariants and
cannot assume that a structurally valid inverse describes the prior state.

## Implementation scope and ownership

Use an isolated worktree. Own a pure application materialization module,
its tests, and the narrowly necessary domain command extensions. Depend
inward on canonical domain and operation-log contracts. Do not import storage,
UI, operating-system, or foreign adapter implementations. Do not modify
capture/session or stored-RSSI work. Retain historical receipt fixtures.

Bind materialization to an explicit validated baseline project and exact
operation-set identity. Establish the relationship among baseline revision,
operation revision, logical time and causal depth; none are interchangeable.
Replay must return a canonical project or a structured error without partial
publication. Admit supported mutations through domain invariants. Validate
inverse claims against the relevant causal state before using them for undo.
Unresolved conflicts must remain explicit; deterministic ordering is not
permission to select a winning edit. Do not silently omit unsupported effects.

Determine a migration path from the existing linear receipt history with
evidence before changing a persisted schema. Record durable baseline,
revision and migration decisions in an ADR with alternatives and
reversibility. A reviewed pure bridge is a prerequisite to transactional
stored-project publication, which remains a separate integration task.

## Acceptance tests

Tests must exercise canonical project state, not only a field map:

- Rename a real project/site, replay exact receipts, and undo/redo while
  preserving geometry and identity.
- Reject missing sites, cross-map calibration IDs, incompatible frames,
  evidence-locked calibration changes, and invalid evidence references.
- Reject forged inverse metadata that is structurally valid but does not
  match the causal prior value.
- Reject baseline/project mismatches and unresolved concurrent edits.
- Reproduce resolved independent branches under permuted input order;
  distinguish causal depth from materialized project revision.
- Preserve immutable input state after every failure, cancellation where
  exposed, and resource-limit rejection.
- Keep legacy serialized fixtures readable and define explicit unsupported
  behavior for any effect that cannot preserve domain invariants.

Run affected domain/operation suites, workspace architecture, formatting,
Clippy/typecheck, and broader regression checks. Obtain independent review
before integration. Report the required ten-field handoff, including exact
requirements still open; do not claim complete command/query integration.

## Architecture investigation findings

Independent investigation identified V1 gaps before implementation: mutation
inverses cannot represent non-reversible floor evidence binding or an unknown
prior active calibration; effect-only replay cannot establish causal inverse
truth; baseline/set identity and aggregate cross-field failures require explicit
contracts. These are actionable implementation prerequisites, not external
blockers. A versioned inverse/replay extension must preserve legacy bytes and
support the complete domain semantics rather than silently skipping effects.
The earlier FND-006 assignment was incorrect: plan §18.1 assigns Parquet to
FND-006 and operation log/undo to FND-011.

V2 implementation review must include a typed replay path: encoding an unknown
calibration prior is insufficient if replay converts it back to V1 Mutation
and rejects it as TypedPriorRequired. The pure operation API must expose usable
unknown-state restoration effects while preserving legacy V1 compatibility;
storage/materializer integration can then consume that versioned contract.
