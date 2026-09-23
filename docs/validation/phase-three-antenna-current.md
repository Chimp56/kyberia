# Phase 3 antenna-pattern candidate validation

Candidate source revision: `b86bd130dc77443fe53a6840f28cdc9d5e98b4a6` on
`feat/phase3-antenna-current`, based on `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`.

This validation covers only the versioned JSON importer and evaluator in
`crates/antenna-model`. The tests use synthetic patterns; there is no licensed
manufacturer dataset, visual normalization review, full product integration,
or Sionna execution in this evidence.

| Command | Result and scope |
| --- | --- |
| `cargo test -p kyberia-antenna-model --locked --offline` | PASS: 10 integration tests; no unit or doctests are defined. |
| `cargo clippy -p kyberia-antenna-model --all-targets --locked --offline -- -D warnings` | PASS: all crate targets, warnings denied. |
| `cargo fmt --package kyberia-antenna-model -- --check` | PASS: candidate crate formatting. |
| `python3 tools/architecture.py` | PASS: reviewed dependency directions and external package boundaries. |
| `python3 tools/source_inventory.py check` | PASS: 522 locked external packages. Count was 522 before and after this local crate addition; the generated inventory diff updates only the Cargo.lock SHA-256. |
| `git diff --check` | PASS: candidate source commit whitespace check. |

The evidence exercises explicit axes/rotation, pole and flattening behavior,
spatial/frequency interpolation, co/cross-plane availability, uncertainty and
provenance fields, canonical signed-zero/quaternion identity, malformed inputs,
and parser/work bounds. It does not establish vendor-format compatibility,
license entitlement or source-checksum correspondence, polarization mismatch
loss, cuts/harmonics, visualization, field calibration, or adapter behavior.

Ledger records are intentionally left `IN_PROGRESS`: an independent review and
subsequent integration decision are still required.
