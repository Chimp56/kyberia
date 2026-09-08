# Native capture session validation

The process/session contract is exercised by executable tests in
[`crates/observation-pipeline/src/process.rs`](../../crates/observation-pipeline/src/process.rs)
and
[`crates/observation-pipeline/src/tests.rs`](../../crates/observation-pipeline/src/tests.rs).
The fixtures are explicitly synthetic process-supervision fixtures. They do
not invoke CoreWLAN, request macOS consent, fabricate capture clocks or poses,
or represent shell output as measurements.

## Focused acceptance matrix

`supervised_scan_normalizes_persists_and_reopens_exactly` runs a real trusted
executable, decodes a valid stream, associates its canonical observations,
publishes the chunk and snapshot, and verifies exact reopen/readback.

`supervised_process_accepts_probe_denied_partial_error_and_empty_terminals`
checks the supported terminal states, closed exit-code mapping and persistence
of terminal/capability evidence for empty and non-success captures.

`supervised_process_rejects_malformed_flood_mismatch_and_untrusted_output`
checks malformed NDJSON, stdout and stderr limits, terminal/exit disagreement,
source-build mismatch and command/timeout provenance mismatch before durable
publication.

`supervised_process_enforces_timeout_cancellation_and_bounded_descendant_drain`
checks timeout, cancellation, a same-process-group descendant that holds a
pipe, and a short-lived `setsid` descendant outside the process group. The
test asserts bounded elapsed time; the reader implementation itself uses
polling and bounded channel drains rather than a blocking join.

`supervised_process_rejects_identifier_policy_mismatch_before_persistence`
checks that an included stream is rejected for a redacted scan request before
the adapter mapping callback or durable publication. The owned-infrastructure
mapping regression uses the same included stream and proves the callback is
never reached. The limit/interface test builds a bounded two-observation
stream from two active interfaces and checks both requested observation limits
and rejection of a mixed active-source result before mapping.

The unit tests in the `process` module cover typed option bounds, injection-like
interface rejection, trusted-path checks and the complete terminal/exit
mapping. Adapter tests separately cover source/build/clock/terminal protocol
validation.

On a host with the locally built signed collector, the ignored
`supervised_real_redacted_capability_probe_uses_the_rust_boundary` test invokes
the real `probe` executable through this Rust supervisor with
`include-identifiers` disabled. It verifies the trusted source-build hash,
redacted capability terminal, empty observation set and durable empty snapshot.
The test passed on the development host after:

    /usr/bin/python3 collectors/macos/build.py
    cargo test -p kyberia-observation-pipeline --lib supervised_real_redacted_capability_probe_uses_the_rust_boundary --locked --offline -- --ignored

This probe does not request consent or establish an authorized scan or
hardware RSSI evidence.

## Commands

Run the focused process/session tests offline:

    cargo test -p kyberia-observation-pipeline --lib supervised_ --locked --offline
    cargo test -p kyberia-observation-pipeline --lib process --locked --offline

Run the package and repository gates:

    cargo test -p kyberia-observation-pipeline --locked --offline
    cargo test --workspace --locked --offline
    cargo clippy --workspace --all-targets --locked --offline -- -D warnings
    cargo fmt --all -- --check
    /Users/vincent/code/kyberia/.tools/venv/bin/python tools/architecture.py
    /Users/vincent/code/kyberia/.tools/venv/bin/python tools/source_inventory.py check

`git diff --check` is also required before review. The native process tests
are POSIX-gated because the safety property depends on a private process group
and `poll(2)` pipe supervision. The non-Unix build returns the explicit
`UnsupportedPlatform` outcome rather than silently substituting an unbounded
blocking reader.

## Remaining validation

Hardware validation still needs a real signed/approved collector, CoreWLAN
authorization behavior, source-build attestation provisioning, operator
consent UX and a production identity-mapping implementation. Those gates are
outside this deterministic synthetic process harness.

## Independent integration regression

Integrated through `365a8e6`. Root executed:

- `cargo test --workspace --locked --offline`: 426 passed, zero failed,
  nine explicitly ignored tests/benchmarks, including doctests in the total.
- `python3 tools/dev.py lint`: workspace formatting, Clippy and architecture passed.
- `.tools/venv/bin/python -m unittest discover -s tests -p 'test_*.py'`:
  179 run, 19 skipped, no failures.
- `python3 tools/validation/fixtures.py check`: passed.
- `.tools/venv/bin/python tools/source_inventory.py check`: 223 packages passed.
- `python3 tools/ledger.py check`: complete source coverage and evidence passed.

The ignored native probe is not RF scan validation. The full suite resumed
only after all identified automatic test-directory cleanup was replaced with
retained fixtures. Hardware and product workflow acceptance remain separate.

## Independent macOS redacted runtime probe

Root built the integrated collector with `python3 collectors/macos/build.py`
and passed one explicitly selected real-host test:
`cargo test -p kyberia-observation-pipeline tests::supervised_real_redacted_capability_probe_uses_the_rust_boundary --locked --offline -- --ignored --exact`.
The initial invocation omitted the `tests::` prefix and selected zero tests;
only the corrected one-test invocation is validation evidence.

Host: macOS 26.6.2, arm64.
Collector source-build identity: `sha256:d574ee6190aaf9c0ab3d096dd1d28a17c4267c4f935d1b1f497b07aa8767cdf0`.
Local ad-hoc signed executable SHA-256: `92d7f61adeaef0187c90012ec1c003362b22d3af17ecb2058cfa0f12e98b4efc`.
The build verified its ad-hoc signature; this is not notarized release signing.
The test proved successful probe terminal/exit status, zero RF observations,
zero survey associations, and publication of the capability-session snapshot
through the integrated Rust process and storage boundary. It did not invoke
authorization or scan, so consent and measured RF capture gates remain open.
