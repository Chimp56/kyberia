# ADR-0008: Gate B renderer comparison and bounded GPU boundary

Status: **Accepted — bounded provisional evidence independently reviewed; final renderer disposition pending**

## Context

RF Atlas needs to display large floor-plan raster/vector content, 100+ numerical layers with an honest unknown mask, 10k AP/path overlays, bounded tile streaming, and multi-floor views. Phase 0 Gate B requires an evidence comparison between a mature web mapping stack and a custom GPU path using identical original workloads. Static mocks and API preference cannot establish browser latency, memory, coordinate correctness, cache cancellation, or GPU support.

The research harness in `research/renderer/` creates fixture `renderer-gate-b-v1` with seed `0x6d2b79f5`, full canonical typed-array bytes, local millimetre/y-down coordinates, a deterministic camera sequence, and a bounded local tile stream. Both candidates run the same four workload selections and switch through all 120 numerical layers. The browser runner records actual `performance.now()` and `requestAnimationFrame` timings, memory snapshots, CSS/framebuffer/DPR values, WebGL/vendor/timer-query availability, cache behavior, camera-bound raster/overlay/wall probes, numeric mask probes, 3D support, and screenshots.

## Alternatives

1. **OpenLayers 10.10.0.** A mature mapping stack with map/view/layer/source/projection abstractions. The harness uses `ImageCanvas` for the shared numerical raster and two synchronized `WebGLVector` maps for the same wall and AP/path sources. Fixture millimetres are converted to metres and y-down coordinates to the local projection's y-up coordinates. Separate actual WebGLVector canvases make source-specific framebuffer checks possible without merged-canvas or brightest-pixel ambiguity; their measured overhead is included in this harness run. The six-floor state is explicitly unsupported by this bounded 2D candidate. Numeric rows intentionally disable its vector layers, so they do not match the custom candidate's numeric-plus-wall composition.
2. **MapLibre GL JS plus a custom layer.** A documented mature WebGL camera/vector-tile stack with an explicit custom-layer contract and 2D/3D modes. It remains a documented alternative requiring its own pinned, same-fixture browser run.
3. **RF Atlas-owned custom WebGL2.** A small path owning numeric texture/mask composition, camera-bound geometry, batched overlays, bounded cache behavior, and a visibly probed six-floor extrusion. It carries the largest maintenance, accessibility, hit-testing, context-loss, and cross-driver burden.
4. **Canvas/SVG-only rendering.** Useful for diagnostics, but excluded as a Gate B promotion candidate for the large overlay and numerical-layer workloads.

The OpenLayers and custom paths have unavoidable implementation differences, recorded in each row: OpenLayers' map/view and WebGLVector internals versus custom WebGL2 programs and buffers. The custom numeric row draws the 324 walls along with the raster; OpenLayers numeric disables wall and AP/path vectors. Numeric timing is consequently a within-candidate diagnostic, while overlays/all are the cross-candidate vector-performance rows. Equality is enforced at the fixture, camera, layer count, overlay count, tile request count, resize/DPR matrix, framebuffer truth probes, and screenshot binding levels. The custom path has no hidden fallback; missing WebGL2 is an explicit unsupported state.

## Evidence

The source-bound captures are `research/renderer/evidence/browser-proof-desktop-v17.json` with its candidate/workload PNGs (headless Chrome `148.0.7778.96`, `MacIntel`, CSS `1440 × 900`, DPR 1) and `browser-proof-mobile-v17.json` with its PNGs (CSS `390 × 844`, DPR 2). Both reports cover both candidates across numeric, overlays, all, and 3D selections, candidate-specific resize probes, 120 rendered layer IDs per row, 10k AP/path overlays, 96 local tile requests, camera-bound framebuffer probes, and explicit support states.

The fixture's full canonical byte stream is 29,857,554 bytes with SHA-256 `88f681ece75f2a6bd73d7d33f54b494fa729fa62008568c5295ea10d8cdefb71`. The final report source SHA-256 is `031aca218fda3aed83032552089cff0ad58267c15c3d2e2849d85012b0f9d1bf`; the lock SHA-256 is `7a1b6f9b90c7980e088707e877d968f95a5571de13336c9c5041e8bb1c82ea8c`. Each report also stores browser identity and screenshot hashes. Both have zero page errors and five retained readback/performance warnings. The custom context records `preserveDrawingBuffer: false`; raster/vector probes read in the same render task, while the custom 3D probe is sampled after one `requestAnimationFrame` to inspect the completed framebuffer. That one-frame 3D readback has a browser/driver scheduling limitation and is not treated as a GPU-timed measurement.

Representative desktop values in milliseconds are:

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

