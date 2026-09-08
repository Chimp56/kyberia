# Metric registry review

Status: **APPROVED**

Scope: `crates/spatial-analysis` canonical metric registry, artifact compatibility,
dimensional operation checks, and numerical-model spatial-method binding.

The first independent review found three MAJOR issues:

1. the new registry rejected the previously validated
   `kyberia.signal-metric-definition/1` wire contract;
2. subtracting dBm from dBm incorrectly returned dBm instead of dB; and
3. the numerical model could ignore a metric's declared spatial method and run
   IDW from an independent configuration.

The correction retained a strict legacy decoder that preserves literal bytes,
version labels, media type, and artifact hashes; added the dBm-to-dB rule and
regression; and requires current model configuration to match the registry's
spatial method. `PointValue` leaves nonexact cells explicitly unknown. Additional
validation rejects invalid identities, future/unknown fields, duplicate evidence
or capabilities, and noncanonical selections.

A second agent, who did not author the implementation or correction, reran the
legacy literal-wire/hash probes, arbitrary legacy-label round trips, dimensional
checks, PointValue/IDW mismatch behavior, focused and full workspace tests,
Clippy with warnings denied, formatting, architecture validation, source
inventory, fixture validation, and diff checks. It reported no BLOCKER, MAJOR,
MINOR, or NIT findings.

Validated commands included:

```text
cargo test -p kyberia-spatial-analysis --locked --offline
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
python3 tools/architecture.py
python3 tools/source_inventory.py check
git diff --check
```

The registry currently contains observed Wi-Fi RSSI only. Additional metric
families remain separate planned capabilities and are not represented as
implemented by this review.
