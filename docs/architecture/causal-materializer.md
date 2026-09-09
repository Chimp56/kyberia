# Canonical causal project materialization

`kyberia-causal-materializer` is the pure application boundary between the
immutable operation DAG and `kyberia-domain::Project`. It binds a validated
baseline and an exact `OperationSet` through
`kyberia-materialization-identity`, validates each V1/V2 inverse against the
operation's causal state, rejects unresolved conflicts, and applies typed
effects through domain invariants. It returns an actual canonical `Project`,
not a field map, and performs no storage publication.

The domain's legacy linear receipt path remains V1 and retains its strict
logical-time rule. A successful DAG materialization uses the project-scoped
`ProjectSchemaVersion::V2` encoding. Its dense `Project::revision` is a local
materialized sequence; operation Lamport time, causal depth, and the
operation-set identity remain separate. V2 permits equal-Lamport independent
effects while retaining the maximum applied operation time and rejects a
nonempty zero-time history. Existing V1 project JSON remains readable and
unchanged.

Before an apply prior is checked, the materializer constructs the project
state from the operation's causal ancestors. A prior that is structurally
valid but differs from that state is rejected. A resolution prior is checked
against the common causal subgraph of the two referenced heads. Its maximal
common field frontier must be unambiguous, so sequential edits inside either
conflict arm do not turn deterministic presentation order into a fabricated
prior. Concurrent causal heads with incompatible prior values remain
ambiguous until an explicit resolution.

The domain also enforces aggregate constraints that are wider than an
operation field key. In particular, concurrent floor-evidence binding and
calibration activation for the same floor are rejected as an explicit
aggregate conflict rather than accepted or rejected according to operation-ID
sort order. Irreversible V2 floor-evidence binding is executable only with its
closed `NonReversibleReason`; V1 binding is reported unsupported because its
legacy inverse cannot prove reversibility.

Materialization retains bounded invocation-wide causal witness work and rejects
oversized sets before replay. It also enforces two separate copy safeguards:
the estimated serialized size of each candidate state includes operation-induced
data growth, and a cumulative serialized-byte copy-work proxy charges every
baseline and domain clone, including repeated ancestor reconstruction. These
byte values are allocation/work accounting proxies, not a claim about the
resident size of Rust `BTreeMap` allocations. It borrows all inputs and
constructs each candidate project on a clone, so a failure cannot partially
mutate the caller's baseline or operation set. Storage adapters remain
responsible for publishing the returned project and identity transactionally.
