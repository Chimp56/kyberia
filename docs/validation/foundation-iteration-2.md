# Foundation iteration 2 validation

Canonical project/calibration implementation: `364ff2d`, integrating reviewed author commit `e93927a`. Independent review: [project-calibration-review.md](../reviews/project-calibration-review.md), integrated in `1cf2395`.

On macOS 26.6.2 ARM64 with Rust/Cargo 1.98.1:

- `cargo test --workspace --locked --offline`: PASS, 32 domain tests (18 evidence/value, 14 project/calibration), 13 storage tests, two executable CLI workflows, seven compile-fail doctests.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: PASS.

The reviewer independently exercised malformed hierarchy snapshots, logical ordering, receipt forgery and 56 oblique calibration cases across both image handedness conventions and large finite angles. A large-angle subtraction defect was corrected by composing normalized direction vectors. Unknown distance/control uncertainty remains unknown, independently of exact geometric round trips.

These are contract and numerical-coordinate tests. The complete frame graph, map import/GUI, full undo/merge semantics, survey persistence and large-history performance remain open. The release metadata-operation benchmark and its observed quadratic replay growth are documented in the domain architecture guide; no GUI latency or field calibration accuracy is inferred.
