# Gate B renderer proof

Status: **Bounded provisional evidence accepted after independent review; final renderer disposition remains pending.** This is a bounded research harness, not product UI, a real RF measurement path, or Phase 0 completion.

## Scope and candidates

One deterministic synthetic fixture, camera sequence, coordinate frame, and local tile workload are sent through both candidates:

* `custom-webgl2` is the small RF Atlas-owned WebGL2 path. It binds raster geometry to the camera/world transform, renders the numeric unknown mask, draws the 324 walls in every numeric/all frame and 10,000 AP/path overlays in overlay/all frames, exercises all 120 numerical layers, runs a bounded tile cache, and visibly probes six custom floor extrusions.
* `openlayers-10.10.0` is the pinned mature mapping candidate. It uses OpenLayers `ImageCanvas` for the shared numerical raster and two synchronized `WebGLVector` maps for the same walls and AP/path sources. Its local projection uses metres (`fixture millimetres / 1000`) and converts the fixture's y-down coordinates to OpenLayers' y-up coordinates. The wall and overlay probes read their own actual WebGLVector canvases directly; no merged canvas or brightest-pixel heuristic is used. Numeric-only and 3D rows keep those vector layers disabled and report 3D as unsupported.

The candidates receive equal fixture data and workload counts, but the numeric rows do not have equal vector composition: the custom path draws walls with its numeric raster, while OpenLayers numeric intentionally disables the wall and AP/path WebGLVector layers. Numeric timing is therefore a within-candidate diagnostic and must not be read as a cross-candidate vector-performance comparison; the overlays/all rows carry that comparison. Their drawing architecture remains explicit: OpenLayers owns its map/view/projection and WebGLVector batching, while the custom path owns its WebGL2 programs and buffers. The harness does not present either implementation as product-ready. OpenLayers six-floor 3D is reported as unsupported; custom 3D is labelled `custom-floor-extrusion-proof-only` after a framebuffer pixel probe.

Dependencies are pinned in `research/renderer/package.json` and `research/renderer/pnpm-lock.yaml`. The browser runner uses Playwright `1.60.0`, Vite `8.1.5`, and OpenLayers `10.10.0`. The Browser plugin was unavailable, so the retained captures use local Playwright Chromium after recording that fallback. No external map, tile, vendor, or real-capture asset is contacted.

## Fixture and truth checks

`research/renderer/fixture.js` is the only data generator. It uses seed `0x6d2b79f5`, fixture ID `renderer-gate-b-v1`, generator `rfatlas-renderer-synthetic-v1`, and a local floor-plan coordinate frame in millimetres with width `4096`, height `3072`, origin `[0, 0]`, and explicit y-down direction.

The fixture contains 120 numerical `256 × 192` layers, each with values and a mask (`0` unknown, `1` observed, `2` interpolated); unknown storage is `NaN` and the public sample API returns `null`. It also contains 10,000 AP points, 10,000 path segments, 324 walls, six floor records, and 96 local synthetic tile requests per benchmark. The cache capacity is 48 and records hits, misses, evictions, ordinary aborts, viewport priority, and superseded-navigation cancellations.

`canonicalFixtureBytes(fixture)` encodes the complete fixture metadata, every layer value and mask, and every overlay/path float in a deterministic byte order. The retained canonical SHA-256 is `88f681ece75f2a6bd73d7d33f54b494fa729fa62008568c5295ea10d8cdefb71` over `29,857,554` bytes. Browser results retain known/unknown framebuffer probe coordinates, camera/world/screen mappings, CSS and framebuffer coordinates, visibility, pixel classes, and expected semantics. The three camera states include a centered view, a pan/zoom view, and an intentionally offscreen view; offscreen probes are recorded as transparent rather than treated as failures.

