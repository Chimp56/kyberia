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
1e-200 square. All four polygon tests and four segment tests pass. This is an
input-validation increment; polygon operations, import/repair, provenance and
complete runtime Gate E acceptance remain open. Independent review is required.
