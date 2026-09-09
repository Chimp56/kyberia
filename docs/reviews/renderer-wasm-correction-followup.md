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

## Raster correction verification

Root independently inspected `7ea934e` and executed:

```text
npm run test:browser-canonical -- http://127.0.0.1:4173/index.html /private/tmp/kyberia-renderer-root-raster-7ea934e-authorized
```

The command passed with macOS Chromium process access. The initial sandboxed
launch failed at Mach port registration before a page opened. Desktop
1280x900, mobile 390x844 at DPR 2, and desktop OpenLayers probes pass. Both
IDW and PointValue custom raster probes preserve constant known pixels at
left/right edges; adjacent unknown samples retain null values and unknown
pixel classes. Desktop custom known pixels were [153,101,89,217] at center
and both edges; adjacent unknown pixels were [29,34,45,204]. OpenLayers
interpolation is disabled and its equivalent edge checks pass.

Root inspected desktop.png: canonical cells render as discrete rectangles
with visible unknown gaps and numeric inspection. The script reported no
console errors; it does not collect all warnings. Screenshots and malformed
input fixtures remain at the output path above. The raster finding is
corrected for this bounded evidence. The synthetic benchmark's default-source
mismatch still requires correction before integration; broader Gate B
acceptance remains open.
