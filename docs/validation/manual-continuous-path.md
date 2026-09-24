# Manual continuous-path model (SUR-002) validation

This model-level increment was authored on
`feat/phase1-manual-path-survey-current-20260923` at `91060ef`, based on
`afc5070`, and assembled on current-main base `172c4a3` in
`integrate/phase1-manual-path-main-20260923`. Independent source review
approved the implementation and work-budget correction with no unresolved
findings in
[`phase1-manual-path-budget-rereview-20260923.md`](../reviews/phase1-manual-path-budget-rereview-20260923.md).
The assembled current-main integration was independently approved with no
scoped findings in
[`phase1-manual-path-current-main-integration-review-fc809af-20260923.md`](../reviews/phase1-manual-path-current-main-integration-review-fc809af-20260923.md).
This approves the bounded pure model only; it does not claim Phase 1 or SUR-002
exit.

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
2,048 coverage intervals, 16,384 output gaps, and 3,000,000 charged work units
per `channel_gaps` call. The shared deterministic counter charges interval
validation and filtering, bounded coverage-order comparisons/moves, every
schedule/segment and coverage/segment visit, and each emitted gap before work is
performed. It fails with `Limit` before the next operation would exceed budget,
including when complete coverage would emit zero gaps. A dense regression uses
8 schedules, 2,048 same-start complete intervals, and 511 path segments; the
interval union fully covers the schedule but the call fails closed at the work
bound. Serde decoding bounds the anchor and observation sequences before
accepting an extra record and then revalidates state invariants. The
application/storage boundary must still enforce its byte/depth limit before
decoding and reconcile each observation ID with the canonical stored envelope.
This crate does not persist snapshots.

## Regression evidence

`crates/survey/tests/manual_path.rs` covers exact timestamp interpolation,
start/turn/pause/resume/stop, pause gaps, unresolved future segments, no
extrapolation, reported-pose precedence and covariance fallback, wrong and
duplicate times/IDs, mixed epochs/frames, speed/turn thresholds, edited-anchor
reprojection preserving original IDs/times/coordinates, schedule-only channel
gaps, complete versus incomplete coverage, pause-safe gaps, malformed
serialization and sequence bounds, interior interpolation near the `u64`
timestamp ceiling, and dense tied complete-coverage work-budget exhaustion. A
package unit test checks anchor and sample admission caps.

The source and focused test file content SHA-256 values at this candidate are:

- `crates/survey/src/manual_path.rs` —
  `af57f104763d2938907f3d2fe78e7b3b427d67d0bcebf7ee5bcfeb99cc813e9f`.
- `crates/survey/tests/manual_path.rs` —
  `5d61f2d72c8ca37695699c3b3d44fb1e2db08157091b86740376f3a6c6e314ff`.
- `docs/architecture/survey-state.md` —
  `360a4377a19aa9c61e9be0c46d3725ae9e5ab61cca1908830e6b980400ca35c4`.

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
passes 47 tests with one pre-existing ignored release benchmark. Strict package
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
admission, or human usability. Source and current-main integration reviews are
approved, but broader Phase 1/SUR-002 acceptance remains open.
