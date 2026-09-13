# ADR 0030: Bounded barrier-aware IDW path cost

Status: Proposed bounded numerical increment; independent review required

## Context

The original spatial-analysis model uses an explicit Euclidean support radius
and inverse-distance weighting. A Euclidean neighbor can be physically across
a wall or other interior boundary, while a farther sample in the same room or
corridor may be the better evidence source. Plan §7.23 requires wall/path cost
to affect spatial interpolation without importing foreign geometry objects or
claiming calibrated uncertainty.

## Decision

Add an inward-owned planar barrier contract to `kyberia-spatial-analysis`:

- `BarrierSegment` contains a stable nonzero `BarrierId`, finite `Point2`
  endpoints and `BarrierMaterial`.
- Material semantics are explicit: nonnegative `Meters` traversal cost,
  nonnegative `Db` attenuation loss, and `Passable`/`Impassable` policy.
- `BarrierSet::new` validates bounds, rejects degenerate segments and duplicate
  IDs, applies a barrier-count limit, and sorts by ID for deterministic paths,
  contributor selection and canonical bytes.
- `Model::new_with_barriers` is additive. `Model::new` preserves the old
  Euclidean method and its existing metric/configuration callers.
- A direct query-to-sample segment is tested against every barrier. A passable
  path gets total cost `geometric_distance + traversal_cost_sum`; attenuation
  remains a separate linear-power factor. IDW chooses bounded neighbors using
  the combined effective path score and records aggregate weights. An
  impassable crossing is excluded from support and remains unknown.
- Exact coordinate lookup precedes barrier policy and remains observed.
- `path_to_group` and `path_assessments` provide path/barrier diagnostics. Cells
  retain the minimum effective path cost through their existing
  `nearest_distance` field and retain contributor weights without changing the
  cross-crate `Cell` struct shape.
- Barrier-aware tiles use the distinct algorithm identity
  `kyberia-spatial/barrier-idw/1` and serialize the canonical barrier set only
  when nonempty. Legacy barrier-free tile bytes remain unchanged.

The implementation uses a scale-free orientation sign and closed finite
segment intersection. It counts each barrier at most once per direct path and
fails closed for invalid material/segment inputs. It intentionally does not
provide polygon shortest paths, floor transitions, geometry repair, or a
calibrated uncertainty interval.

## Alternatives considered

### Euclidean IDW with a wall flag

Rejected. It cannot change neighbor influence or support semantics and would
fail the one-wall, two-wall, and material-cost fixtures.

### Raster crossing counts

Rejected for the numerical core. Rasterization introduces resolution-dependent
crossings and would make translated/permuted vector geometry less stable.

### Importing `geo` or application geometry types

Rejected for this increment. The geometry adapter remains an outer contract;
the numerical core owns only a finite segment representation and does not
leak foreign objects inward.

### General polygon visibility/shortest-path graph

Deferred. It needs validated topology, openings, obstacle boundaries and a
larger resource contract. Direct barrier crossings provide a bounded,
inspectable path-cost increment for wall/corridor evidence now.

### Inferring uncertainty from neighbor spread

Rejected. Neighbor spread and barrier count are diagnostics, not calibrated
error bounds. `uncertainty_db` remains explicitly unknown.

## Consequences

The spatial engine can preserve support gaps across impassable barriers, rank a
farther open path above a closer high-loss path, and explain effective path
cost/material IDs. Stable sorting and fixed resource/cancellation checks make
the result permutation deterministic. Existing users and stored Euclidean
artifacts remain compatible because the old constructor and empty-barrier wire
shape are retained.

Barrier-aware tiles are not admitted by the current renderer/stored-analysis
adapters, which still require the legacy Euclidean algorithm identity. This is
intentional scope: those adapters need a follow-up contract that carries and
replays barriers before barrier tiles can be published through them.

## Validation

Focused tests cover one-wall ranking/value, two-wall path cost, equal-distance
material loss, impassable support rejection, explicit path-cost extrapolation,
measurement-gap behavior inherited from the baseline, exact points,
translation and input/barrier permutation determinism, malformed barriers,
canonical barrier serialization, cancellation and barrier-work exhaustion.
The independent oracle is a hand-computed direct path-cost/linear-attenuation
formula; no competitor code or geometry implementation is used. Run the
spatial-analysis tests and Clippy with warnings denied before review, then
run the workspace architecture and ledger-independent checks at integration.
