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

## Correction `d29cf40`: independent review in progress

The correction processes component pairs and adds intermediate budgets. It
explicitly rejects overlapping concave or holed intersection/difference inputs;
those required geometry capabilities remain open, not completed by rejection.

Root source inspection identified two checks for the independent reviewer to
challenge before approval. Intersection removes the previous result-subset
checks and substitutes an existential overlap check; overlap alone does not
establish containment. Difference checks that some residual survives for each
original left polygon, which alone cannot establish completeness when sequential
convex cuts produce several residual pieces. These are proof gaps under review,
not yet new reproduced kernel failures. No approval is recorded for this
correction while numerical probes and containment review remain pending.

Root independently ran `cargo test -p kyberia-geometry-adapter --locked
--offline` at frozen `d29cf40`: 22 passed (five segment tests and 17 polygon
tests), zero failed. The supplied regression fixtures pass, including explicit
narrow-overlap rejection. This does not resolve the proof gaps above; the
candidate remains unapproved pending independent adversarial review.

## Independent correction review: REQUEST_CHANGES

Reviewer `/root/operation_log_review_luna` found a MAJOR containment defect in
`d29cf40`, independently reproduced by root. Left triangle vertices are
`(56384,93040), (121920,93040), (56384,158576)`; right vertices are
`(64547.2,106243.2), (130083.2,99689.6), (57993.6,171779.2)`.
The successful intersection includes `(59639.4666381836,155320.53341064454)`.
Its coordinate sum is `214960.00004882814`, outside the left hypotenuse
`x+y=214960`. Both inputs passed the candidate's admission checks.

Retained probe relative to `.worktrees/polygon-partial-loss`:
`.trash/review-probes/geometry-independent`; command
`cargo run --offline --quiet --bin triangle_containment`. This diagnostic
prints the escaped vertex and exits successfully after detecting it; its exit
status is not evidence of correct containment.

Restore intersection containment validation and reject unsupported numerical
excursions. The reviewer also classifies the existential completeness checks
as MAJOR: preserving some overlap or residual does not establish preservation
of all required geometry. Independent 5,000-case rectangle and multi-cut
probes passed, but do not discharge these findings. Candidate integration
remains rejected until corrections and independent verification.

## Evolving correction: difference and tolerance review

Independent reviewer `/root/operation_log_review_luna` still requests changes
on `fix/polygon-containment-completeness`. Its draft uses convex clipping for
intersection and an area-based check around kernel difference output. Total
area and exterior containment alone do not establish every residual region or
hole's correct position and topology. This is an unresolved proof obligation,
not a newly demonstrated misplaced equal-area result.

Root inspected the retained `.trash/review-probes/geometry-public/probe-output.txt`
in that worktree. The recorded public difference of `[0,10]²` and the vertical
strip `[9.9,9.95]×[0,10]` is `UnsupportedCoordinateResolution`; other ordinary
strips `[9.8,9.9]` and `[9.85,9.92]` return `InvalidKernelResult`. The review
message initially named a different error for the first case; the retained
artifact is the evidence used here. These are evolving-draft observations,
not a claim about a frozen commit. Normal centimetre-scale geometry needs a
usable, validated construction or an evidence-backed numerical boundary.

The reviewer also questioned extent-based containment tolerance. Its numeric
example was incorrect: `1000 * 1e-10` is `1e-7` metres, not `0.1` metres.
Moreover, actual code normalization must be accounted for before assigning
physical units to the tolerance. The valid remaining requirement is a justified
precision contract and tests bounding geometric error; the erroneous magnitude
claim is not accepted as a finding. Remove draft coordinate debug output and
run formatting before freezing the next candidate.

## Frozen correction approval

Independent review approves `28ab92e` (including prerequisites `2d3cf82` and
`d29cf40`) for bounded Phase 0 boolean operations. No BLOCKER or MAJOR remains
within the explicitly provisional finite-tolerance contract. Root and reviewer
each ran all 30 geometry tests successfully; the reviewer also passed workspace
tests, workspace Clippy, formatting, architecture and source-inventory checks.

Retained adversarial probes cover translated/narrow intersections, small holes,
near-total-cover residual rejection, mixed-scale disjoint union and single/multi
strip differences. Small boundary excursions remain bounded by the documented
normalized tolerance; this is not an exact-topology guarantee. General concave
or holed overlapping overlays, offsets, import/repair, CRS/3D, WASM runtime
parity and full Gate E remain open.

Integration follow-ups: correct research-ledger wording now that `geo` is a
pinned runtime dependency; describe the verification as a bounded numerical
certificate and its work cap as an estimate, not an invocation-wide CPU bound.
