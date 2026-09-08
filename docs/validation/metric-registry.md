# Metric registry Phase 0 validation

Date: 2026-09-08

Scope: plan §11.8, §12.5, FND-010, and ADR-0017. This increment provides one
canonical bounded definition in `kyberia-spatial-analysis`; it does not claim
that capacity, SINR, throughput, or predictive metrics are implemented.

`MetricDefinition` contains typed ID/revision, semantic description, unit,
valid range, evidence/capability requirements, aggregation, spatial method,
selection filters/grouping, uncertainty method, explicit unknown compatibility,
compatibility rules, compliance direction, and separate visualization defaults.
The only builtin is observed Wi-Fi RSSI and it delegates signal aggregation to
the existing versioned `kyberia-wifi-semantics` contract.

Canonical bytes are compact pinned Serde JSON under
`kyberia.metric-definition/1`, with the media type
`application/kyberia-metric-definition+json`, a 16 KiB byte limit, and depth 16
limit. Parsing requires closed typed fields and byte-for-byte canonical
equality. `content_hash`, `ui_help`, `compute_contract`, and verified spatial
bindings all derive their identity from those same bytes. Unknown values remain
unknown by reason under `PropagateReason`; no zero or synthetic fallback is
introduced.

The registry uses a bounded B-tree keyed by `(MetricId, MetricVersion)`, rejects
duplicate versions, and lists deterministically. The dimensional checker
accepts same-unit minimum, dBm-to-dB difference and dimensionless rank, while
rejecting mismatched units, categorical arithmetic, and unsupported
unary/binary forms. Current metric bindings validate the declared spatial
method against the model configuration; a PointValue definition cannot run
IDW. Legacy signal artifacts use a strict decoder that preserves their original
wire bytes and hash.

Focused validation:

```text
cargo test -p kyberia-spatial-analysis --locked --offline
PASS — 31 active tests passed, one explicit benchmark ignored.

cargo clippy -p kyberia-spatial-analysis --all-targets --locked --offline -- -D warnings
PASS

cargo fmt --all -- --check
PASS
```

The complete locked workspace suite, workspace Clippy, architecture check and
source inventory remain integration gates. The registry source uses only the
already pinned Serde, serde_json, SHA-256, domain, and Wi-Fi semantic contracts;
no UI, persistence, capture, Kismet, or Sionna dependency is introduced.
