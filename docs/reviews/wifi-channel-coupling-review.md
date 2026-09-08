# Wi-Fi channel coupling independent review

Status: **APPROVED after correction**

Reviewer: `/root/channel_coupling_review_luna`
Correction verification: `/root`

Scope: `crates/wifi-semantics` channel geometry, puncturing admission,
spectral coupling, effective interference, identity deduplication, and its
architecture, validation, ADR, and source-provenance records.

## Findings and resolution

The first review requested changes for overly broad puncturing admission,
nonstandard bonded centers, per-row unknown handling, and noncanonical center
serialization. The implementation now rejects puncturing for 20/40/80+80
MHz, admits only the documented 80/160/320 MHz subset, protects the primary
segment, validates standardized centers and 80+80 blocks, stores canonical
centers, and preserves unknown or unsupported geometry in contribution rows.

The final semantic review found no BLOCKER or MAJOR issue. It confirmed the
asymmetric receiver-weighted coefficient, total integrated interferer-power
convention, linear `coupling × power × utilization` computation, explicit
zero/unknown/unsupported states, bounded input handling, deterministic radio
deduplication, and same-BSS/MLD policy. It also confirmed that the API does not
claim final SINR, CCA, airtime, capacity, or regulatory legality.

One MINOR provenance issue remained: the puncturing table cited Linux's
mutable `master` branch. The review record now pins Linux commit
`1a15bf9708ba3bf80410065e113aa17cd6a18dcf` and raw `net/wireless/chan.c`
SHA-256
`a58880f0b3225a5790e1874e6a88675e4ac0eb8098df432d597ca1549d79ded0`.
The reviewer applied that documentation-only correction. The orchestrator
independently verified the immutable URL, digest, absence of mutable source
references, source inventory, and diff before integration.

## Validation

- `cargo test -p kyberia-wifi-semantics --locked --offline`: PASS, 29 tests.
- `cargo clippy -p kyberia-wifi-semantics --all-targets --locked --offline -- -D warnings`: PASS.
- `cargo test --workspace --locked --offline`: PASS, 244 tests plus eight compile-fail doctests.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `python3 tools/architecture.py`: PASS.
- `.tools/venv/bin/python tools/source_inventory.py check`: PASS, 97 locked packages.
- `git diff --check`: PASS.

## Residual scope

The V1 mask remains a bounded fixed 1 MHz-bin approximation with a flat
integrated transmitter spectrum and trapezoidal receiver response. Complete
regional regulations, calibrated transmitter/receiver masks, CCA/airtime,
PHY/PER/capacity, spatial selection, and final SINR remain open requirements.
