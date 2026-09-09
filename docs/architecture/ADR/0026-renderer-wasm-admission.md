# ADR-0026: Rust WASM admission boundary for the research renderer

Status: **Proposed — bounded renderer correction; product renderer gate remains open**

## Context

The browser renderer consumes `kyberia.render-scene/1` bytes. JavaScript can
check structure and replay display invariants, but parsing loses distinctions
that are part of the Rust canonical wire contract, including escaped strings
and alternate numeric spellings. Accepting those bytes in the browser would
make the UI's evidence boundary weaker than the canonical adapter.

## Decision

The research renderer sends every canonical scene through the exact
`kyberia-rendering-scene` Rust validator compiled to `wasm32-unknown-unknown`
inside a short-lived module Web Worker. The worker exposes only a two-function
bounded ABI: reserve an input buffer and return a versioned admission status.
The browser's structural and numerical checks remain a diagnostic second
boundary and run only after Rust admission succeeds. Worker failure, an
unsupported runtime, cancellation, resource rejection, and malformed bytes
remain explicit states; no JavaScript or synthetic fallback is used.

## Alternatives

1. Keep the JavaScript validator as authority. Rejected: JSON parsing erases
   canonical byte distinctions, as shown by escaped `"V1"` and `8.0` fixtures.
2. Reimplement canonical serialization in JavaScript. Rejected: it duplicates
   versioned Rust semantics and would create another drift-prone authority.
3. Bind Rust directly into the product shell. Deferred: this research harness
   needs a browser boundary and does not choose the eventual desktop runtime.

## Evidence

`research/renderer/support/scene-validator-wasm` builds with the repository's
locked Rust dependencies and the checked-in fixture. Native WebAssembly
execution returns admission status `0` for the canonical fixture, `6` for the
escaped-schema spelling, and a nonzero malformed status for the `8.0` numeric
spelling. The Playwright desktop/DPR2 and OpenLayers smoke matrix exercises the
worker through the file-input path with zero page errors.

## Consequences

The browser and Rust adapter now share an executable canonical admission
authority, while the worker keeps a malformed or expensive request from
blocking the UI and can be terminated on cancellation. The checked-in WASM
artifact must be rebuilt whenever the rendering-scene contract changes. This
does not validate the final product renderer, Tauri packaging, every browser,
or a production worker pool.

## Reversibility

The boundary is isolated to the research renderer and its support crate. A
future generated binding, desktop IPC adapter, or another approved renderer
can replace the worker without changing domain or rendering-scene contracts.

## Validation plan

Run the locked offline support-crate build, direct WASM admission regression,
Node renderer tests, syntax checks, package inventory, and the retained
Playwright desktop/mobile/OpenLayers smoke matrix. Add a second browser/OS
runtime and final Tauri packaging evidence before changing this ADR to an
accepted product renderer decision.

## Reproducible build layout correction

Integration demonstrated that a standalone support workspace depending on path crates outside that workspace produced different WASM bytes under different checkout roots, even after compiler path remapping. A retained experiment copied identical source files into two differently nested roots and made the validator a member of the same workspace as its dependencies. With owned `--remap-path-prefix` compiler arguments, both builds produced 902426 bytes with SHA-256 `20660fda0de0775bda08300b5584961c417384cb19c5d65d254711bd9f78c193`. The experiment is retained under `.trash/wasm-workspace-proof/run-a397r2yr/`; it is current-host evidence, not a cross-toolchain claim.

The validator therefore becomes a research-only composition member of the primary Cargo workspace and uses its lockfile and release profile. Its dependency remains outward-to-inward: validator to rendering-scene adapter. No domain or numerical crate imports the validator. The former standalone lock is retained in the trash bin. Compiler flags map checkout and Cargo registry roots to fixed logical paths and replace inherited Rust flag overrides; no manual compiler metadata override is used. This is reversible by moving to a reproducibly staged build workspace with equivalent evidence, without changing the browser ABI or domain contract.

Validation must rebuild from distinct checkout paths, compare exact bytes, check package inventory and dependency directions, run native and WASM admission tests, and repeat the browser fixture matrix with the newly built artifact. A same-directory cached build or changing the expected digest alone does not prove reproducibility.