At DPR 2, custom overlay/all pan p95 is `9.1/9.3` and frame p95 is `9.4/9.2`; OpenLayers overlay/all pan p95 is `366.2/360.9` and frame p95 is `442.2/369.2`. These are diagnostics for this headless macOS host, not product SLOs. OpenLayers overlay/all rows expose separate wall and overlay WebGLVector canvases; numeric and 3D rows keep those vector layers disabled while retaining synchronized map contexts and report the explicit unsupported 3D state. Numeric timings are not a cross-candidate vector comparison because the custom numeric row includes walls.

The framebuffer probes bind raster, walls, and overlays to each camera at center, pan/zoom, and offscreen positions. Known and unknown classes pass in both matrices; offscreen probes are transparent and marked out of viewport. The OpenLayers candidate records separate direct WebGLVector wall/AP canvases, expected source colors, bottom-left readback, source counts, and a negative magenta control. Each layer exercise also records a visible, distinct known-cell framebuffer sample while switching all 120 layers; OpenLayers invalidates its `ImageCanvas` source on each layer change. Custom 3D uses a perspective look-at matrix, six distinct floors, 180 geometry vertices, six positive per-floor full-frame color counts, and `visible: true`; its Node test classifies a valid per-floor color buffer and a background/no-op buffer, with the latter failing the floor assertion. An independent browser draw mutation was review-only. Tile stress remains bounded at 48 entries and reports a center-first scheduler capped at three concurrent requests plus superseded-navigation cancellation. Tile compositing remains outside this Phase 0 harness. The source ledger enumerates all 72 lockfile records with license, source, repository, archive URL, integrity, platform conditions, redistribution statement, and metadata hash.

The independent review resolved the bounded evidence findings and accepts this research artifact provisionally. These results do not authorize a final renderer selection or product promotion. The measured OpenLayers overlay cost and explicit 3D gap are architecture inputs that need product thresholds and broader host review.

## Provisional boundary

Keep the renderer decision open. If a commodity stack meets eventual product p95, memory, interaction, coordinate/mask, and 2D requirements after broader host/browser review, prefer its mature primitives and add measured custom layers only for demonstrated gaps. If it fails a required workload that the custom path passes, record the exact workload, runtime provenance, maintenance cost, and reversibility before selecting custom WebGL2.

The custom path remains research-only. It must not become a product renderer, canonical numerical engine, or silent fallback. A candidate without WebGL2 or native 3D must continue to show an explicit unsupported state.

## Consequences

* OpenLayers supplies mature camera, projection, vector, and interaction primitives. This proof exposes its actual WebGLVector path and the current-host cost of the large overlay/all workload, while retaining its explicit 3D limitation.
* Custom WebGL2 supplies direct control over masks, camera-bound geometry, batching, and a limited extrusion proof, while leaving product-grade accessibility, hit testing, text, context loss, packaging, and cross-driver behavior open.
* A shared fixture, full SHA-256 binding, retained screenshots, and complete lock/license inventory make the comparison reproducible and reversible. They do not prove real survey-data throughput, network tile behavior, Tauri WebView behavior, or field usability.
* No external tiles, copied vendor assets, real captures, or browser binaries are committed or redistributed. The synthetic tile stream measures bounded cancellation and cache behavior only.

## Reversibility and validation

Candidates are isolated behind `BaseRenderer` and selected at runtime. Replacing OpenLayers, adding a measured MapLibre candidate, or removing this harness does not alter domain crates or project data. The fixture seed and schema are versioned; workload changes require a new fixture ID and fresh evidence.

Completed validation:

1. Frozen install of OpenLayers `10.10.0`, Playwright `1.60.0`, and Vite `8.1.5` with a complete pnpm integrity lockfile.
2. Eight deterministic Node tests for generation, coordinate scale/y-axis, DPR and malformed bounds, full SHA-256 bytes, unknown null semantics, cache cancellation/eviction, viewport priority/superseded navigation, and a custom-3D per-floor color-classifier check whose background/no-op input produces zero floor counts.
3. Desktop and DPR2 Playwright Chromium matrices for both candidates and all four workloads, with candidate-specific framebuffer truth, cache/memory/timing/provenance, resize probes, and retained screenshots. Both matrices have zero page errors.
4. Complete 72-record source/license inventory verified offline against the lockfile and retained package metadata. An explicit `--refresh` mode performs registry metadata refresh when network access is intended.

Open validation:

1. Primary-branch integration freeze with source-ledger and traceability binding.
2. A second browser/OS/driver and actual Tauri WebView/native packaging capture.
3. Product acceptance thresholds, context-loss/error recovery, accessibility, and field usability.
4. A separate measured decision for external tile latency and a native 3D/multi-floor path.

This ADR accepts the bounded provisional evidence after independent review. Final renderer disposition remains pending the open host, packaging, product-threshold, and native 3D gates above; no renderer choice or product promotion is made here.
