# Bounded planar polygon offsets

This validation covers the Phase 0 rounded offset boundary in
`kyberia-geometry-adapter`. The public API accepts only canonical meter
coordinates in one identified floor-local frame. The `geo` and `i_overlay`
types remain private to the adapter.

## Acceptance cases

- An outward offset of a square grows every exterior bound and uses rounded
  joins with an area inside the expected tessellation interval.
- An inward offset shrinks a square, preserves its scope, and returns an
  explicit empty multipolygon when the requested distance is large enough to
  erase it.
- Outward and inward offsets change hole width in the expected direction.
- Inward offsets of a concave shape may produce multiple components; each
  positive-area component is retained.
- Disjoint components, translated coordinates near the supported bound, tiny
  coordinates, and reordered rings/components retain deterministic canonical
  results. A mixed-scale disjoint component is checked independently so a
  kernel alias cannot silently drop it.
- Invalid angular options, excessive distance, excessive estimated work,
  unsupported local resolution, and cancellation return structured errors
  before a result is exposed.

The executable fixtures are in
[`crates/geometry-adapter/tests/offsets.rs`](../../crates/geometry-adapter/tests/offsets.rs).
The public options and error boundary are in
[`crates/geometry-adapter/src/offset.rs`](../../crates/geometry-adapter/src/offset.rs).

## Backend and precision contract

The adapter pins `geo` 0.33.1 and uses its round `Buffer` implementation. The
requested `max_arc_angle` is an upper bound for tessellation and is validated
between 0.025 and pi/2 radians. The adopted kernel clamps round joins to
0.01*pi through 0.25*pi; admission estimates use that effective angle so a
large accepted request cannot undercount generated arc work. `Meters` is
nonnegative, so the typed direction is the only source of the signed kernel
distance.

The kernel converts a locally centered, normalized shape to a finite integer
grid. Offset admission therefore uses the local input span and the requested
distance when checking that four grid steps represent the offset. This keeps a
small offset on a translated floor from being rejected solely because of its
world-coordinate origin. It also rejects a feature that cannot be represented
at the adopted kernel resolution. Source coordinates are restored after the
kernel and are admitted through the same canonical polygon validation; no
snapping, repair, reprojection, or implicit closure is performed.

The adapter performs bounded output checks and buffers each input component as
an independent completeness probe. Every outward component must contribute to
the global result. An inward empty result is accepted only when the component
bounding boxes provide a conservative half-width certificate; uncertain
empty or missing-component outcomes return `UnsupportedCoordinateResolution`.
These checks reduce known mixed-scale aliasing risks but do not make the
floating-point kernel exact. The result carries no backend or precision
receipt; a persistence layer that records an offset must retain its own
adapter/protocol version and input identities.

## Limits and cancellation

`MAX_OFFSET_WORK` is a checked estimate over input ring coordinates and the
effective rounded-join vertex count. It is evaluated before invoking the
kernel and all output coordinate/component limits are checked before canonical
construction. This is an admission bound, not a promise of a fixed CPU or
resident-memory ceiling for the third-party implementation.

`offset_with_cancellation` polls during input accounting, before the kernel,
after the kernel, during component probes, and before returning the canonical
result. The adopted `geo` call is synchronous and cannot be interrupted while
inside the kernel, so cancellation latency is bounded by that noninterruptible
call plus the admitted work. A cancelled call never returns a partial result.

## Remaining scope

The boundary does not implement import/repair, CRS conversion, three-
dimensional offsets, production artifact publication, or renderer behavior.
Simple validated inputs and the explicit finite-precision contract are covered
here; broader Gate E evidence still requires independent import, provenance,
desktop/WASM, and production integration validation.

## Validation commands

```text
cargo test -p kyberia-geometry-adapter --locked --offline
cargo clippy -p kyberia-geometry-adapter --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
python3 tools/architecture.py check
python3 tools/source_inventory.py check
git diff --check
```
