# Renderer comparison harness

Serve this directory with the pinned Vite command after installing the locked
research dependencies:

```text
pnpm install --frozen-lockfile
pnpm test
pnpm run serve
```

To verify that the checked-in Rust admission artifact still reproduces from
the pinned source, toolchain, and lockfile, run:

```text
pnpm run check:wasm
```

The check builds only in the ignored support-crate `target` directory and
fails if the target/toolchain/dependencies are unavailable or the rebuilt
bytes differ. It uses the primary Cargo workspace lockfile/release profile,
pins the repository toolchain and remaps local source paths. It does not
install dependencies or replace the reviewed WASM.

With the server running, `pnpm run test:browser-canonical --
http://127.0.0.1:4173/index.html .trash/browser-<unique-run>` runs the
bounded Playwright admission and renderer smoke test. It retains desktop and
mobile screenshots in a new ignored directory for each run; never reuse or
overwrite a retained trash entry. It checks the canonical
grid bounds and invalid-file clearing, and switches through the OpenLayers
candidate. The Browser plugin is optional; the pinned local Playwright package
is used when that plugin is unavailable.

`node benchmark.mjs <url> <result.json> <screenshot.png>` is the separate Gate B
stress benchmark. It explicitly switches the page from its safe canonical
default to the synthetic fixture before asserting the 120 numeric layers,
10,000 APs, and 10,000 paths. Its result records `benchmarkSource: synthetic`
and `evidencePlane: Synthetic`; those measurements do not describe canonical
RF evidence.

The page opens with the Rust-generated canonical scene selected. Use **data
source** to switch explicitly to the synthetic Gate B stress fixture, or use
**scene file** to load another `kyberia.render-scene/1` JSON artifact. The
canonical loader validates the bounded wire contract and numerical semantics
before drawing. The numeric inspector and coordinate readout show values,
unknown reasons, masks, support, uncertainty, provenance, and frame metadata.

The synthetic source is retained for renderer comparison: it exercises 120
numeric layers, 10,000 AP overlays, 10,000 paths, bounded tile streaming, and a
six-floor custom WebGL pass. It is explicitly synthetic and must not be used as
measured RF data.

The canonical fixture is generated from the Rust adapter with the command and
provenance recorded in
[`docs/validation/renderer-canonical-scenes.md`](../../docs/validation/renderer-canonical-scenes.md).
