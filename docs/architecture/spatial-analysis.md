# Numerical spatial analysis, version 2

`crates/spatial-analysis` implements the bounded numerical part of plan.md
§7.9, §7.21, §7.24–7.25, §11.7, §12.1–12.2 and iteration 4. It is an inward
Rust computation crate, depending only on canonical domain units, the pure
`kyberia-wifi-semantics` crate, and Serde/serde_json/SHA-256. The Wi-Fi dependency is a narrow
directional dependency: it supplies signal aggregation and RF arithmetic, but
does not know spatial coordinates, grids, geometry, persistence, or rendering.
The spatial crate does not read files, obtain clocks, filter live collectors,
render colors, or import foreign project objects.

## Inputs and responsibility boundary

`Inputs` names one floor, one meter coordinate frame, a metric definition and
an immutable source artifact. The caller must verify the artifact bytes and
bind that reference to exactly the supplied sample set. The metric definition
must pin transmitter/BSSID selection, sensor/time selection, aggregation and
position-assignment versions. The caller resolves the floor's coordinate frame
through the canonical project graph. This crate checks that every supplied
sample has the same floor/frame identity; it does not own or bypass that graph.

Samples contain observation IDs, signed meter coordinates, explicit known or
unknown scalar dBm values and position covariance evidence. Unknown values are
retained in the numerical tile input manifest but do not provide scalar support.
Duplicate observation IDs are rejected even if their contents match. Different
observations at the same coordinate remain different evidence records.

`Inputs::metric_definition` is a verified `MetricDefinitionBinding`, consisting
of the immutable metric-definition artifact reference and its typed
`SignalAggregationSelection`. Its validating constructor accepts only bounded
canonical bytes with the expected schema, content hash, byte length, media type,
artifact version identity and exact method projection. Current definitions also
bind `Config::method` to their declared spatial method, so a point-value
definition cannot silently run IDW or nearest-neighbor interpolation. The
canonical observed-RSSI builtins are additive and exact: `wifi.rssi/1` is
point-value, `wifi.rssi.nearest/1` is nearest-neighbor, and `wifi.rssi.idw/1` is
inverse-distance weighted. The IDW power remains an explicit `Config` parameter
and is retained in the spatial configuration; it is not hidden in the metric
identity. The original `wifi.rssi/1` canonical bytes and hash are unchanged.
Historical signal-only artifacts remain readable through a strict compatibility
decoder; they preserve their original schema, media type, canonical bytes and
hash and retain the pre-registry caller-supplied spatial method. The canonical
`MetricDefinition` supplies the semantic description, typed unit and valid
range, evidence/capability requirements, aggregation, spatial method,
selection filters/grouping, uncertainty and unknown policy, compatibility,
compliance direction, and explicitly nonsemantic visualization defaults. The
same canonical bytes and SHA-256 are exposed to UI help and compute contracts.
The registry rejects duplicate `(id, version)` entries and lists them in stable
order. The selection is closed and versioned by `kyberia-wifi-semantics`;
malformed, duplicate, unknown or future wire values fail validation. Current
canonical JSON artifacts use media type
`application/kyberia-metric-definition+json`, while the historical
`application/kyberia-signal-metric-definition+json` artifact remains accepted.
Both are bounded to 16 KiB and depth 16 and must byte-for-byte equal their
respective pinned Serde JSON encoding.

The input evidence plane distinguishes measured evidence from synthetic test
evidence. Cell class `Observed` means exact input-coordinate support, within that
plane; it never converts a synthetic fixture into a real RF measurement. The
observed-RSSI selection boundary rejects `SyntheticFixture` sources and
`SyntheticFixture` quality flags before they can become measured samples. It
uses the existing V1 rejection vocabulary (`UnsupportedPayload` for a synthetic
source and `UnusableQuality` for a synthetic quality flag), so this admission
correction does not change the selection-manifest schema.

No statistical independence is inferred from observation count. Position
covariance is retained, but this baseline does not propagate it. Height,
multi-floor coupling, sensor bias, temporal autocorrelation and calibration
corrections belong to subsequent explicitly configured methods.

## Algorithms and support

Unknown is the default outside configured support. There is no default RF
distance or default IDW power: callers supply explicit parameters. Geometric
support consists of locations within the configured inclusive Euclidean radius,
with a configurable minimum number of distinct locations. For minimum 1 it is
the union of radius disks. This is an explicit model policy, **not a convex hull,
wall-aware boundary, calibrated confidence contour, or proof of accuracy**.
In particular a single point can produce a constant estimate inside a selected
radius; the UI must display this assumption and the sample count. A radius can
bridge an unsampled hole if the user chooses it too large. TIN/hull support and
barrier-aware support remain separate planned methods.

