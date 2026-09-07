# Foundation iteration 1 validation

Environment: macOS 26.6.2 ARM64, Rust/Cargo 1.98.1, Python 3.9.6. Canonical domain commit: `de50f5f` (integration of independently reviewed `e6c4a4b91badc50d5e77c3fd00b2bc3eb444965a`). Storage and CLI are the accompanying reviewed patch; this report is not a cross-platform or live-capture gate.

Executed:

- `cargo test --workspace --offline`: PASS, 18 domain contract/property tests, 13 storage tests, two executable CLI workflows and seven compile-fail doctests.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: PASS.
- `cargo fmt --all`: PASS.
- `python3 tools/ledger.py check` and 31 ledger adversarial tests: PASS in independent tooling review; repeated at integration.

Storage tests demonstrate fresh-directory creation, identity/content round trips, read-only enforcement, invalid declaration rollback, missing/corrupt bytes, stale JSON diagnosis/recovery, bounded hash paths, direct symlink rejection, two-writer inventory preservation, schema-version rejection from stale handles, mismatched database revisions, and rollback on projection publication failure. The CLI test invokes the built executable and checks success/failure exit codes through create, inspect, verify and recovery.

Initial compilation exposed SQLite's signed integer API mismatch; explicit checked conversion now maps the bounded manifest revision. SQLite resource-limit configuration errors are propagated. Independent review also corrected stale-handle schema checking, ambiguous publication ordering and repeated manifest parsing during verification. Canonical covariance review rejected a previously accepted indefinite matrix and verified the fix across scaled, permuted and singular cases.

Not covered: process-kill crash injection, concurrent malicious filesystem replacement, second operating system execution, full migrations, performance thresholds, GUI workflows, radio hardware or external propagation runtimes. These remain implementation/validation work rather than claimed successes.
