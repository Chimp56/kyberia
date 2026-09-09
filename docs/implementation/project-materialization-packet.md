# Canonical project materialization task

Status: IN_PROGRESS (architecture investigation; bridge implementation remains open).
Canonical name-command prerequisite integrated in `4482b36`.
Requirements: plan §§10.5–10.8, source-qualified `backlog:FND-011:1`,
and ADR 0016's explicitly open application bridge.
Canonical baseline and operation-set identity prerequisite is implemented in
`kyberia-materialization-identity`; replay and aggregate application remain
open under this packet.

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
and rejects it as TypedPriorRequired. The pure operation API and the
project-store adapter now expose usable unknown-state restoration effects while
preserving legacy V1 compatibility; the remaining materializer integration must
consume that versioned contract against a validated causal baseline.

## Independent API audit and executable ordering

The current `Project::execute` rejects any logical time less than or equal to
the aggregate's current logical time. Reusing it unchanged for a DAG therefore
rejects valid concurrent edits to different fields with equal Lamport times.
Keep the legacy linear receipt behavior intact; establish a separate reviewed
materialized-application boundary that preserves aggregate invariants and the
maximum logical time without treating Lamport time as revision.

The validated project exposes serde serialization but no versioned canonical
identity contract. The operation set similarly needs an exact membership and
byte identity independent of arrival order. The first independent prerequisite
is now assigned to `feat/materialization-identity`: domain-separated canonical
baseline and operation-set hashes, project/revision/logical-time bindings,
resource limits and mutation/permutation tests. It must not claim to validate
causal inverse truth or materialize a project merely by hashing inputs.

Subsequent application tests must compare actual causal field states, including
the baseline prior for roots, rather than whichever state happens to precede an
operation in a deterministic presentation. Verify equal-timestamp independent
edits, forged causal priors, unknown restoration, evidence locks, and unresolved
same-field conflicts against complete canonical project outputs. Storage
publication remains downstream of this pure application boundary.

## Aggregate validation constraints found during bridge review

`Project::validate` also requires `logical_time >= revision`, revision equal
to the applied-operation count, and dense unique applied-operation revisions.
Two independent operations at Lamport time `N+1` after a baseline at revision
and time `N` produce revision `N+2` with maximum logical time `N+1`. A new
application entry point alone therefore cannot preserve the current aggregate
validation unchanged. Define a versioned history/counter migration, retain V1
fixtures and decoding, and test serialization round trips of concurrent output.
Do not inflate Lamport time to the revision merely to satisfy the old invariant.

Calibration activation and floor-evidence binding write different operation-log
fields but share a domain lock: `activate` and `BindFloorEvidence` both invoke
`unlocked`. Concurrent valid branches can therefore conflict across fields.
Test both operation-ID orderings and require explicit aggregate conflict/error
semantics rather than allowing deterministic sort order to decide admission.
Also test operation IDs that collide with the baseline's applied-operation
history, not only duplicates within the incoming set.

Conflict checks must use the active causal frontier. In particular, create two
concurrent differing edits, explicitly resolve them, then edit the resolved
value and undo/redo the new edit. Historical disagreement must not make the
resolved descendant permanently ambiguous. Conversely, a resolution elsewhere
in the final set cannot retroactively validate a forged prior on an earlier
branch. Exercise both cases against actual aggregate outputs.

Resource admission must account for the complete request's causal traversals
and retained project state. A per-operation traversal counter and a count of
full-project snapshots do not establish a practical total work or memory bound.
Test resource rejection before retaining excessive baseline copies, with input
state unchanged. Resolution-prior semantics and multi-head common ancestry
need an explicit durable decision and fixtures, not an incidental sorted winner.
