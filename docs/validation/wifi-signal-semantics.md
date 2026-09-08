# Wi-Fi signal semantics validation

Date: 2026-09-07

Environment: Apple Silicon macOS; locked Rust workspace, offline dependency resolution

Scope: plan §§7.9–7.10 numerical algorithms and contracts

Kyberia's pure Wi-Fi numerical crate implements explicitly selected median dBm, trimmed mean dBm, linear-power mean, R-7 percentile interval, EWMA, and a Huber-limited one-dimensional state-space estimate. Results retain closed algorithm version, parameters, sample count, and observation identities. It also implements scaled linear power sum, dBm/mW conversion, and evidence-aware SNR, SIR, and SINR without a manufactured noise floor.

Validation commands and results:

```text
cargo test -p kyberia-wifi-semantics --locked --offline
PASS — 11 unit/property tests.

cargo clippy -p kyberia-wifi-semantics --all-targets -- -D warnings
PASS

python3 tools/architecture.py
PASS — dependency directions and external package boundaries.

cargo test --workspace --locked --offline
PASS — complete Rust workspace regression suite; focused tests and all 8 compile-fail domain doctests were rerun after the final numerical changes.

/Users/vincent/code/kyberia/.tools/venv/bin/python tools/source_inventory.py check
PASS — 97 locked external packages.
```

The tests use independent numerical oracles for logarithmic conversions, two equal-power addition, each static aggregate, one-step EWMA and robust state update, SNR/SIR/SINR, and R-7 interpolation. Properties exercise dBm/mW round trips and power-sum permutation invariance. Negative cases cover unknown evidence, duplicate observation IDs, mixed clock epochs, non-monotonic streaming input, invalid configurations, direct-conversion underflow, invalid milliwatts, closed wire versions, and the 100,000-sample request bound.

This validates the numerical core. Capture/source normalization, persistence of every raw RSSI and sensor relationship, analysis-manifest execution, spatial interpolation selection, UI inspection, and measured-data validation remain separate integration requirements; PAS-001 therefore remains in progress.
