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

## Initial fetch selection race — MAJOR, open

Root reproduced this against the current renderer candidate using a real
Chromium route interception. Delay fixtures/canonical-scene-v1.json, wait for
application initialization, select the synthetic source through the selector,
then release the response. After 1500 ms the result is:

```json
{"before":"synthetic","after":{"source":"canonical","selected":"synthetic","status":"ready"}}
```

loadBundledScene starts its fetch before loadSceneBytes owns a generation and
AbortController. A newer selection invalidates an existing validator but does
not prevent the delayed initial fetch from starting a new canonical load.
The source selector and actual rendered evidence can disagree. Integration
requires generation/cancellation coverage across the whole initial fetch and
a delayed-fetch source-selection regression. This is distinct from the
corrected numeric raster sampling finding.

## Follow-up at 89e1470 — startup race remains open

Root's independent browser command failed at the delayed-fetch regression's
line 108, waiting for an explicit synthetic selection to remain ready. Log:
/private/tmp/kyberia-renderer-root-final-89e1470.log. The server returned HTTP
200. Inspection shows window.__rfatlas is exposed before boot finishes its
asynchronous fixture digest; a selection made during that interval precedes
loadBundledScene's new generation and can be overwritten. The new in-flight
fetch guard does not cover this earlier startup interval.

Requested correction covers both pre-fetch and in-flight selections, with
deterministic synchronization of the route-held test. The author-reported
benchmark pass does not resolve this independently failing lifecycle test.
Renderer integration remains pending.

## Independent runtime follow-up at 125133e

Static independent review approved the bounded boot-generation correction, but the root's authorized Chromium run still failed. Command: `npm run test:browser-canonical -- http://127.0.0.1:4173/index.html /private/tmp/kyberia-renderer-root-125133e`. The process exited 1 with a 30-second timeout in `verifyDelayedBundledFetchCannotOverrideSynthetic`, browser-canonical-scenes.mjs line 126. Log retained at `/private/tmp/kyberia-renderer-root-125133e.log`. The author is investigating the discrepancy, including served-source identity. Renderer integration remains pending runtime acceptance.

## Fresh-server independent acceptance at 125133e

The root reran the same Chromium suite against the author's fresh Vite server on port 4177: `npm run test:browser-canonical -- http://127.0.0.1:4177/index.html /private/tmp/kyberia-renderer-root-125133e-4177`. It exited 0; both deterministic startup-selection cases, desktop/mobile canonical raster probes, malformed-input handling and OpenLayers probes passed, with `errors: []`. Log: `/private/tmp/kyberia-renderer-root-125133e-4177.log`.

Comparing the served renderer modules on ports 4173 and 4177 found identical application statements; differences were Vite timestamp/dependency query parameters and sourcemaps. Both included the new boot-generation guard. Therefore the earlier timeout cannot be attributed to a stale application source without further evidence. The fresh-server result supports the bounded correction; the retained older-server failure remains an environment/reliability follow-up. An independent full workload benchmark is running separately; no final renderer-choice or whole-product UX gate is closed.

## Independent full synthetic workload benchmark

At candidate 125133e, the root ran `node benchmark.mjs http://127.0.0.1:4177/index.html /private/tmp/kyberia-renderer-root-benchmark-125133e/proof.json /private/tmp/kyberia-renderer-root-benchmark-125133e/proof.png`. Exit 0: all eight custom/OpenLayers numeric, overlay, combined and 3D workloads reached ready, with zero page errors and five preserved console messages. This includes the harness's layer-switch, framebuffer-mask, camera-alignment and resize assertions. [Source-bound summary](../validation/renderer-independent-125133e.json) retains the full local report hash and source/browser/screenshot bindings. This is current-host synthetic proof, not a final renderer choice or a usable application acceptance claim.
