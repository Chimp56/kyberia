# Renderer WASM correction follow-up

Independent reviewer: root. Functional correction: `873a1c7`.
Reproducibility corrections: `a0d3c68`, `6b10011`.

The three major findings against `1af1cba` are corrected in the candidate:
production admission always invokes Rust WASM; PointValue and Nearest wire
forms and exact-cell semantics match the canonical fixtures; worker admission
has a finite deadline and worker-side fetch cancellation.

Independent retained browser evidence:
`/private/tmp/kyberia-renderer-root-873a1c7/deadline-state/method-review.json`.
At the actual file-input route, PointValue displays 3 known and 45 unknown cells;
Nearest displays 17 known and 31 unknown cells. A silent worker produces an
explicit wasm-timeout after 10059 ms and clears both scene and result state.
Four ReadPixels GPU stall warnings were recorded; this is not a zero-warning
performance acceptance result. Desktop/mobile and OpenLayers smoke evidence
is retained alongside the independent browser scripts. Broader renderer,
accessibility, memory-pressure, browser/OS and Tauri gates remain open.

The build guard now rebuilds the locked Rust support crate and compares bytes
and digest with the browser artifact. Root found that inherited Cargo output
paths could permit stale artifact comparison; `6b10011` fixes this with an
explicit owned target directory and removal of both environment overrides.
Root ran npm run check:wasm with both overrides set: PASS, 977165 bytes,
SHA-256 3aa3541c85e40b4564227b4b3101362b0b3ceb33c4c0904f996d98c4f844e888.

Integration remains pending final portability correction to the build-argument
test, source-inventory reconciliation and integrated regression. No Gate B
completion or production mapper usability is claimed.

## Additional visual finding — MAJOR, integration blocked

Root inspected the retained PointValue screenshot and traced rounded gradient
blobs to numericTexture in renderer.js: RGBA values and the unknown mask use
LINEAR filtering. This blends constant known-cell values with unknown texels
and changes cell support at the alpha threshold. Canonical numeric and mask
semantics must survive rasterization, including near cell edges. The author
is correcting sampling and adding framebuffer checks beyond cell centers;
OpenLayers sampling also requires inspection. Passing admission tests and
center probes do not close this rendering correctness finding.