Coincident known readings form one location group. Its value is produced by the
typed aggregation selected by the retained metric definition: median dBm,
trimmed mean dBm, linear-power mean converted back to dBm, or percentile range
with a median scalar estimate. Every group retains the complete
`SignalAggregate`, including method parameters, semantic algorithm version,
sample count and canonical observation IDs. Group positions sort by x then y.
One group contributes once to spatial weighting regardless of its repeat count;
both distinct-location and known-observation support counts are reported.

The spatial sample shape carries no capture monotonic timestamp. Consequently,
EWMA and robust state-space aggregation are rejected with
`TemporalAggregationRequiresMonotonicEvidence`; observation IDs and coordinate
ordering are never used as a fabricated time sequence. A future time-aware
spatial input must carry a validated single clock epoch and strictly increasing
monotonic timestamps before those methods can be admitted.

At an exact coordinate, the selected group aggregate is returned as `Observed`, even if the
minimum count for interpolation is not met. With `PointValue`, every nonexact
cell remains explicitly unknown; it never falls through to interpolation.
Otherwise:

1. Neighbors are bounded by the configured radius and maximum neighbor count.
2. Equal distances break ties by stable location-group order (x then y).
3. Nearest returns the first neighbor; supporting counts still describe all
   known evidence inside the support radius.
4. IDW uses `w_i = (d_min / d_i)^p`, then `sum(w_i * v_i) / sum(w_i)`, where
   `v_i` is the selected location-group aggregate in dBm. This is algebraically
   the usual inverse distance weight, rescaled to keep the closest weight 1 and
   others at most 1.

The same method interface returns numeric values, classes, support, distance
and contributors. A bounded max-heap retains the nearest locations during the
linear input scan. No general geometry or triangulation implementation is
introduced. The power range `(0, 64]` and resource limits are numerical API
bounds, not recommended scientific settings.

Extrapolation is disabled unless the caller explicitly selects a second finite
radius at least as large as the support radius. A query lacking normal support
may use neighbors inside that larger radius, subject to the same minimum count,
and is labeled `Extrapolated`. Beyond that radius it remains unknown. Enabling
extrapolation does not change values already supported by the normal radius.
Unsupported/extrapolated cells must not be treated as measured compliance passes.

Every nonzero-weight contributor records its location-group index and normalized
probability weight, enabling numeric evidence inspection. Very small weights may
underflow to zero and are omitted; this does not turn an interpolated point into
an observed one. The scaled compensated mean avoids overflowing on finite extreme
values and stays within the contributor extrema. Distance overflow is treated as
larger than all finite radii. If every known location distance is unrepresentable,
the value remains outside-support unknown and distance is `InvalidGeometry`
unknown; an empty known dataset instead reports `NotMeasured` unknown.

## Uncertainty and validation claims

All output `uncertainty_db` entries are explicitly `NotMeasured` unknown. A count,
distance, or dBm spread is not substituted for calibrated model uncertainty.
This preserves the uncertainty output channel without claiming a confidence
interval or pretending the spatial policy measures accuracy. There are no
predicted, hybrid, GP or calibrated uncertainty outputs in this increment.

Original fixtures in `tests/baselines.rs` cover a symmetric affine field,
two-point rational IDW weights, a radial center holdout, separated clusters with
an unknown gap, coincident repeats with each static signal aggregation, and sparse
one/two-point cases. For the radial
field `-40 - 10r`, four samples at radius 1 all read -50 dBm. IDW predicts -50 dBm
at the held-out center whose truth is -40 dBm: **10 dB error is expected and
asserted**, demonstrating why smooth interpolation and low neighbor spread do
not establish accuracy. The affine fixture is exact only at its symmetric test
point; IDW is not claimed to reproduce arbitrary affine fields.

The fixtures are independently authored numerical examples, not field data,
material presets, copied competitor fixtures or upstream executions. The existing
clean-room `fixtures/wifiheatmap-oracle/tin-v1.json` was inspected as a boundary
reference. Its triangle/hull behavior is not claimed implemented by this IDW
method. No third-party source or dataset was copied. The pure
`kyberia-wifi-semantics` dependency is intentionally added to the production path
so aggregation cannot drift between live/RF arithmetic and spatial layers.
Existing Serde, serde_json and proptest pins are reused; no statistics or geometry
dependency was added.

## Tiles, exports and bounded execution

