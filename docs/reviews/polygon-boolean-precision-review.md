# Polygon boolean draft precision review

Disposition: REQUEST_CHANGES. Reviewer: `/root`, independent of the polygon
boolean author. This finding concerns the unintegrated polygon-boolean draft
and does not apply to the integrated segment-intersection operation.

## MAJOR: mixed-scale union silently loses a component

A standalone offline probe against pinned `geo` 0.33.1 unions two strictly
disjoint squares: `[0, 1e-8] × [0, 1e-8]` and `[1, 100] × [1, 100]`.
The first has positive area approximately `1e-16`; the expected union has two
components. The kernel returns one component with area `9801`, losing the small
square. The draft's shared normalization scale is 1 for these inputs, so its
direct kernel call does not prevent this loss.

Retained independent harness:
`.trash/review-probes/polygon-precision-gli_wp6v`.
Command: `cargo run --offline --quiet` in that harness. Its assertion requiring
two components fails with actual count one. This is rejected numerical evidence,
not a passing test or a production fixture.

Preserve disjoint input components where topology permits exact composition.
For overlapping operations, establish an explicit supported precision boundary
or validate the result strongly enough to detect lost features. Do not silently
equate a quantized-away component with a mathematically empty result. Add
mixed-scale union/difference and narrow-overlap regressions before integration;
correcting only this one input is insufficient.

The author has received the reproduction. Final independent review remains
required after the correction is frozen.
