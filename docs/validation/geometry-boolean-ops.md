# Bounded planar boolean validation

This validation covers the Phase 0 boolean increment in
`kyberia-geometry-adapter`. It uses canonical `CoordinateMeters`, `FloorId`
and `FrameId` values while keeping `geo` types private to the adapter.

## Acceptance cases

- Overlapping rectangles produce measured union, intersection and difference
  areas of 7, 1 and 3 square metres.
- Subtracting a contained rectangle produces one polygon with one hole and
  area 64 square metres.
- Disjoint union preserves two components; disjoint intersection and equal
  difference return explicit empty multipolygons.
- Boundary-contact union merges adjacent rectangles and boundary-contact
  intersection is empty.
- Operand order and input ring direction produce identical canonical union and
  intersection results.
- Floor and frame mismatches fail before kernel execution.
- Component, coordinate and pre-kernel work limits return explicit resource
  errors.
- A tiny valid polygon remains present when unioned with a disjoint large
  polygon. A narrow overlapping feature at the pinned kernel resolution is
  rejected as `UnsupportedCoordinateResolution`, preventing silent component
  loss.
- A valid near-collinear sliver is rejected before overlay when its area is
  below the pinned kernel's conservative precision bound; it cannot become a
  false empty result.
- A multipolygon with two positive-area component pairs preserves both
  intersection components and the residual difference component. A narrow
  partial overlap is rejected explicitly instead of returning the unrelated
  surviving component.
- A translated 50-metre intersection near the coordinate bound retains its
  measured 625 square metres when area is computed from a local origin.
- A verifier fixture with a large residual and a missing small residual is
  rejected even when a relative whole-result tolerance would otherwise hide
  the omission.
- Overlapping non-convex or holed inputs return `UnsupportedTopology`; this is
  the current bounded overlay contract rather than an implicit repair.
- Tiny `1e-200` coordinates are normalized for overlay without returning a
  false empty result.

The focused test source is
[`crates/geometry-adapter/tests/polygons.rs`](../../crates/geometry-adapter/tests/polygons.rs).
The implementation is
[`crates/geometry-adapter/src/polygon.rs`](../../crates/geometry-adapter/src/polygon.rs).

## Limits and remaining scope

The adapter caps a multipolygon at 256 components and 16,384 total ring
coordinates. Each boolean estimates coordinate-pair work before calling
`geo`, with a cap of 4,194,304 units. The independent convex intersection
clipper applies the same work cap to every edge pass and bounds each
intermediate vertex vector. Sequential difference applies the cap to every
subtraction step and bounds each retained intermediate component. Results are
counted and revalidated after the kernel. No repair, snapping, CRS conversion,
material attenuation, three-dimensional operation, or import-format handling
is performed here.
Touching component topology and precision below the kernel grid are explicit
unsupported outcomes. Full Gate E evidence still requires broader import,
repair/provenance, desktop/WASM behavior and production integration tests.

## Backend and precision contract

The adapter pins `geo` 0.33.1 for the portable overlay boundary. For the
supported overlapping contract (simple, convex, hole-free components),
intersection coordinates come from a bounded Sutherland-Hodgman clipper. The
pinned `geo` intersection is retained as an independent topological sanity
check: it must agree on empty versus positive-area status and return at most
one component for a convex pair. Its floating coordinates and area are not
copied into the canonical result because the reviewed kernel can emit a vertex
outside an input edge or lose a narrow component after integer conversion.

Intersection candidates are then checked for containment in both operands and
for per-pair area conservation. Difference retains the pinned sequential
`geo` result, checks that no result overlaps the excluded set, and verifies
per-left-component area conservation against the independent convex
intersection areas. Containment allows `max(1e-8,
32 * f64::EPSILON * container_extent)` in normalized coordinate units, which
admits ordinary floating evaluation error without accepting scale-sized
outliers. Area comparisons use local-origin compensated summation and a fixed
absolute `1e-7` normalized-coordinate area bound (before restoring source
units). Exceeding that bound returns
`UnsupportedCoordinateResolution`; it is a fail-closed numerical contract,
not an arbitrary-precision or exact-overlay claim.

The geometry result does not carry a request or backend provenance record.
Callers that persist an operation result must record the adapter version,
`geo` version, the bounded clipper contract, and the input hashes alongside
their own operation provenance. This adapter does not imply that a geometry
request has passed the broader Gate E provenance or export requirements.

## Validation commands

```text
cargo test -p kyberia-geometry-adapter --locked --offline
cargo clippy -p kyberia-geometry-adapter --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
python3 tools/architecture.py
python3 tools/source_inventory.py check
python3 tools/ledger.py check
```
