# Renderer Gate B review

Status: **resolution recorded for independent re-review; final renderer disposition remains pending**

This review records the findings raised against the bounded Phase 0 Gate B renderer proof and the corrections present in the frozen v17 harness. It accepts the evidence boundary provisionally for research use. It does not approve a product renderer, a final architecture choice, or Phase 0 completion.

## Findings and resolutions

1. **Camera-bound raster and geometry.** The custom raster quad, walls, and overlays use the same world-to-screen camera transform. Center, pan/zoom, and offscreen framebuffer probes retain camera/world/CSS/framebuffer coordinates; wall and overlay probes are required to align when in view.
2. **120-layer exercise.** Every browser row switches and renders all 120 layers, records a visible known-cell sample for each layer, and requires at least two distinct samples. The custom path retains 120 numeric textures during the sequence; OpenLayers invalidates and regenerates its selected `ImageCanvas`. This is sequential switching evidence, not a claim of simultaneous 120-layer composition.
3. **Comparable OpenLayers path.** OpenLayers `10.10.0` uses the same fixture and camera sequence, `ImageCanvas` for raster, and synchronized direct `WebGLVector` wall and overlay maps for the 324 walls, 10,000 APs, and 10,000 paths. The direct wall/AP canvases expose source-specific color probes with bottom-left WebGL readback and an impossible-magenta negative control. Numeric timing is explicitly limited: custom numeric draws walls, while OpenLayers numeric disables vector layers. Cross-candidate vector timing is therefore limited to overlays/all.
4. **3D proof.** Custom 3D uses a z-sensitive perspective look-at projection, six distinct floor colors, separate floor geometry, 180 vertices, and positive per-floor framebuffer counts. OpenLayers reports `unsupported-by-this-candidate`. The committed Node test classifies a valid per-floor color buffer and a background/no-op buffer; the latter produces zero floor counts. An independent browser draw mutation was review-only and is not represented as a committed test.
5. **Coordinate, mask, and framebuffer truth.** Tests cover scale, y-axis, DPR sizing, malformed bounds, unknown-mask `null` semantics, and canonical values. Browser rows require candidate-specific known/unknown framebuffer classes and camera alignment; OpenLayers wall/AP probes read the actual candidate canvases directly rather than a merged or brightest-pixel canvas.
6. **Provenance and supply chain.** The canonical complete fixture byte stream is SHA-256 `88f681ece75f2a6bd73d7d33f54b494fa729fa62008568c5295ea10d8cdefb71`. The final source digest map is `031aca218fda3aed83032552089cff0ad58267c15c3d2e2849d85012b0f9d1bf`; the lockfile digest is `7a1b6f9b90c7980e088707e877d968f95a5571de13336c9c5041e8bb1c82ea8c`. The 72-record package inventory has resolved versions, integrity/archive/source/license metadata, conditions, redistribution statements, and metadata hashes, and passes offline verification.
7. **Tile scheduling.** The local tile workload uses a center-first queue with a concurrency cap of three, bounded cache capacity of 48, viewport-priority records, and superseded-navigation cancellation. Tile compositing and external tile latency remain outside this Phase 0 harness and are not claimed.
8. **Measurement and units.** Fixture coordinates are millimetres; OpenLayers converts them to metres and reverses the fixture y-down axis into the local y-up projection. Timings use the normal `preserveDrawingBuffer: false` context. Raster/vector readbacks are synchronous in the same render task; custom 3D is sampled after one `requestAnimationFrame` to inspect the completed framebuffer, so that probe includes a browser/driver scheduling point and is correctness evidence rather than a GPU timer.
9. **Evidence and documentation.** Final evidence is limited to the v17 desktop DPR1 and mobile DPR2 reports, their bound workload/final screenshots, and `research/renderer/evidence/README.md`. Documented DPR2 values match the retained JSON: custom overlay/all pan `9.1/9.3` ms and frame `9.4/9.2` ms; OpenLayers overlay/all pan `366.2/360.9` ms and frame `442.2/369.2` ms. The final reports bind source, fixture, lock, browser, and screenshot hashes.

## Validation performed

The following checks were rerun after the documentation and evidence-retention corrections:

* frozen pnpm install with the pinned OpenLayers, Playwright, and Vite versions;
* 8/8 Node tests passed;
* JavaScript syntax check passed;
* offline package inventory verification passed for all 72 records with no registry access;
* SHA-256/content validation passed for both retained v17 reports, source digests, lockfile, fixture, and all retained screenshots;
* `git diff --check` passed;
* retained evidence contains exactly 21 files: README, two JSON reports, and 9 screenshots per matrix.

The v17 browser captures were not regenerated for these doc-only corrections. Their runtime source, lockfile, fixture, and screenshot bytes remain unchanged, so the source-bound hashes remain valid. Both matrices retain zero page errors and five headless Chromium readback/performance warnings per report.

## Provisional boundary and pending disposition

The bounded current-host evidence is accepted as a reproducible research artifact after the findings above were resolved. The renderer decision remains pending final independent re-review, product acceptance thresholds, a second OS/browser/driver, actual Tauri WebView/native packaging, context-loss/error recovery, accessibility, field usability, external tile latency, and a native multi-floor 3D path. No candidate is promoted to product use and no silent fallback is permitted.
