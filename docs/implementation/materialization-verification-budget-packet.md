# Shared materialization verification budget

Status: IMPLEMENTED in the isolated publication consumer; independent review and
main-branch integration remain open. Requirements: FND-011, plan §§10.5–10.8,
bounded imports and reproducible analysis. Existing per-job limits remain mandatory.

Independent review confirms that semantic replay comparison and inventory reuse
address their respective correctness/decoding issues. They do not bound the sum
of historical prefix cloning, graph replay, baseline decoding and project copies.
SQLite's progress handler does not meter Rust-side computation.

Implement an inward-owned shared work budget usable by operation-log replay and
causal materialization. Preserve existing one-job entry points as wrappers using
the current limits. Add explicit budget-aware entry points. Charge ancestry
visits, witness scans, conflicts, aggregate checks, frontier/event retention and
estimated project-copy bytes before work or allocation. Keep cumulative counters
across publications while retaining per-job rejection thresholds.

The storage adapter must also charge decoded artifact bytes, identity encoding
and prefix/payload copies. Reuse the validated inventory and verified baseline
within one transaction. Process publications in deterministic order. Exhaustion
must return an explicit resource-limited verification result, never success after
skipping history. Cancellation/deadline checks supplement deterministic counters;
wall time alone is not a reproducibility bound. Any resumable cursor must bind to
the exact committed input identity and cannot skip prior integrity failures.

Acceptance tests:

- Increasing valid publication prefixes cumulatively exhaust a shared budget;
  row insertion permutations produce the same resource error.
- A pathological single graph still reaches its per-job bound.
- Prefix and payload copies are charged before allocation.
- Cancellation during a later replay prevents successful verification.
- Below-budget history verifies every result against replay.
- Above-budget history reports incomplete verification explicitly.

Require independent review and appropriate domain/operation/materializer/storage
regressions before integration. No UI, adapter, or storage dependency may enter
pure domain or numerical crates. Budget values require documented units and
calibration evidence; do not treat a serialized-byte proxy as exact heap usage.

The publication consumer now exposes
`Bundle::verify_materialized_project_publication_with_budget`. One caller-owned
`ResourceBudget` is carried through manifest and baseline decoding, operation
inventory validation, every historical prefix, identity generation, replay, and
result verification. A compatibility wrapper creates one finite default budget
for the existing `verify()` path. Publications are read in `publication_id` order;
the verifier never starts a fresh budget per row.

The storage adapter charges a scalar metadata proxy before operation-row text and
metadata vectors, declared BLOB lengths before BLOB queries, and conservative
known-copy proxies before baseline identity decoding, project decoding and
canonical readback. The baseline identity/project and decoded operation inventory
are loaded once per verification transaction. The budget's byte counters are
cumulative deterministic work proxies; they are not resident-heap measurements.

Focused evidence in `crates/project-store/tests/materialized_publication.rs` covers
a valid three-publication history, repeated deterministic exhaustion, cancellation
after replay copy work begins, an empty budget, and a decoder-stage quota boundary.
The test suite currently passes 13 publication tests; full storage regression and
independent review are still required before this packet can be marked accepted.
