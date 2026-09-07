# Independent review: canonical analysis manifest V1

Scope: `/Users/vincent/code/kyberia/.worktrees/manifest-luna-review`, branch `review/manifest-luna`, copied from the frozen author worktree. The author source worktree and main worktree were not edited. Review covered only the 13 paths in `.worktrees/analysis-manifest/.tools/analysis-freeze.json`; `.review/` contains only independent probes and this report.

## Freeze verification

All 13 SHA-256 values matched the freeze manifest:

```text
Cargo.lock                                      7c1c1fd9a9b46cc121bccc17d5d2ec3b5d9308967a8e149ad3122ad0057b1ff7
crates/domain/Cargo.toml                        02e91346a4b5dd81180d49c9378192b21bec2c879ea06d2f29f03832898d0f33
crates/domain/src/analysis.rs                   7ee0556dcfde25aaaf59e3f269714c64f0adfdada1b679f0627ae5bce0f1377b
crates/domain/src/lib.rs                         8ab35e9388a72956ed7c2b7e7c0834032d48bf01f0d7d09248b1e4f8a84ac8b1
crates/domain/src/units.rs                       9f1f7d3da6d687c2ea7ef262d88ddba07443c075aa1e8c6b65aebe495a843078
crates/domain/tests/analysis_manifest.proptest-regressions d7c90e10ec45da2edd1d16f75c132d5329fd50eeb1d37112c2f3bce28a7b2afd
crates/domain/tests/analysis_manifest.rs         573a3ed6042eba5a226654870e04e762cf848ca98b0f47fec7ad934e431412dd
crates/domain/tests/contracts.rs                 e7dfd98947b77a97223a79023629f7c57ae03217dad51da44d26340885999d60
crates/domain/tests/fixtures/analysis-manifest-v1.json ae595b6518430dd7ffc8424aadb16678b0b2b076ed063397bc48d1c2c69539f0
docs/architecture/ADR/0009-analysis-manifest.md cb33b9ab413b8595776cea523a0f018ed0b0714990710efaa8201682167f91b4
docs/architecture/analysis-manifest.md           64cf880b4c470f7059f7f173d20873e0325bab7d53a0fafaba577a9f193637c0
docs/licenses/cargo-sources.json                 b5e73dd77112bfa9bdc95ab29f259c8d7beeccfa7ba8d8dc4821ada6650b3653
tools/architecture.json                          1018ee379d8c3c1db59f8f743dc03216f28655d5367f7e69d42a848c249f1b66
```

## Commands and results

- `CARGO_TARGET_DIR=.review/target cargo test -p kyberia-domain --test analysis_manifest --locked --offline`: **13 passed, 1 ignored**.
- `CARGO_TARGET_DIR=.review/target-full cargo test --workspace --locked --offline`: all workspace tests passed, including **172 Rust tests and 7 doctests** (ignored benchmarks were not executed).
- `CARGO_TARGET_DIR=.review/target-clippy cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: **pass**.
- `cargo fmt --all --check`: **pass**.
- `/Users/vincent/code/kyberia/.tools/venv/bin/python tools/architecture.py`: **pass**.
- `/Users/vincent/code/kyberia/.tools/venv/bin/python tools/source_inventory.py check`: **97 locked external packages, pass**.
- `/Users/vincent/code/kyberia/.tools/venv/bin/python tools/validation/fixtures.py check`: **pass**.
- `/Users/vincent/code/kyberia/.tools/venv/bin/python -m unittest discover -s tests -p 'test_*.py'`: **128 total, 110 passed, 18 skipped**.
- Release benchmark `cargo test -p kyberia-domain --test analysis_manifest --release --locked --offline benchmark_manifest_roundtrip -- --ignored --nocapture`: **pass**; 4,096 refs, 1,042,059 bytes, encode/hash 21,406 µs, decode/validate/hash 20,162 µs.
- Independent Python hash: fixture **2,183 bytes**, no newline, SHA-256 `ae595b6518430dd7ffc8424aadb16678b0b2b076ed063397bc48d1c2c69539f0`.
- Independent probe in `.review/src/main.rs`: extra and duplicate fields on Boolean, Count, Dimensionless, and Artifact parameter variants were rejected; 2,049 deterministic random inputs of length 0..2048 produced **0 panics**.

## Findings

### No BLOCKER or MAJOR implementation findings

Canonicalization includes the versioned schema, all artifact references and byte lengths, identity/session/snapshot references, client/selection/algorithm/execution/randomness/parameter/grid fields. Survey and parameter sets are sorted with duplicate checks. Full-width integers are canonical decimal strings. The grid guard rejects zero/overflow/collapsed precision cases. Artifact verification checks both exact length and SHA-256. The `float_roundtrip` feature preserves finite binary64 round trips, including the retained tiny-float regression. No scheduler, cache, metric registry, artifact-closure loader, or analysis execution is claimed; the docs and ADR scope these as follow-on integrations.

### MINOR: generated source ledger is stale in the review snapshot

`/Users/vincent/code/kyberia/.worktrees/manifest-luna-review/docs/implementation/ledger.json:161595-161596` still records the old `crates/domain/src/units.rs` digest `ec884a...`, while the frozen file is `9f1f7d...`. `/Users/vincent/code/kyberia/.tools/venv/bin/python tools/ledger.py check` reports `backlog:FND-001:1: stale evidence digest: crates/domain/src/units.rs`. This is outside the 13 frozen paths and is a required integration bookkeeping update before merge.

### No Serde schema finding

`docs/architecture/analysis-manifest.md:15` records the historical regression involving internally tagged `Randomness::Deterministic` / `ClientProfile::NotApplicable` variants and the current empty-struct rejection. With the exact pinned dependency and current code, the independent probe also rejects an `extra` field on Boolean, Count, Dimensionless, and Artifact `ParameterValue` variants and rejects duplicate `unit`/`value` keys. Current schema behavior is closed; no finding remains here.

### NIT: cache-invalidation test coverage is narrower than its name

`crates/domain/tests/analysis_manifest.rs:96-140` mutates the major references and grid fields, but does not independently mutate `algorithm.name`, parameter names, survey session IDs, or each `VersionedArtifact` metadata field (version/media type/length). The canonical serializer currently includes those fields, so this is regression-test-strength debt rather than an observed hash defect.

## Recommendation

**Approve the frozen manifest implementation with the ledger refresh required before integration.** No unresolved BLOCKER or MAJOR semantic/security finding was observed. Keep the documented scope: this is a pure canonical spatial-analysis identity and byte verifier, not a job executor/cache/metric registry or recursive artifact-closure validator.