The Node tests cover deterministic generation, world/screen scale and y-axis behavior, DPR framebuffer sizing and malformed inputs, full-fixture SHA-256 stability, unknown-mask null semantics, bounded cache cancellation/eviction, center-first viewport requests with superseded-navigation cancellation, and a custom-3D per-floor color-classifier check whose background/no-op input produces zero floor counts. All 8 tests pass.

## Browser protocol and evidence

Each browser context runs both candidates across `numeric`, `overlays`, `all`, and `3d`. Every row first switches through all 120 layers and records the rendered IDs, then records load, 40 pan/zoom timings, 50 `requestAnimationFrame` timings, local tile/cache counters, JS memory API snapshots, WebGL/vendor/timer-query availability, camera-bound raster/overlay/wall probes, numeric known/unknown framebuffer classes, and explicit 3D support. The runner captures one screenshot per candidate/workload and a final screenshot. Resize probes change and restore the viewport for both candidates.

The final source-bound captures are:

* `research/renderer/evidence/browser-proof-desktop-v17.json` and its per-candidate/workload PNGs: headless Chrome `148.0.7778.96`, `MacIntel`, CSS viewport `1440 × 900`, DPR 1.
* `research/renderer/evidence/browser-proof-mobile-v17.json` and its per-candidate/workload PNGs: the same browser/runtime, CSS viewport `390 × 844`, DPR 2.

Both reports have zero page errors and five retained browser warnings: four Chromium `ReadPixels` stall diagnostics and one Canvas2D readback advisory. They are performance observations from the probe mechanism, not application errors. WebGL reports `WebKit WebGL`/`WebKit`; timer queries are unavailable, so no GPU timer claim is made. The JS memory API is available, but values are process-level snapshots rather than clean per-candidate allocation deltas. The custom context is created with `preserveDrawingBuffer: false`. Raster/vector probes read synchronously in the same render task; the custom 3D probe is sampled after one `requestAnimationFrame` so the completed browser framebuffer can be inspected. That one-frame 3D readback includes a browser/driver scheduling point and is correctness evidence, not a GPU-timed metric. No preserved-buffer context is used.

Each JSON binds its retained rows and screenshots to the fixture SHA-256, a SHA-256 over the source digest map, individual source digests (including `package-inventory.mjs`), the lockfile SHA-256, browser identity SHA-256, and per-file screenshot SHA-256 values. The desktop and mobile fixture and lock hashes match; browser hashes differ because the viewport/DPR is part of browser identity.

Representative desktop p95 values in milliseconds from `browser-proof-desktop-v17.json`:

| candidate/workload | pan/zoom p95 | frame p95 | load | 3D state |
| --- | ---: | ---: | ---: | --- |
| custom / numeric | 9.1 | 9.3 | 8.3 | custom extrusion proof only |
| custom / overlays | 9.0 | 8.8 | 6.6 | custom extrusion proof only |
| custom / all | 9.9 | 8.7 | 9.3 | custom extrusion proof only |
| custom / 3d | 9.2 | 9.1 | 9.2 | custom extrusion proof only |
| OpenLayers / numeric | 9.2 | 9.0 | 7.6 | unsupported by this candidate |
| OpenLayers / overlays | 367.3 | 361.1 | 361.1 | unsupported by this candidate |
| OpenLayers / all | 363.8 | 398.0 | 359.7 | unsupported by this candidate |
| OpenLayers / 3d | 9.2 | 9.2 | 8.4 | unsupported by this candidate |

At DPR 2, custom overlay/all pan p95 is `9.1/9.3` and frame p95 is `9.4/9.2` ms; OpenLayers overlay/all pan p95 is `366.2/360.9` and frame p95 is `442.2/369.2` ms. These are current-host diagnostics, not product SLOs or evidence for a final renderer choice. The OpenLayers overlay/all rows report separate wall and overlay WebGLVector canvases; numeric and 3D rows leave those vector layers disabled while retaining the map contexts for synchronized view state, and report the explicit unsupported 3D state.

