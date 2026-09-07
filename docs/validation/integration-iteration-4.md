# Iteration 4 integration validation

Scope: reviewed native macOS normalization and correction of the project container's silent-write defect. Phase 0 remains open; this is not a usable desktop survey acceptance.

## Integrated units

- Capture implementation `d6189d6`, independent review `21cd7e7`, dependency/traceability integration `a3a932b`.
- Storage correction `8f83a2f`, independent review `0d0ff97`, following independently reproduced P0-MID-001 in `20b309e`.
- Internal package version pins `b28bdaa` allow the strict dependency bans policy to pass without wildcard exemptions.

## Commands and results

| Command | Result and scope |
|---|---|
| `python3 tools/dev.py check` | PASS: 159 Rust tests, seven compile-fail doctests, 110 Python tests; 18 optional environment tests skipped. Formatting, Clippy, Cargo typecheck, architecture, source inventory, lossless ledger and synthetic-fixture checks pass. |
| `python3 tools/dev.py build` | PASS: complete current Rust workspace, locked/offline. |
| `cargo check --workspace --all-targets --locked --offline` | PASS after internal package version constraints. |
| `.tools/venv/bin/python tools/source_inventory.py check` | PASS: all 97 locked external packages match their source inventory. |
| `python3 tools/dev.py benchmark` | PASS: original deterministic project-operation workload, results below. |

Six Rust support tests are deliberately ignored by ordinary test runs: five explicit benchmarks and one canonical-golden regeneration command. The 18 Python skips are five native collector lifecycle tests and thirteen optional real analytical-engine tests; those environments have separate successful validation recorded in their reviews. The Sionna contract/lifecycle tests run in this ordinary suite; actual CPU engine execution remains a separate pinned environment.

## Dependency policy proof

The actual downloaded cargo-deny 0.20.2 binary was checksum-verified against the upstream release artifact. Running it with `--config .tools/supply-chain/deny-proof.toml --locked --offline check all --hide-inclusion-graph` on the current workspace reports **advisories, bans, licenses and sources all OK**. The current RustSec checkout is `faedffd5118c1835e13cca3babb6059afb1eb8d0`. Duplicate versions of getrandom, hashbrown, r-efi and syn remain visible warnings; they are not advisory exceptions. This command uses retained research configuration, not the new automation awaiting independent review. Complete platform/dependency/distributable coverage remains open.

The new native parser uses patched time 0.3.55. No production core dependency imports CoreWLAN, Kismet or Sionna objects. The only storage dependency change enables rusqlite hooks already resolved elsewhere in the workspace.

## Replay performance

Measured on the existing macOS ARM64/Rust 1.98.1 host with the checked-in `crates/domain/examples/project_benchmark.rs`, release profile. Inputs are the example's deterministic site-creation operation sequences; timings exclude compilation and are single-run diagnostics, not CI limits.

| Sites / operations | Final revision | Elapsed |
|---|---|---|
| 100 | 100 | 1.010 ms |
| 1,000 | 1,000 | 37.740 ms |
| 10,000 | 10,000 | 2,111.923 ms |

This measures domain project-operation replay, not SQLite persistence, scan ingestion, rendering or a complete project open. The known quadratic replay behavior remains technical debt for large projects. The native decoder's separately reviewed 4,096-observation baseline is 89.212 ms decode and 5.797 ms normalization; it must be scheduled off the UI thread.

## Outstanding acceptance

Native transport/host ingestion, receipt-based point assignment with honest uncertainty, live inspector, project observation persistence and desktop workflows remain open. The storage correction rejects executable schemas and checks authoritative writes, but does not claim hard native-memory, sidecar or hostile filesystem-race isolation. Supply-chain automation and renderer comparison are being implemented by isolated Luna xhigh agents with independent review required; their existence is not acceptance evidence.
