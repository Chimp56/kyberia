# ADR 0013: Spatial signal aggregation uses the Wi-Fi semantics contract

- Status: Accepted bounded numerical boundary; manifest/storage/UI integration remains open
- Date: 2026-09-07

## Context

Plan §§7.9, 7.20, 7.21, 11.7 and 12.1 require every derived spatial signal
value to identify its aggregation method, parameters, inputs and version. The
initial spatial baseline grouped coincident dBm samples with a private
arithmetic mean. That behavior was neither physically appropriate for all
metrics nor constrained by the metric-definition provenance retained in an
analysis request. It could silently diverge from live Wi-Fi signal semantics.

The spatial `Sample` contract intentionally contains position evidence and a
scalar value, but no capture monotonic timestamp. Temporal estimators therefore
cannot be admitted by inventing an order from IDs or coordinates.

## Decision

`kyberia-spatial-analysis` depends inward on the pure
`kyberia-wifi-semantics` crate. `Inputs::metric_definition` is a typed
`MetricDefinitionBinding` containing the versioned immutable metric artifact
reference and its `SignalAggregationSelection`. The spatial model validates
that selection before grouping and uses the Wi-Fi contract's static aggregation
entry point for every coincident location group.

Each location group and each tile retain the exact aggregate result: closed
algorithm version, method parameters, count and observation identities. Static
methods canonicalize identity order. EWMA and robust state-space methods are
rejected with an explicit monotonic-evidence error until a time-aware spatial
sample contract is introduced. Unknown samples remain in the input evidence
plane but never provide group support or a zero value.

The `MetricDefinitionBinding` constructor verifies the bounded canonical bytes,
schema, content hash, length, media type, version identity and exact typed
selection before a binding can enter the numerical model. It is backed by
the complete canonical `MetricDefinition` in ADR-0017 for
current artifacts. The historical `SignalMetricDefinition` type and
`kyberia.signal-metric-definition/1` wire schema remain as a strict
compatibility decoder. Legacy bindings preserve their original bytes and hash
and leave spatial-method selection to the legacy caller. The numerical crate
does not read files; an outer adapter only loads the untrusted bytes and passes
them to this constructor.

## Alternatives

- Keep a private arithmetic mean in spatial analysis: rejected because it hides
  metric semantics and can add dBm values incorrectly for power-oriented
  metrics.
- Duplicate the Wi-Fi aggregation implementations in spatial analysis: rejected
  because two numerical authorities would drift.
- Fabricate timestamps from observation IDs or coordinate order: rejected
  because it changes an unordered spatial input into an unsupported time series.
- Put Wi-Fi aggregation in the UI or application layer: rejected because all
  derived layers must share one pure, testable semantic contract.

## Evidence

The focused spatial tests independently verify median, trimmed mean, linear
power mean, percentile-range scalar/interval output, coincident grouping,
method/version/parameter/sample serialization, permutation determinism, finite
extreme values, unknown/extrapolation behavior, canonical-wire and duplicate/
unknown/future rejection, artifact hash/length/version/media-type mismatches,
method mismatch, byte/depth bounds, malformed configuration, and explicit
rejection of temporal methods. Wi-Fi semantic tests cover the same aggregate
oracles, static temporal rejection, unknown handling and bounded numeric
behavior.

`tools/architecture.py` validates that the dependency is an allowed numerical
to numerical dependency with no outward platform, storage, UI or adapter edge.

## Consequences

Metric-definition construction is more explicit and existing callers must
provide a versioned artifact binding. Tiles now use schema version 2 because a
hidden v1 arithmetic coincidence rule is not semantically compatible with the
typed selection. The spatial crate remains deterministic and side-effect free;
it still owns interpolation policy separately from signal aggregation.

Future time-aware spatial aggregation requires a new input contract carrying a
single validated monotonic epoch, strict ordering, and the associated clock
provenance. It cannot be enabled by changing a UI selection alone.

## Reversibility

The dependency is replaceable at the crate boundary, and new Wi-Fi algorithm
versions are additive closed variants. Existing v1 tile artifacts remain
historical data and are not reinterpreted as v2. A future spatial input schema
can add timestamped samples while retaining the current static path.

## Validation plan

Run the focused Wi-Fi and spatial tests, full locked workspace tests, formatting,
Clippy with warnings denied, architecture checks and source/license inventory.
Review the resulting API for provenance consistency before integrating any
application or storage caller. Keep PAS-001 and full analysis-manifest/runtime
integration open until those surrounding evidence paths are implemented.