Every final row reports `layerExercise.rendered.length === 120`, with a visible known-cell framebuffer sample recorded for every switched layer and multiple distinct sample values across the sequence. In the custom candidate this also leaves 120 numeric textures in its bounded renderer cache; OpenLayers explicitly invalidates and regenerates the selected shared `ImageCanvas` layer while the same 120 layer IDs are switched and rendered. This is an exercised sequential layer workload, not a claim that 120 layers are simultaneously composited by both candidates.

The tile stream retains center-first priorities such as `3/4/4`, then neighboring tiles. The scheduler launches at most three requests, records the priority order, and aborts superseded navigation. Final stress counters show capacity bounded at 48 entries and viewport cancellations (four per representative row, with completed and cancelled requests visible in every row). This is a tile-data request/cache workload only; tile compositing remains outside Phase 0 and the local synthetic stream does not claim network tile behavior.

Screenshots show populated raster/vector output for both candidates, the custom all workload with dense walls and overlays, the OpenLayers WebGLVector output, and visible custom floor extrusions. The custom 3D draw uses a perspective look-at matrix, six distinct floor colors, 180 vertices, and full-frame per-floor color counts. The retained Node negative test passes a background/no-op framebuffer through the same per-floor color classifier and confirms that no floor pixels fail the assertion; an independent browser draw mutation was used only during review. OpenLayers has no native 3D path in this harness and reports unsupported. The coordinate readout is `center world=(2048.00, 1536.00) mm · layer=metric-001 · cell=(128,96) · value=-40.83 dBm mask=1`. Known and unknown samples pass candidate-specific framebuffer checks in both matrices. Loading, ready, unsupported, and error states are represented; missing package or WebGL2 context is fail-closed and does not silently switch candidates.

## Reproduction

From the renderer worktree, using the repository-pinned executables:

```text
cd research/renderer
<pinned-node> <pinned-pnpm> install --store-dir .tools/pnpm-store --frozen-lockfile
<pinned-node> <pinned-pnpm> test
<pinned-node> <pinned-pnpm> run check
<pinned-node> <pinned-pnpm> run serve
PLAYWRIGHT_BROWSERS_PATH=.tools/ms-playwright <pinned-node> benchmark.mjs \
  http://127.0.0.1:4173/index.html \
  evidence/browser-proof-desktop.json evidence/browser-proof-desktop.png
RFATLAS_VIEWPORT=390x844 RFATLAS_DPR=2 \
  PLAYWRIGHT_BROWSERS_PATH=.tools/ms-playwright <pinned-node> benchmark.mjs \
  http://127.0.0.1:4173/index.html \
  evidence/browser-proof-mobile.json evidence/browser-proof-mobile.png
<pinned-node> package-inventory.mjs
```

By default, `package-inventory.mjs` performs an offline verification of the retained 72-record inventory against `pnpm-lock.yaml`, including exact package versions, integrity, archive/source/license metadata, and metadata hashes. `package-inventory.mjs --refresh` is the explicit registry-refresh command; it is not required for offline verification. The package inventory and generated browser artifacts remain under the owned research path; dependency stores and browser payloads are ignored local tooling.

## Validation boundary

The evidence supports a reproducible current-host comparison of the two bounded 2D paths, numeric mask and coordinate probes, custom 3D visibility, layer switching, cache cancellation, and DPR/resize behavior. It does not cover Tauri WebView/native packaging, a second OS/driver, real RF data, external tile latency, context loss, accessibility, field usability, or product acceptance thresholds. OpenLayers' measured large-overlay cost and lack of native 3D remain architecture inputs. The ADR accepts this bounded evidence provisionally; final renderer disposition remains pending those open gates and product thresholds.

## Primary research sources

`docs/licenses/renderer-research-sources.json` records the primary OpenLayers, MapLibre custom-layer, WebGL2, browser timing, Vite, and Playwright sources plus a complete 72-record resolved npm package inventory. No external source code, map tiles, vendor assets, or browser binaries are committed or redistributed by this harness.
