# Phase 5 band-power tie regression review

Decision: **APPROVE** the narrow tie-regression candidate, commit
`b769724477f843a4bc5961c01bbcc7dad5b128a8`, based on
`a8cf7dccbe1a1c40d6adc51868e90dfe2cdcd452`.

## Findings

No blocker, major, or minor findings.

The source change extracts the existing final `f64::round()` into the private
`round_milli_dbm_ties_away_from_zero` helper. The existing public
milliwatts-to-rounded-milli-dBm path calls that helper only after the
logarithmic conversion. The new unit test invokes this exact final-quantization
seam with exact `+0.5` and `-0.5` milli-dBm values and their immediately
adjacent representable `f64` values. The assertions distinguish ties-away-from-
zero from nearby values on both signs. This is a direct test of the documented
quantizer behavior, not a full-sweep fixture: no claim is made that a sweep's
`log10` output is exactly halfway.

The refactor preserves the public API, the `round()` operation, and the
existing non-finite/range check and error mapping. It does not mutate the
source sweep. The test is not tautological: it exercises the production
quantizer implementation at the only controllable input boundary for exact
binary half-way values.

The authoritative `plan.md` SHA-256 is
`1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`.
Phase 5/SPE-002 requires considerably more than this calculation-level
regression. `SPE-002` remains `IN_PROGRESS`; this review makes no spectrum,
product, or Phase 5 acceptance claim.

## Independent validation

All checks below were run in the isolated review worktree at
`/private/tmp/kyberia-phase5-bandpower-tie-review-20260923`:

- `cargo test --locked --offline -p kyberia-spectrum-contract` — 1 unit test,
  19 integration tests, and 0 doctests; all 20 tests passed.
- `cargo clippy -p kyberia-spectrum-contract --all-targets --locked --offline -- -D warnings` — passed.
- `cargo fmt -p kyberia-spectrum-contract -- --check` — passed.
- `python3 tools/architecture.py check` — passed.
- `python3 tools/source_inventory.py check` — passed, 522 locked external packages.
- `python3 tools/ledger.py check` — passed, 5,396 source blocks, 438 explicit
  ID occurrences, and 447 headings.
- `git diff --check a8cf7dccbe1a1c40d6adc51868e90dfe2cdcd452..b769724477f843a4bc5961c01bbcc7dad5b128a8` — passed.

Verified SHA-256 values:

- `crates/spectrum-contract/src/band_power.rs`:
  `c74add0289f21ca90b3ede8e087cb1e7e43e472cda10cdc12208a08d9955b773`
- `docs/validation/phase5-spectrum-contract.md`:
  `690382de1f95c24f4b833ac7d89f01005c6f4a8c625c6aba8f34cf5a68194e4e`

No codebase-memory graph tools were available in this review context, so no
graph coverage claim is made.
