# Analysis manifest integration validation

Date: 2026-09-07. Host: macOS 26.6.2 ARM64, Rust 1.98.1. Scope: the pure spatial-analysis identity and artifact-byte verifier in backlog FND-008, not analysis execution or a usable heatmap workflow.

The [independent Luna xhigh review](../reviews/manifest-luna-review.md) approved all 13 frozen source files after numerical, malformed-input, architecture and workspace checks. Integration changes only clarified historical enum wording, accepted ADR-0009, and refreshed evidence/traceability. No reviewed runtime code was changed.

Commands executed in the primary integration tree:

| Command | Result |
|---|---|
| `cargo test --workspace --locked --offline` | PASS: 172 tests and seven compile-fail doctests; seven deliberate benchmark/golden commands ignored |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | PASS |
| `.tools/venv/bin/python -m unittest discover -s tests -p 'test_*.py'` | PASS: 110 tests, 18 optional tests skipped (128 discovered) |
| `.tools/venv/bin/python tools/architecture.py` | PASS |
| `.tools/venv/bin/python tools/source_inventory.py check` | PASS: 97 locked external packages |

Thirteen manifest tests cover golden encoding/hash, full-width seeds, finite binary64 round trips, set ordering, 20 major cache inputs, malformed/oversized specifications, framed grid precision and artifact hash/length verification. The independent review also ran 2,049 bounded random inputs without a panic. The [format documentation](../architecture/analysis-manifest.md) retains the explicit release benchmark inputs and measurements.

Known limits: individual algorithm names, parameter names, session IDs and every artifact metadata field are serialized but not each separately mutated by the cache-invalidation test. This is recorded test-strength debt, not a missing hash input. No transitive artifact loader, metric compatibility registry, scheduler, cache storage or user-facing analysis is claimed. Those requirements remain open.
