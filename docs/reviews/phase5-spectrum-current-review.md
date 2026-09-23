# Phase 5 spectrum contract review

Disposition: CHANGES REQUIRED before integrating this candidate.

Reviewed candidate: `2245c0d0cc64c3de000b88e74868218d0f992b02` (`feat/phase5-spectrum-current`), based on `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`.

Review scope: hardware-independent spectrum sweep/calibration and generic signature contracts against `plan.md` sections 5.18, 6.6, Phase 5, and 18.9, including SPEB-001/004/005 and SPE-001/003/004/005. This review does not certify a SoapySDR/vendor adapter, visual spectrum UI, physical calibration, labeled-trace classifier, or Phase 5 completion.

## Findings

### MAJOR — Signature comparison mixes dBm and dBm/Hz

`PowerUnit` accepts both `DbmPerBin` and `DbmPerHertz` (`crates/spectrum-contract/src/model.rs:102-109`), and a signature input stores the selected unit (`crates/spectrum-contract/src/signature.rs:47-56`). `SpectrumEvent::from_sweeps` accepts either unit but its threshold is always `threshold_milli_dbm` (`signature.rs:129-139`); assessment then compares bin values directly against that threshold without branching on `power_unit` (`signature.rs:317-341,363-367`). The same dBm threshold therefore has different and incorrect meaning for PSD readings in dBm/Hz, potentially changing an event from occupied to unoccupied (or vice versa). The current fixtures exercise only `DbmPerBin`.

Before integration, make threshold units explicit and compatible with the sweep unit, perform a documented conversion using sufficient bandwidth metadata, or reject `DbmPerHertz` for signature assessment until normalization exists. Add paired tests showing equivalent physical signals expressed as dBm/bin and dBm/Hz yield equivalent classifications after the chosen conversion; also verify invalid unit/threshold combinations remain unknown or fail closed.

### MAJOR — Global support can hide sparse per-bin evidence

Per-bin persistence is calculated as `above_threshold / determinate` and qualifies after only five determinate observations (`signature.rs:317-360`). Missing/clipped bins are excluded from `determinate`; the later 80% support gate is global across the whole grid, not per persistent bin (`signature.rs:370-418`). Consequently, a frequency bin observed above threshold in five sweeps and unobserved in the other 507 can receive 100% per-bin persistence, while unrelated alternating energy in other bins keeps global frequency-time and active-sweep support over 80%. That path can emit `NarrowbandPersistentPattern` even though absence/presence at the claimed frequency is largely unknown. Existing tests cover globally sparse hopping, but not localized bin gaps masked by otherwise high global support (`tests/contract.rs:354-370`).

Before integration, require sufficient observation coverage per classified bin relative to the event's total sweep window (or define a conservative per-bin support rule that emits `Unknown` below a documented floor). Add a regression with a mostly unobserved candidate bin and adequate unrelated/global support; it must not claim a persistent pattern. Keep global support as a separate diagnostic rather than using it as a substitute for frequency-local support.

## Validation performed

- `cargo test -p kyberia-spectrum-contract --locked --offline` — PASS: 11 integration tests; no unit or doctests.
- `cargo clippy -p kyberia-spectrum-contract --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS: 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `python3 tools/architecture.py check` — PASS.
- `python3 tools/source_inventory.py check` — PASS: 522 locked external packages.
- Worktree initially clean at the specified candidate commit; review build outputs were retained in its ignored `target/` directory.

## Scope and evidence limits

The candidate provides a useful versioned hardware-independent schema: bounded frequency grids and canonical bytes, explicit observed/below-threshold/not-observed bins, time/pose/calibration references, deterministic sweep ordering and event hashes, and unknown results for several incomplete-evidence cases. The README, STATUS row, and ledger correctly describe this as synthetic contract work and leave adapters, actual calibration, path completeness, labeled traces, and runtime gates open. Architecture and source inventory entries match the new crate and its resolved dependencies.

These checks are local synthetic/schema validation only. No spectrum hardware, external labeled traces, remote-sensor replay, visualization, or physical calibration was available or exercised. The two findings affect the semantics of positive signature results and should be resolved before treating those results as usable interferer-pattern evidence.
