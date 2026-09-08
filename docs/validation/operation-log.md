# Operation-log validation evidence

The Phase 0 validation suite is in
`crates/operation-log/tests/operation_log.rs` and
`crates/operation-log/tests/architecture.rs`.

## Executable coverage

The focused tests demonstrate:

- inverse, undo, and redo replay with the original immutable operation retained;
- repeated undo/redo admission rejection with unchanged revision, tip, and
  operation count;
- exact duplicate append idempotence and same-ID digest tamper detection;
- concurrent branch conflict records and refusal to produce an applyable merge;
- a 10,000-operation same-field causal chain proving frontier merge behavior
  without all-pairs conflict scanning;
- a width-100,000 equal-effect root set proving equal frontier representatives
  collapse without the conflict cap;
- concurrent duplicate undo and redo races replaying one effect, mixed
  undo/redo intent conflicts, and descendant join behavior;
- a deep hostile toggle chain that fails with the bounded ancestry-work error;
- cyclic and self-parent graphs rejected before topological replay;
- an unequal-length offline branch join using the maximum parent causal depth,
  plus invalid-depth and redundant-parent rejection;
- typed conflict resolution that validates exact references and fields, clears
  the exact current conflict pair, preserves unrelated heads, and retains both
  immutable provenance references;
- causal ordering independent of input order;
- missing parents, mismatched inverses, wrong target hashes, and invalid append
  causal-depth/parent contexts;
- canonical SHA-256 verification, non-canonical-byte rejection, unknown-field
  rejection, future-version failure, and unknown operation-kind failure;
- operation and referenced-artifact resource bounds;
- observation-chunk references without payload bytes;
- strict typed payload round trips;
- a proptest permutation invariant proving deterministic order for arbitrary
  generated labels;
- an architecture check that prevents storage, packet, wall-clock, and generic
  JSON value dependencies in this crate.

Run the focused suite offline with:

```text
cargo test -p kyberia-operation-log --locked --offline
```

The suite does not claim project-store application or durable collaboration is
complete. It proves only the pure command, query, encoding, merge, and replay
contract delivered in this increment.

## Required follow-up validation

The SQLite persistence increment is covered by the
[operation-store validation record](operation-store.md), including
crash-safe transaction boundaries, reopen/recovery, source hash verification,
and additive migration fixtures. The application increment must apply typed
mutations to the canonical project, validate entity references, create
user-visible conflict-resolution commands, and exercise cross-device actor
authorization. A coordinator increment must add reordered/duplicated delivery,
signature/replay protection, chunk availability, and bounded merge stress
fixtures. None of those concerns belongs in this pure crate.
