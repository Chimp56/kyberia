# Planar segment adapter

The first production geometry boundary uses the geo 0.33.1 decision from
ADR 0010 for bounded segment intersections. Inputs and outputs use canonical
meter coordinates, floor IDs and frame IDs; geo types remain private.
Cross-floor/frame inputs, degenerate segments and coordinates beyond plus or
minus 1e9 meters fail explicitly. That bound controls numerical range; it is
not a projection, model accuracy, or snapping tolerance. Inputs are ordered
before the kernel call for deterministic endpoint and operand permutations.

Results distinguish disjoint, a single point and collinear overlap. These are
XY topology only, not wall penetration counts, material loss or attenuation.
Polygon operations, import/repair, material joins, provenance artifacts and
production WASM execution remain subsequent adapter work. This increment does
not close Gate E or the complete geometry requirement.

Initial acceptance: analytic crossing, endpoint touch, overlap and parallel
separation; 99 symmetric crossings under operand/direction permutations; floor,
frame, degeneracy and coordinate-range rejection. Focused tests and all-target
Clippy pass. Dependency inventory and architecture checks pass. Independent
review is required before integration.

Review correction GEO-001: raw coordinates around 1e-200 caused the kernel
orientation products to underflow and report overlap for a crossing. Small
coordinate sets are now scaled up by direct division by their largest absolute
coordinate before evaluation (never an overflowing reciprocal). Restored proper
crossings that collapse onto an endpoint are rejected as unsupported resolution.
Mixed-scale nonzero component separations below 1e-100 after normalization
are also rejected explicitly. This conservative numerical operating boundary
keeps ordinary second/third-order scale products above subnormal magnitudes;
it is not a physical minimum distance, snapping tolerance, or proof of exact
arbitrary-precision topology. Four focused tests pass, including the reported
1e-150, 1e-200 and 1e-300 cases, minimum-subnormal rejection and mixed-scale
rejection. Independent review of this correction is still required.