`kyberia.numeric-rssi-tile/2` is a Serialize-only numerical export shape. It
contains the complete sorted inputs, immutable source reference, configuration,
algorithm/aggregation versions, location dictionary, grid and row-major cells.
The top-level `signal_aggregation` field and each location group's
`signal_aggregate` field make the selected method, exact parameters, version and
sample identity audit-visible in the tile. The top-level field is private in the
Rust API and is populated only from the verified binding, so it cannot diverge
through a public struct literal. The previous v1 artifact encoded a
hidden arithmetic dBm coincidence rule and is not semantically interchangeable;
old artifacts remain historical evidence and are not silently reinterpreted.
Values/masks/uncertainty/support are independent of palette/presentation. There
is deliberately no unchecked deserializer or production file import path.
Future persisted imports must validate resource limits, manifests, references,
cell invariants and schema versions before trust. The owning application/store
must content-hash and persist these artifacts; this crate does not authenticate
the caller's claimed source hash or generate the global analysis manifest.

Grid coordinates are floor-local meters, +x across columns, +y across rows.
`origin` is the grid corner; cell centers are
`origin + (integer_offset + cell_index + 0.5) * resolution`. Shared origin,
resolution and integer offsets give identical centers in independently computed
tiles. Pixel coordinate inversion remains a rendering transform. Every adjacent
axis center must be representably increasing; collapsed or overflowing grids
fail with a structured numerical error instead of duplicating positions.

Limits per call are 100,000 input observations, 100,000 cells, 64 contributing
locations per cell and 100,000,000 distance evaluations. These bound allocation
and work per tile, not project size. Larger projects must split jobs into tiles.
Outputs retain a shared dictionary within each tile, avoiding repetition of all
coincident observation IDs per cell. Large neighbor limits can still produce
tens of megabytes of contribution data; a future streaming artifact writer is
needed before claiming arbitrarily large project performance.

Cancellation is caller supplied, checked before/after tile work, between cells,
and at most every 64 location-distance evaluations. It returns `Cancelled` and
never a partially filled tile labeled complete. Construction and grid validation
are bounded synchronous setup work; they do not have resumable progress. The
clockless numerical core does not enforce wall-clock deadlines itself.

## Executed validation

On macOS 26.6.2 build 25G83, arm64, rustc 1.98.1 (2026-09-01):

```sh
cargo fmt --all -- --check
cargo test -p kyberia-domain -p kyberia-spatial-analysis -p kyberia-wifi-semantics --locked --offline
cargo clippy -p kyberia-spatial-analysis -p kyberia-wifi-semantics --all-targets --locked --offline -- -D warnings
cargo test -p kyberia-spatial-analysis --release --locked --offline benchmark_tiles -- --ignored --nocapture
```

Results: 23 spatial tests passed, one explicit benchmark excluded from normal
tests; 13 Wi-Fi semantic tests, 32 domain tests and 7 domain compile-fail
documentation tests passed; Clippy passed with warnings denied. Properties exercise convex bounds,
permuted-input determinism and integer translation invariance. Tests also cover
schema output inspection, tile seam equality, cancellation, duplicates, frame and
floor mismatch, unsupported inputs, extreme finite/subnormal arithmetic, and
collapsed interior grid centers.

Release timings from one local run on 2026-09-07 (not a cross-platform or stable
CI performance threshold):

| Cells | Input locations | Radius / resolution m | Build ms | Tile ms | Known / unknown cells |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 100 | 5 / 1 | excluded | 20.489 | 7,220 / 2,780 |
| 100,000 | 100 | 5 / 1 | excluded | 54.236 | 7,220 / 92,780 |
| 100,000 | 100 | 15 / 0.1 | excluded | 60.396 | 100,000 / 0 |
| 100 | 10,000 | 5 / 1 | 2.602 | 4.010 | not recorded by timing loop |
| 100 | 100,000 | 5 / 1 | 23.304 | 78.802 | not recorded by timing loop |

All use IDW power 2 and at most 8 neighbors. The first three input sets are a
10×10 grid spaced 10 m with values `-40 - id/10` dBm. The last two are 1000-column
unit grids with values `-40 - (id modulo 60)` dBm. Exact inputs and timing loops
are in the ignored benchmark test. Timings include complete output construction
and cloning its input manifest; they exclude serialization, rendering and disk.

## Remaining requirements

This increment supplies backend portions of ANA-001/002/004 and the first
numeric dBm baseline. It does not complete iteration 4 or the product heatmap
feature: evidence-drawer UI, global analysis hashing/cache, validated tile import,
full metric registry, wall/TIN/hull/RBF/kriging/GP methods, blocked spatial
cross-validation, calibrated intervals, floor transitions, GPU parity, numerical
GeoTIFF/Parquet export, report methodology generation, and user workflow tests
remain open. No external dependency blocks those independent requirements.
