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
`geo`, with a cap of 4,194,304 units; componentwise intersection and sequential
difference also enforce the cap and output limits after every intermediate
step. Results are counted and revalidated after the kernel. No repair, snapping,
CRS conversion, material attenuation,
three-dimensional operation, or import-format handling is performed here.
Touching component topology and precision below the kernel grid are explicit
unsupported outcomes. Full Gate E evidence still requires broader import,
repair/provenance, desktop/WASM behavior and production integration tests.

## Validation commands

```text
cargo test -p kyberia-geometry-adapter --locked --offline
cargo clippy -p kyberia-geometry-adapter --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
python3 tools/architecture.py
python3 tools/source_inventory.py check
python3 tools/ledger.py check
```
