# Spatial signal aggregation validation

Date: 2026-09-07

Scope: plan §§7.9, 7.20–7.21, 11.7, 12.1–12.2 and backlog PAS-001/ANA-001/002/004

The spatial analysis request now retains a typed `SignalAggregationSelection`
inside a privately constructed `MetricDefinitionBinding`, alongside the
immutable metric-definition artifact reference. The binding constructor
verifies bounded canonical bytes, schema, content hash/length, media type,
version identity and exact method projection before the model accepts it.
For current artifacts, the model rejects a Config spatial method that differs
from the metric declaration; PointValue therefore leaves nonexact cells
unknown. The binding is backed by the complete canonical `MetricDefinition`
registry record; `SignalMetricDefinition` remains a strict compatibility type
for the original wire contract. Existing signal artifacts retain their
historical schema, media type, bytes and content hash when opened; new complete
definitions use the additive metric registry contract.
Canonical JSON is limited to 16 KiB and depth 16 and must match the byte-for-byte
Serde encoding; duplicate, unknown, reordered and future fields are rejected.
The registry media type is
`application/kyberia-metric-definition+json` (historical signal artifacts use
`application/kyberia-signal-metric-definition+json`); UI help and compute contracts
expose the same definition SHA-256.
Coincident samples are passed to the pure
`kyberia-wifi-semantics` contract. The old hidden arithmetic dBm coincidence
rule is no longer used; the tile schema is `kyberia.numeric-rssi-tile/2`. Tile
construction keeps its top-level aggregation field private and derives it from
the verified binding; the serialization test compares both provenance copies.

The spatial sample shape has no monotonic capture timestamp. EWMA and robust
state-space selections therefore fail with
`TemporalAggregationRequiresMonotonicEvidence`. The implementation never
derives a time order from observation IDs or coordinate order. Static methods
preserve canonical observation identities and all unknown inputs remain in the
input evidence plane without contributing support.

Focused validation:

```text
cargo test -p kyberia-spatial-analysis -p kyberia-wifi-semantics --locked --offline
PASS — 23 spatial tests and 13 Wi-Fi semantic tests.

cargo clippy -p kyberia-spatial-analysis -p kyberia-wifi-semantics --all-targets --locked --offline -- -D warnings
PASS
```

Repository gates also pass in this worktree:

```text
cargo test --workspace --locked --offline --quiet                         PASS
cargo clippy --workspace --all-targets --locked --offline -- -D warnings   PASS
cargo fmt --all -- --check                                                 PASS
python3 tools/architecture.py                                                PASS
python3 tools/source_inventory.py check                                      PASS — 97 packages
python3 tools/validation/fixtures.py check                                   PASS
cargo test -p kyberia-spatial-analysis --release --locked --offline benchmark_tiles -- --ignored --nocapture  PASS
```

The repository source-qualified ledger check reports 27 expected stale
implementation evidence digests for the changed Rust files. `ledger.json` and
the generated traceability file remain integration-owned and must be
regenerated after review; no stale digest is treated as validation evidence.

Tests cover independent median, trimmed mean, linear-power mean and percentile
oracles for coincident groups; exact method/parameter/version/sample identity
serialization; static permutation determinism; extreme finite dBm values;
duplicate, canonical-wire, hash, version, media-type and malformed configuration
rejection; temporal-method rejection;
unknown samples; explicit extrapolation; spatial support counts; cancellation;
and tile resource limits. The Wi-Fi crate separately tests its timestamped
ordered methods and the static no-timestamp contract.

An application or storage adapter still loads untrusted artifact bytes, but the
pure binding constructor authenticates their declared metric-definition content
before constructing `Inputs`. Full
analysis-manifest execution, artifact loading, UI evidence inspection,
interpolation validation and field/lab calibration remain integration work.
