# ADR-0024: Shared cumulative materialization resource budget

- Status: Accepted bounded contract; independently reviewed with storage transaction adoption
- Date: 2026-09-09
- Related: plan §§10.5–10.8, FND-011, ADR-0022, `materialization-verification-budget-packet.md`

## Context

Operation-log validation and causal materialization perform graph traversal,
conflict inspection, witness reconstruction and project cloning. Their legacy
entry points each enforce a local invocation limit. A storage verifier that
checks several historical publications could therefore reset the same limits
for every publication and accept an unbounded total workload. The inner
packages must share a deterministic accounting contract without importing
SQLite, operating-system cancellation, or application services.

## Decision

Add the pure inward `kyberia-resource-budget` crate. A caller creates one
`ResourceBudget` with explicit limits and passes it through all related work.
The budget cumulatively charges six stable categories:

1. operation ancestry visits;
2. causal witness visits and scans;
3. aggregate conflict relation checks;
4. canonical operation bytes retained by an operation set;
5. estimated bytes copied while applying project effects; and
6. estimated bytes retained by replay collections.

Charges are checked before the associated work or allocation. Failed charges
do not mutate usage, checked arithmetic reports a deterministic limit error,
and cancellation is polled even for a zero-sized charge. A caller may provide
a pure `CancellationHook`; the default wrappers use `NeverCancel` and retain
the previous per-invocation limits and public error labels.

The operation log exposes budget-aware operation-set construction, replay,
replay-semantic validation and conflict traversal. The causal materializer
exposes `materialize_with_budget`; its existing `materialize` function creates
the compatibility budget. The materializer's defaults allow the separate
validation/replay passes while preserving their individual maxima. The storage adapter creates one budget for a publication verification
transaction and charges decoded artifacts and prefix copies before retaining
them. The identity
crate exposes budget-aware baseline serialization and allocation-free operation
wire-length preflight so an exhausted shared budget rejects before constructing
the final identity buffers.

## Alternatives

1. **Keep independent per-call counters.** Rejected because historical
   verification can multiply the intended work limit by the number of
   publications.
2. **Use a global mutable quota.** Rejected because it violates deterministic
   replay, complicates concurrency and leaks application state into domain
   code.
3. **Stop after a history-count threshold.** Rejected because a small causal
   graph can have expensive fan-out and a large set can be cheap; counters
   must charge actual bounded operations and allocation proxies.
4. **Treat the counters as a resident-memory ceiling.** Rejected because Rust
   collection overhead is implementation-dependent; the contract labels byte
   values as deterministic estimates and the outer adapter owns process-level
   memory policy.
5. **Make cancellation an OS or storage concern inside the crates.** Rejected
   because the pure boundary only needs a replaceable polling hook.

## Evidence

The [independent consumer review](../../reviews/shared-budget-review-in-progress.md)
approves the series through `b5dec0c` with 96 passing combined tests. The two
additional [writer regressions](../../reviews/identity-budget-writer-tests-review.md)
have separate independent approval and mutation evidence. The subsequent [independent merge review](../../reviews/budgeted-operation-merge-review.md)
approves `a29967a`: public merge now charges retained copies before cloning,
shares graph/conflict counters, and uses finite default byte limits. Topological
ordering precharges node/edge allocation proxies and polls cancellation.

Focused tests cover cumulative exhaustion across repeated replay and
materialization calls, stable usage under input permutation, operation-byte
preflight, duplicate-input admission bounds, referenced toggle-payload
charging, failed-charge atomicity, checked overflow, cancellation including a
zero-sized charge, cancellation propagation through both domain boundaries,
and the existing deep-chain per-job copy limit. Existing operation-log and
causal-materializer suites remain green.

## Consequences

The same budget can prevent a verifier from resetting ancestry, witness,
aggregate, structural-work or copy quotas between calls. The counters are
monotonic for one job; there is no refund when temporary collections are
dropped. This intentionally bounds cumulative work rather than estimating
peak live memory. A storage adapter must use a single budget and charge its
own decoded baseline, operation-prefix and result allocations before making
them durable. Error mapping preserves legacy labels at existing public
wrappers while budget-aware callers receive the corresponding structured
category at the inward budget boundary.

## Reversibility

The crate is dependency-free and replaceable behind a small pure API. Existing
wrappers remain source-compatible. Removing it later requires restoring a
caller-owned cumulative equivalent in every verifier, so that change requires
new evidence and an ADR update.

## Validation plan

Storage integration passes one budget through inventory validation, baseline
and prefix decoding, materializer replay and publication-result verification;
see the [independent correction review](../../reviews/materialized-publication-correction-review.md).
It must test repeated historical publications, deterministic permutation,
budget exhaustion before allocation, cancellation before and during reads,
and transaction rollback after a resource error. Runtime memory monitoring
and platform cancellation remain outer-layer responsibilities.
