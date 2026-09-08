# ADR 0017: Canonical bounded metric registry

- Status: Accepted bounded Phase 0 contract; broader analysis integration remains open
- Date: 2026-09-08

## Context

Plan §11.8 and FND-010 require one source for metric meaning, UI help, units,
and compute contracts. §12.5 requires dimensional checks for layer algebra.
The existing spatial signal increment already had a versioned
`SignalMetricDefinition`, but it carried only an aggregation selection. A
second general registry would allow the two definitions to drift.

## Decision

Extend the existing inward `kyberia-spatial-analysis` boundary with one
canonical `MetricDefinition`. Retain `SignalMetricDefinition` as a strict
legacy wire type and decoder for existing artifacts; it is projected into the
canonical model only after its original bytes and hash are verified. Every
current definition has a typed ID and numeric revision,
semantic description, physical or categorical unit, finite valid range,
evidence/capability requirements, typed aggregation, spatial method,
selection filters/grouping, uncertainty method, explicit unknown policy,
compatibility, compliance direction, and separate nonsemantic visualization
defaults.

The wire schema is `kyberia.metric-definition/1` with media type
`application/kyberia-metric-definition+json`. Its closed typed document is
serialized with the pinned Serde JSON encoding, limited to 16 KiB and depth 16,
and content-addressed with SHA-256. A binding authenticates bytes, length,
hash, media type, version identity, and the exact typed aggregation before a
spatial computation can use it. `MetricRegistry` rejects duplicate ID/revision
pairs and returns deterministic ID/revision order. The Phase 0 builtin is
observed Wi-Fi RSSI using the already implemented versioned signal aggregation;
capacity, SINR, throughput, and predictive metrics are not registered until
their semantics are implemented.

UI help and compute contracts both carry the definition's canonical hash.
Unknown evidence is preserved by reason through an explicit
`PropagateReason` policy. The bounded dimensional checker supports same-unit
minimum, dBm-to-dB difference and dimensionless rank, and rejects categorical
arithmetic or mismatched units. It does not introduce a general expression
language. A current definition's declared spatial method is checked against
the model config; legacy bindings retain an explicitly absent declaration for
backward-compatible callers.

## Alternatives

- Keep the signal-only definition and add a separate registry: rejected because
  aggregation and general metric semantics could diverge.
- Put metric metadata in UI/application JSON: rejected because computation and
  exports would have no authenticated canonical source.
- Register unimplemented throughput/capacity metrics: rejected because a label
  would overclaim semantics without an evidence and algorithm contract.
- Build a general DSL in Phase 0: rejected until bounded operator and unit
  semantics have independent implementations and fixtures.

## Consequences

Existing spatial callers keep their legacy constructor and serialization API;
new callers use the complete canonical definition and current media type.
Reading a legacy artifact does not rewrite its canonical bytes, media type or
hash. Changing visualization defaults changes the content identity of a
definition, but those fields are visibly separate and are never consumed by
numerical aggregation. New metric revisions are additive; an unknown revision fails
closed at the boundary. The registry does not persist definitions or execute
analysis; project storage and analysis-manifest adapters remain outer-layer
work.

## Validation

`crates/spatial-analysis/tests/metric_registry.rs` covers complete builtin
fields, canonical round-trip, future/unknown-field rejection, invalid ranges,
bounded bytes, duplicate registry versions, deterministic lookup/listing,
same-hash UI/compute bindings, legacy schema/media byte and hash preservation,
dBm-to-dB difference, categorical arithmetic, rank dimensions and the
PointValue/model-method contract. Existing spatial tests continue to
independently verify the signal aggregation and tile provenance contract.
