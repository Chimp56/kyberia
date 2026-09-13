# Desktop shell validation

The first desktop boundary is a Tauri 2 + React/Vite shell. The UI reads and
mutates projects only through the versioned `kyberia.desktop-ipc/1` adapter.
Blank-project creation calls `kyberia_application::Application::create`; the
current application boundary does not yet expose `ImportFloorPlan`, so import
is an explicit unsupported state and never pretends to store a selected file.

## Commands run

| Check | Result |
| --- | --- |
| `npm run typecheck` | PASS |
| `npm run build` | PASS |
| `npm test -- --run` | PASS (4 tests) |
| `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --offline` | PASS |
| `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --offline` | PASS (4 tests) |
| `npm run e2e` | PASS with Playwright Chromium fallback |

The Browser plugin was unavailable in this environment, so Playwright
Chromium is the recorded browser verification fallback. Its retained output is
under `apps/desktop/evidence/` and `.trash/test-runs/playwright-artifacts/`.

## Fidelity ledger

| Comparison point | Concept evidence | Render evidence | Result |
| --- | --- | --- | --- |
| Three-rail layout | 72 px tool rail, 250 px layers, 328 px Inspector | CSS grid preserves those widths at 1586 px | Matched |
| Empty-state copy | “Import floor plan”, “New blank floor”, supporting sentence | Canvas and Inspector use the approved wording | Matched |
| Canvas treatment | Dark navy grid with 20/100 px lines and numeric axes | CSS layered grid, axes, scale bar, and 100% control | Matched |
| Chrome palette | Blue accent, slate surfaces, cool white type, amber unavailable marker | Tokenized palette and explicit status icon/text | Matched |
| Inspector anatomy | Next steps, warning section, keyboard shortcuts | Componentized steps, capability notice, shortcut rows | Matched |
| Responsive behavior | Desktop surface is dense and map dominant | At 390 px layers collapse and Inspector becomes a bottom sheet | Intentional responsive continuation |

Above-the-fold copy diff: the visible empty state matches the accepted concept.
The shell adds product name `RF Atlas` in the brand bar and an honest
“Desktop command required” heading only after an unsupported command is
attempted; both are product chrome/state copy, not replacement empty-state
copy. No networks, RSSI, throughput, or other synthetic measurements are
rendered.

The accepted concept at
`docs/design/desktop-empty-project-concept.png` and the latest Chromium
screenshots were inspected with `view_image` before handoff.
