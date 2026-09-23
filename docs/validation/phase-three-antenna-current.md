# Phase 3 antenna-pattern candidate validation

Implementation base revision: `b86bd130dc77443fe53a6840f28cdc9d5e98b4a6` on
`feat/phase3-antenna-current`, based on `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`.
This follow-up adds a non-pole elevation-interpolation golden; its exact source
bytes are recorded by content hash in the implementation ledger.

This validation covers only the versioned JSON importer and evaluator in
`crates/antenna-model`. The tests use synthetic patterns; there is no licensed
manufacturer dataset, visual normalization review, full product integration,
or Sionna execution in this evidence.

| Command | Result and scope |
| --- | --- |
| `cargo test -p kyberia-antenna-model --locked --offline` | PASS: 10 integration tests; includes a local +X/45° elevation golden between the 0° +6 dBi sample and +90° −10 dBi pole. No unit or doctests are defined. |
| `cargo test -p kyberia-antenna-model --locked --offline direction_elevation_and_frequency_interpolation_are_in_linear_power -- --exact` | PASS: targeted non-pole elevation golden. |
| `cargo clippy -p kyberia-antenna-model --all-targets --locked --offline -- -D warnings` | PASS: all crate targets, warnings denied. |
| `cargo fmt --package kyberia-antenna-model -- --check` | PASS: candidate crate formatting. |
| `python3 tools/architecture.py` | PASS: reviewed dependency directions and external package boundaries. |
| `python3 tools/source_inventory.py check` | PASS: 522 locked external packages. Count was 522 before and after this local crate addition; the generated inventory diff updates only the Cargo.lock SHA-256. |
| `python3 tools/ledger.py check` | PASS: 5,396 source blocks, 438 explicit ID occurrences, 447 headings. |
| `python3 -m unittest discover -s tests -p 'test_ledger.py'` | PASS: 31 ledger tests. |
| `git diff --check` | PASS: candidate source commit whitespace check. |
| Draft 2020-12 schema conformance | NOT RUN: no executable schema validator is available in this worktree. The repository pins `jsonschema==4.25.1` for supply-chain tooling, but it is not installed and `.tools/venv` is absent; the pip cache is unavailable. Pinned AJV 8.20.0 exists only in the Lab MCP lockfile, but is neither installed nor cached here. `cargo search jsonschema --offline` failed because no local index entry was available (`attempting to make an HTTP request`). No dependency was added. |

The evidence exercises explicit axes/rotation, pole and flattening behavior,
the non-pole elevation golden and spatial/frequency interpolation,
co/cross-plane availability, uncertainty and
provenance fields, canonical signed-zero/quaternion identity, malformed inputs,
and parser/work bounds. It does not establish vendor-format compatibility,
license entitlement or source-checksum correspondence, polarization mismatch
loss, cuts/harmonics, visualization, field calibration, or adapter behavior.

Ledger records are intentionally left `IN_PROGRESS`: an independent review and
subsequent integration decision are still required.

The executable Draft 2020-12 conformance gate remains explicitly open. The
existing schema test only checks that the file parses as JSON and that selected
structural fields are present; it is not a substitute for a standards validator.
The independent review classified validator availability as a MINOR gap, but
the plan-tracking record continues to show schema conformance as unverified.
