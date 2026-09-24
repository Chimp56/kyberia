# Phase 4 route-backed geometric candidate generation

This validation record covers a bounded OPTB-001 foundation: deterministic
stations along one caller-supplied, explicitly floor/frame-bound cable
polyline. It does not infer cable routes from closets or drops, sample a whole
floor with a grid or Poisson process, model mounting/height/power/safety, score
radio propagation, select AP models, optimize assignments, or produce a plan.
The route is treated as the complete cable-length evidence for this call; it
is not proof that the building has a routable cable network.

Plan baseline: `plan.md` SHA-256
`1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`.
Relevant coverage anchors are §5.21 automatic AP placement, §6.8 OPT-001
candidate sources and pruning, §9.1 planner inputs, §9.3 hard constraints,
§16.3 geometry tests, §16.11 optimizer validation, Phase 4 deliverables and
exit criteria, and §18.8 OPTB-001. This slice directly exercises only
route-backed geometric/exclusion/cable filtering and deterministic repeatability;
it does not satisfy the broader acceptance scope of those sections.

`kyberia-geometry-adapter` now classifies point location as `Inside`,
`Boundary`, or `Outside`. Polygon holes are outside their containing polygon;
the exterior and hole edges are both `Boundary`. Queries require the exact
floor and frame identities and do not project or transform points. The
candidate generator includes allowed-region boundaries (including hole
edges), prunes hole interiors, and rejects exclusion-region boundaries as
well as their interiors.

The new `kyberia-planner-candidate-adapter` is classified as a composition
crate: it orchestrates the existing geometry adapter with the numerical
evaluator and is not itself an infrastructure boundary adapter. It emits
stations at route distances
`0, spacing, 2*spacing, ...` not exceeding the full polyline length. Station
IDs are zero-based route ordinals and are not renumbered after pruning. At an
exact route vertex, interpolation advances to the following segment while
preserving the shared vertex. Cable length is the sum of every segment, and
the exact maximum is accepted. Validation and work admission happen before
candidate generation; errors return no partial vector.

Hard bounds are 4,096 route points, 65,536 pre-pruning stations, 256 exclusion
regions, 32,768 total region coordinates, and 4,194,304 conservative work
units. Work admission counts region object visits (including empty regions),
up to four linear coordinate passes per point-location query, station
polygon/object visits, station preflight/emission, route traversal, and
region-size preflight. Unsupported spacing/resolution, mismatched floor/frame,
degenerate segments, out-of-range coordinates, cable excess, arithmetic
failures, or any resource-limit excess are typed errors. Geometry itself
retains its existing per-polygon and per-multipolygon validation bounds.

## Validation

The focused checks exercise interior/exterior/boundary and hole point
classification, multipolygon membership, floor/frame mismatch, stable route
stations through a turn, inclusion/exclusion boundary policy, hole pruning,
exact cable-length threshold, invalid route inputs, route and station limits,
full-polyline cable length, and metamorphic restoration of pruned station IDs
when an exclusion is removed. An explicit 256-empty-exclusion fixture confirms
region-object visits count against the aggregate work ceiling.

Validation results:

- `cargo test -p kyberia-geometry-adapter -p kyberia-planner-candidate-adapter --locked --offline` — PASS, 53 tests total (41 geometry, 12 candidate-generator); no ignored tests.
- `cargo clippy -p kyberia-geometry-adapter -p kyberia-planner-candidate-adapter --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 locked external packages.
- `python3 tools/ledger.py check` — PASS after evidence reconciliation.
- `git diff --check` — PASS.

These synthetic geometry tests do not establish performance for real
buildings, cable topology, RF coverage, optimizer correctness, or any Phase 4
exit criterion. OPTB-001 and Phase 4 remain `IN_PROGRESS`.
