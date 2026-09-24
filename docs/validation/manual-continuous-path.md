# Manual continuous-path model (SUR-002) validation

This is an isolated, model-level increment on branch
`feat/phase1-manual-path-survey-current-20260923`, based on
`afc5070fadac46411f02d355a0d00351f388205f`. The change has not been integrated
to `main` or independently reviewed. It does not claim Phase 1 or SUR-002 exit.

## Implemented scope

The model in `crates/survey/src/manual_path.rs` is pure and source-local. It
records timestamped start, turn, pause, resume, and stop anchors, each from one
configured `ClockEpochId` and `FrameId`. Anchor event times must strictly
increase; duplicate observation IDs, foreign epochs, foreign pose frames,
invalid transitions, and arithmetic overflow are rejected. Observation capture
times and IDs are retained verbatim and sorted by `(nanoseconds, observation
ID)` for deterministic output. Distinct IDs may have equal capture timestamps.

Only adjacent anchors in one active leg support manual projection. An exact
anchor-time sample receives that anchor; an interior sample uses the
source-monotonic nanosecond ratio and linear interpolation, including values
near the `u64` timestamp ceiling without collapsing the fraction to an endpoint.
This is explicitly
an assumed uniform-motion position, not a measured pose. Times after the most
recent anchor are retained as unresolved until an endpoint arrives. Pause and
resume create separate legs; samples in a pause remain unpositioned absent a
trustworthy supplied pose. No manual extrapolation or pause bridging occurs.

A known pose takes precedence only when it is in the configured frame and has
known covariance whose largest per-axis standard deviation is no greater than
the caller-configured limit. Unknown or excessive uncertainty is reported in
`PoseDecision` and falls back to supported manual interpolation. The original
operator coordinates are immutable; post-stop edits add a corrected coordinate
without changing anchors' original positions, timestamps, IDs, or raw
observation references. Position and diagnostics are recomputed from current
anchors on access.

Speed diagnostics use 3D endpoint distance divided by elapsed monotonic seconds.
Sharp-turn diagnostics use the heading change in the horizontal x-y plane. Both
thresholds are caller-configured in meters/second and radians. Exceedances are
typed advisories, not hard survey rejection. Channel gaps are generated only
from caller-provided per-frequency schedule and completeness intervals. Complete
coverage subtracts from half-open schedule intervals; unknown/incomplete
coverage does not. Gaps are clipped to supported active path segments and
explicitly mean “scheduled interval without complete coverage,” never AP
absence or negative RF evidence.

Bounds: 512 path anchors, 8,192 raw observations, 512 schedule intervals,
2,048 coverage intervals, and 16,384 output gaps. Serde decoding bounds the
anchor and observation sequences before accepting an extra record and then
revalidates state invariants. The application/storage boundary must still
enforce its byte/depth limit before decoding and reconcile each observation ID
with the canonical stored envelope. This crate does not persist snapshots.

## Regression evidence

`crates/survey/tests/manual_path.rs` covers exact timestamp interpolation,
start/turn/pause/resume/stop, pause gaps, unresolved future segments, no
extrapolation, reported-pose precedence and covariance fallback, wrong and
duplicate times/IDs, mixed epochs/frames, speed/turn thresholds, edited-anchor
reprojection preserving original IDs/times/coordinates, schedule-only channel
gaps, complete versus incomplete coverage, pause-safe gaps, malformed
serialization and sequence bounds, and interior interpolation near the `u64`
timestamp ceiling. A package unit test checks anchor and sample admission caps.

The source and focused test file content SHA-256 values at this candidate are:

- `crates/survey/src/manual_path.rs` —
  `46aeb29a41ad7e84dac1f6fc2c4cb6f8c39b05b94646cae41d1f8390e06339f9`.
- `crates/survey/tests/manual_path.rs` —
  `8e2265e25c3d7a9314a7747feaa3ce9767608549ddee05e07a794f5bfd78fc40`.
- `docs/architecture/survey-state.md` —
  `acc62d3c29039202af0828ca1986e49d95d62439bff22b3422b660437f31bbfe`.

## Reproduction commands and results

Run from the repository root. The isolated local registry cache and build output
used for this candidate are under the assigned worktree; replace the paths if
reproducing in a fresh checkout.

```sh
CARGO_HOME=<assigned-worktree>/.trash/phase1-manual-path-cargo-home \
CARGO_TARGET_DIR=<assigned-worktree>/target \
cargo test -p kyberia-survey --locked --offline

CARGO_HOME=<assigned-worktree>/.trash/phase1-manual-path-cargo-home \
CARGO_TARGET_DIR=<assigned-worktree>/target \
cargo clippy -p kyberia-survey --all-targets --locked --offline -- -D warnings

cargo fmt --all -- --check
python3 tools/architecture.py check
python3 tools/source_inventory.py check
python3 tools/ledger.py generate
python3 tools/ledger.py check
git diff --check
```

On the assigned macOS ARM64 host, Rust/Cargo 1.98.1, the focused survey suite
passes 46 tests with one pre-existing ignored release benchmark. Strict package
Clippy, formatting, architecture, source inventory, ledger generation/check,
and diff whitespace checks pass. The source inventory remains 522 locked
external packages; `Cargo.lock` is unchanged. Python 3.9.6 runs the repository
checks. A copied 1.0 GiB registry cache is retained under the assigned
worktree's ignored `.trash/phase1-manual-path-cargo-home`; it was not deleted.
These are local pure-model checks only; they do not test a desktop,
capture backend, persistence adapter, human walking pace, or field usability.

## Traceability and disposition

`catalog:SUR-002:1`, its five source-qualified bullets, and `backlog:MAPB-004:1`
remain `IN_PROGRESS`. This code does not implement the map HUD, application
commands, capture orchestration, durable project wiring, calibrated/map-frame
admission, human usability, or independent review. Broader Phase 1 and SUR-002
acceptance therefore remains open.
