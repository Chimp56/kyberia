# Phase 5 spectrum correction re-review

Disposition: APPROVED for the two previously reported MAJOR findings.

Reviewed correction: `9be9e0fd60384d13743a08ee3d9b8672f3c7f716` (implementation) and `50313bd4dbcbadb7d39b2903aa71162b9c69ad3e` (tracking/docs), based on `2245c0d0cc64c3de000b88e74868218d0f992b02`. Review was performed in a fresh isolated worktree at `/private/tmp/kyberia-phase5-spectrum-rereview-20260923`; no candidate files were changed.

## Previously reported findings

1. **dBm/Hz compared against a dBm threshold — resolved.** `assess` now returns an explicit `Unknown/PowerUnitUnsupported` result for `PowerUnit::DbmPerHertz`, while the original validated sweep content remains referenced and round-trips unchanged. It does not invent a conversion from bin spacing. A new test verifies preserved PSD bins, zero occupied/active claims, event decoding, and the explicit reason. The ordinary signature path remains limited to dBm-per-bin evidence.

2. **Global support hiding sparse per-bin evidence — resolved.** A candidate bin must now be determinate in at least 800,000 ppm of all event sweeps before it can count as persistent. Low-coverage bins with observed above-threshold energy cannot produce a positive pattern when no other qualifying persistent bin exists. The regression verifies a 50%-observed bin remains unknown despite sufficient global support and activity; an exact 80% local-coverage fixture verifies the documented boundary is accepted. Coverage is also included in the result diagnostics and support score.

The regression inputs exercise the reported failure modes directly. I found no remaining MAJOR issue in either correction.

## Versioning, docs, and traceability

- The event schema and pattern rules are explicitly bumped to `/2`; the unchanged signature-input and temporal-policy shapes remain `/1`. Event input identity includes the v2 rule-set identifier, so the revised rules produce a distinct analysis identity.
- README and validation notes accurately say PSD is retained but not classified pending equivalent-noise-bandwidth normalization; they explain the local coverage floor and do not claim hardware, classifier, or Phase 5 completion.
- STATUS stays in progress and explicitly leaves adapters, normalization, hardware, remote replay, labeled traces, and runtime gates open.
- SPEB-001/004/005 remain `IN_PROGRESS`; their implementation/validation file hashes match the current files. `python3 tools/ledger.py check` validates the complete ledger and generated traceability.
- The architecture and source inventory remain consistent; no dependencies or architecture edges were added.

Minor diagnostic note: a legacy serialized event `/1` lacks the new v2 assessment field, so typed deserialization fails with `SpectrumError::Serialization` before the schema check can return `UnsupportedSchema` (`signature.rs` event decode path). The v1 payload is rejected and cannot be misinterpreted, so this does not block the correction. If callers need to distinguish unsupported prior versions from malformed bytes, add an envelope/version-first decoder and a regression test.

## Independent validation

- `cargo fmt -p kyberia-spectrum-contract -- --check` — PASS.
- `cargo test -p kyberia-spectrum-contract --locked --offline` — PASS: 14 integration tests; no unit or doctests.
- `cargo clippy -p kyberia-spectrum-contract --all-targets --locked --offline -- -D warnings` — PASS.
- `python3 tools/ledger.py check` — PASS: 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `python3 tools/architecture.py check` — PASS.
- `python3 tools/source_inventory.py check` — PASS: 522 locked external packages.
- `python3 -m json.tool docs/implementation/ledger.json` — PASS.
- `git diff --check 2245c0d0cc64c3de000b88e74868218d0f992b02 HEAD` — PASS.
- Local toolchain matched the validation note: `rustc 1.98.1`, `cargo 1.98.1`.

All evidence remains synthetic contract validation. No spectrum device, equivalent-noise-bandwidth conversion, SoapySDR/vendor adapter, remote-sensor replay, labeled trace, or physical calibration was exercised. This approval covers only the two corrected contract semantics, not Phase 5 runtime acceptance.
