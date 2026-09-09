# Bounded planar polygon validation

This candidate adds an immutable validated polygon boundary to the geo adapter.
It retains canonical meter coordinates, floor/frame IDs, exterior and interior
rings. Construction requires explicit ring closure, no consecutive duplicate
vertices, at most 4096 total coordinates and 128 holes. Bounds apply before
kernel allocation. Library objects remain private; callers cannot mutate the
validated rings through the public API.

Validation normalizes small coordinates before invoking geo 0.33.1 and applies
the segment candidate's conservative mixed-scale resolution bound. Source
coordinates are preserved exactly; no repair, implicit closure, reprojection,
winding rewrite or snapping occurs. Errors distinguish resource/range/resolution,
ring structure, invalid topology and unsupported touching rings.

The pinned geo source at `src/algorithm/validation/polygon.rs` explicitly says
it does not check whether touching rings disconnect a polygon interior. This
candidate therefore refuses any intersection between distinct ring boundaries
after kernel validation, reporting TouchingRingsUnsupported rather than calling
a potentially valid tangent configuration invalid or admitting an unchecked
one. Completing connected-interior validation remains actionable work; it is
not an external blocker or removal of the full geometry requirement.

Tests exercise a valid hole with preserved scope/coordinates, unclosed rings,
a bow-tie, an exterior hole, tangential contact, hole-count limits and a tiny
1e-200 square. The focused polygon and segment suites pass. This is an
input-validation increment; polygon operations, import/repair, provenance and
complete runtime Gate E acceptance remain open. Independent review is required.

## Boolean operations

`ValidatedPolygon` and `ValidatedMultiPolygon` expose typed union,
intersection and difference operations. Results retain the floor/frame scope,
holes, disjoint components and explicit empty results. Output rings and
components are canonically rotated, oriented and ordered before they are
admitted back through the same validated boundary, so operand and vertex
permutations have deterministic results. The public API never exposes `geo`
types.

The operation limits are 256 components, 16,384 total output/input ring
coordinates and a pre-kernel work estimate of 4,194,304 coordinate-pair units.
Component intersections are checked before admission; overlapping or touching
components are rejected rather than dropped. When all components are
provably disjoint, union/intersection/difference use exact component-set
semantics and avoid the overlay kernel. This preserves valid narrow components
that the kernel's integer conversion could otherwise alias away.

For workloads requiring overlay, the adapter models the pinned kernel's
float-to-integer grid and requires two grid steps between distinct normalized
coordinate values. A narrower feature returns
`UnsupportedCoordinateResolution` before kernel execution. The same guard
rejects non-collinear consecutive edges whose cross-product area is below that
bound, covering near-collinear slivers that have adequate axis gaps but can be
lost by the kernel's integer overlay. Kernel outputs are bounded, finite,
explicitly closed and revalidated; malformed, degenerate or out-of-range
output returns `InvalidKernelResult`. These are conservative precision limits,
not snapping or a claim of arbitrary-precision booleans.
