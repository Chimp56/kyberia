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

## Second reproduction: near-collinear self intersection

The triangle `(0,0), (1,1), (2,2.000000000001), (0,0)` passes the kernel's
polygon validation and has positive area `5.000444502911705e-13`. Intersecting
it with itself returns zero components and zero area. Root reproduced this
with `geo` 0.33.1; the nonempty-result assertion failed.

The draft's new guard compares distinct input x/y values against a grid step.
Those gaps are approximately 1 in this example, so that guard does not detect
the tiny altitude. Precision admission must address edge/feature geometry and
generated intersection slivers, not just coordinate-axis separation. A special
case for identical operands cannot establish correctness of near-identical
overlaps.

The retained harness now has two binaries: use
`cargo run --offline --quiet --bin polygon-precision-probe` for the original
disjoint union, or `cargo run --offline --quiet --bin near_collinear` for this
self-intersection reproduction. Both are deliberately failing review probes.

## Frozen `2d3cf82`: partial intersection loss

Root reran all 18 candidate tests successfully, then reproduced a MAJOR failure
through the public `ValidatedMultiPolygon::intersection` API. Left contains
triangle `(0,0), (10,0), (0,10)` plus square `[30,40] × [30,40]`. Right contains
triangle `(5,5-1e-10), (20,1), (1,20)` plus the identical square. Rings are closed.
The triangles have a small positive-area overlap: the first vertex of the
second triangle is strictly inside the first and its other vertices lie outside.

The expected result has two components. The adapter returns `Ok` with only the
square. Its whole-result empty check does not detect a lost component when
another component survives; subset containment alone does not prove intersection
completeness. Difference validation similarly needs to establish exclusion and
completeness, rather than only containment in the left operand.

Retained public-API probe:
`.trash/review-probes/partial-intersection-22gnw2r9`, command
`cargo run --offline --quiet`; output `result Ok(1)` followed by the failing
two-component assertion. The frozen candidate remains unintegrated pending
correction and independent review.
