# Phase 5 spectrum contract correction validation

Implementation revision: `9be9e0fd60384d13743a08ee3d9b8672f3c7f716`.

Scope: correct the dBm versus dBm/Hz signature mismatch and prevent sparse
frequency-local evidence from being promoted by otherwise high global grid
support. These checks cover the hardware-independent sweep/signature contract
only.

On Darwin arm64 with `rustc 1.98.1` and `cargo 1.98.1`, the following checks
passed at the implementation revision:

- `cargo fmt -p kyberia-spectrum-contract -- --check`
- `cargo test -p kyberia-spectrum-contract --locked --offline` — 14 integration
  tests passed; no unit or doctests.
- `cargo clippy -p kyberia-spectrum-contract --all-targets --locked --offline -- -D warnings`
- `python3 tools/architecture.py`
- `python3 tools/source_inventory.py check` — 522 locked external packages.

The tests verify that dBm/Hz sweep payloads round-trip unchanged while their
signature assessment is `Unknown/PowerUnitUnsupported`, with no occupied-bin
or active-sweep claims. No conversion is inferred from bin spacing. For
dBm/bin, each candidate frequency bin must be determinate in at least 800,000
parts per million of all event sweeps, independently of conditional persistence
and global grid support. Regression fixtures demonstrate that a 50%-observed
candidate remains unknown despite high global support/activity, while the
exact 80% local-coverage boundary is accepted.

Fixtures are synthetic. No SoapySDR/vendor adapter, spectrum hardware,
equivalent-noise-bandwidth normalization, remote-sensor replay, labeled traces,
or Phase 5 runtime acceptance was exercised. Independent review of this
correction remains required.
