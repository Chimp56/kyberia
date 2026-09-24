# Phase 5 spectrum contract validation

Corrected implementation: candidate `9be9e0fd60384d13743a08ee3d9b8672f3c7f716`,
integrated on `main` at `9641431`; tracking update `ac11cd3`.

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

Independent re-review in
[`phase5-spectrum-correction-rereview-20260923.md`](../reviews/phase5-spectrum-correction-rereview-20260923.md)
resolves both prior MAJOR findings. It records one non-blocking diagnostic
MINOR: a legacy v1 event may be rejected as a serialization error before the
decoder can report `UnsupportedSchema`; it is not accepted or misinterpreted.
The original findings are in
[`phase5-spectrum-current-review.md`](../reviews/phase5-spectrum-current-review.md).

Fixtures are synthetic. No SoapySDR/vendor adapter, spectrum hardware,
equivalent-noise-bandwidth normalization, remote-sensor replay, labeled traces,
or Phase 5 runtime acceptance was exercised.

## Integrated bounded whole-bin band-power increment

This increment is integrated from author commit `9122de8` as `eb27cbf` and
independently approved in
[`phase5-band-power-current-review.md`](../reviews/phase5-band-power-current-review.md)
(report commit `8406e06`). The review found no blocker or major issue. It notes
that the integrated tests did not construct an exact half-millidBm tie. This
isolated follow-up adds a direct regression for the documented
tie-away-from-zero behavior.

The integrated API extends the validated sweep contract with a pure
`SpectrumSweep::integrate_band` query. It integrates one in-grid half-open band
only when both endpoints align exactly to whole-bin boundaries. It sums dBm
values after converting them to mW, or integrates dBm/Hz density over the
explicit grid-bin width before summing. It rounds only the final derived
display result to the nearest milli-dBm, with half-way cases away from zero.
The code does not alter sweep or event schemas/hashes, apply calibration
corrections, infer a noise floor, interpolate partial bins, or alter the
signature path's rejection of PSD event thresholds without equivalent
noise-bandwidth normalization.

If any selected bin is below detection, clipped, or not observed, the result
contains no total; it returns the selected range, observed/exact coverage and
per-bin blocking causes in source-grid order. The calculation is bounded by
`ProcessingLimits` and fails on empty, misaligned, out-of-grid, or over-budget
queries. Synthetic regressions cover two -30 dBm bins (-26.990 dBm), 0 dBm/Hz
integrated over one 1 MHz bin (+60 dBm), full-grid edges, invalid ranges,
clipped/missing evidence, unchanged source identity, and a lowered work limit.

This is a narrow computation core only. It does not implement current/average/
minimum/max-hold views, a waterfall, threshold occupancy, channel overlays,
time-domain bursts, analyzer adapters, calibration application, a renderer, or
product/Phase 5 acceptance. `SPE-002` and Phase 5 remain open pending the broader
numerical, rendering, hardware, calibration, and field gates.

The bounded addition passes 19 contract tests, strict Clippy, formatting,
architecture, source inventory (522 locked packages), ledger generation/check,
and whitespace checks. The independent reviewer reran the focused tests and
static checks. At that integration point, no half-millidBm tie fixture was
constructed.

## Integrated final-quantization tie regression

The final milli-dBm quantization regression was integrated from author commit
`b769724` as `b63aab6` and independently approved with no findings in
[`phase5-bandpower-tie-current-review.md`](../reviews/phase5-bandpower-tie-current-review.md)
(report commit `71f9de2`). The test checks exact `+0.5` and `-0.5` inputs and
their immediately adjacent representable `f64` values, verifying ties round
away from zero and neighbors to the nearest integer. It reaches the private
`round_milli_dbm_ties_away_from_zero` seam after logarithmic power conversion;
it does not claim to construct a real sweep whose transcendental output is
exactly halfway. The only production-source change extracts that private
quantizer from the existing mW-to-dBm helper. The public API and intended
calculation semantics are unchanged.

Focused checks on this candidate passed:

- `CARGO_HOME=/private/tmp/kyberia-phase5-bandpower-tie-20260923/.trash/cargo-home cargo test --locked --offline -p kyberia-spectrum-contract` — 1 unit test, 19 integration tests, 0 doctests; 20 passed.
- `CARGO_HOME=/private/tmp/kyberia-phase5-bandpower-tie-20260923/.trash/cargo-home cargo clippy -p kyberia-spectrum-contract --all-targets --locked --offline -- -D warnings`
- `CARGO_HOME=/private/tmp/kyberia-phase5-bandpower-tie-20260923/.trash/cargo-home cargo fmt -p kyberia-spectrum-contract -- --check`
- `python3 tools/architecture.py check`
- `python3 tools/source_inventory.py check` — 522 locked external packages.
- `python3 tools/ledger.py check`
- `git diff --check`

The focused source file SHA-256 is
`c74add0289f21ca90b3ede8e087cb1e7e43e472cda10cdc12208a08d9955b773`.
This is unintegrated candidate evidence pending independent review; it does
not close spectrum adapters/UI, calibration, hardware/field gates, or SPE-002/
Phase 5 acceptance.
