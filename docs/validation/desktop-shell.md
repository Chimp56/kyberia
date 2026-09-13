# Desktop shell validation

The desktop boundary is a Tauri 2 + React/Vite shell. React receives project
state only through the versioned `kyberia.desktop-ipc/1` adapter. Blank-project
creation and current-project queries use the canonical application boundary;
long operations run in bounded `spawn_blocking` jobs with an atomic cancellation
hook and progress state. Opening a project first uses a native folder selector,
then consumes a single-use opaque grant. The renderer never receives or submits
a filesystem path.

Floor-plan decoding and calibration are still outside the current application
command boundary. Import and dropped files therefore enter an explicit
unsupported state while preserving the active project state. No network,
measurement, floor-plan, or calibration data is fabricated.

## Commands run

| Check | Result |
| --- | --- |
| `npm run typecheck` | PASS |
| `npm test -- --run` | PASS (12 tests) |
| `npm run build` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --workspace --locked --offline` | PASS (workspace, including desktop; 10 desktop boundary tests) |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | PASS |
| `python3 tools/architecture.py` | PASS |
| `python tools/source_inventory.py check` | PASS (522 locked external packages) |
| `python tools/ledger.py check` | PASS |
| `npm run e2e` | PASS (4 Chromium tests) |
| `npm run tauri:build` | PASS (`tauri build --no-bundle`; `target/release/kyberia-desktop`) |
| CSP inspection | PASS: no `unsafe-eval`/`unsafe-inline`; `object-src 'none'`, `base-uri 'self'`, and bounded `connect-src` are present |

The Browser plugin was unavailable in this environment. Playwright Chromium is
the recorded fallback. The E2E suite covers empty, loading, error, unsupported,
opaque grant open, command-palette arrows/Enter/Escape/focus restoration, and
390×760 Inspector toggle behavior. Fresh screenshots are retained at
`apps/desktop/evidence/desktop-shell-1586x960-v3.png` and
`apps/desktop/evidence/mobile-shell-390x760-v3.png`; the earlier evidence files
remain unchanged.

## Fidelity ledger

| Comparison point | Concept evidence | Render evidence | Result |
| --- | --- | --- | --- |
| Three-rail layout | 72 px tools, 250 px layers, 328 px Inspector | Root desktop grid keeps the same proportions at 1586 px | Matched |
| Empty-state copy | “Import floor plan”, drag/drop sentence, “New blank floor” | Canvas uses the same above-fold wording and both actions | Matched |
| Canvas treatment | Dark navy surface, 20/100 px grid, numeric axes | Layered CSS grid, axes, map-dominant empty card, numeric status | Matched |
| Chrome palette | Blue accent, slate surfaces, cool type, amber unavailable marker | Shared CSS tokens and text/icon unsupported state | Matched |
| Inspector anatomy | Next steps, warning section, keyboard shortcuts | Componentized steps, capability notice, nine-row shortcut list | Matched with added Zone `Q` row |
| Desktop Inspector fit | Shortcut list reaches the bottom of the concept panel | Compact rows keep Pan visible above the status bar | Corrected |
| Responsive continuation | Concept is desktop-first | At 390 px layers collapse, primary actions remain visible, Inspector opens as a drawer | Intentional responsive continuation |
| Scale semantics | Concept shows a scale bar | Empty project says “Scale unavailable” and only calibrated projects receive a bar | Intentional honesty correction |

Above-the-fold copy diff: the empty canvas keeps the accepted heading, support
sentence, Import action, divider, and New blank floor action. The shell adds
`Kyberia` product chrome, the platform-aware `⌘K`/`Ctrl K` label, and explicit
state copy for unsupported commands. The scale label changes from the concept's
illustrative `10 m` to `Scale unavailable` until the application reports a
calibrated floor. This is a deliberate semantic correction; no RF metric is
shown as measured.

## Boundary decisions

- `project_select_open` uses the OS selector adapter (`osascript` on macOS,
  PowerShell folder picker on Windows, and `zenity`/`kdialog` on Linux). Rust
  canonicalizes the selected root, rejects symlink roots, requires a `.rfatlas`
  directory, and stores the path only behind an opaque grant ID.
- `project_open_grant` consumes the grant once and checks the optional expected
  display name. Forged, reused, mismatched, traversal, and symlink selections
  are rejected before application open.
- `project_create_blank`, `project_open_grant`, and `project_current` admit one
  job at a time. Each job has a cancellation token and progress counter; query
  work crosses `spawn_blocking` before touching the application session.
- The project response carries an explicit `calibrated` flag. The current
  application projection cannot prove map calibration, so it returns `false`
  and the renderer keeps the scale unavailable.
- `apps/desktop/src-tauri` is a member of the root Cargo workspace, so root
  test, clippy, lock, architecture, inventory, and evidence commands include it.
  The former nested lock was moved to `.trash/desktop-lock-forward/` with an
  origin note; root `Cargo.lock` is canonical.
- Frontend configuration remains self-contained under `apps/desktop`; its
  `package-lock.json` pins the React/Vite and Tauri CLI graph because the root
  untracked `package.json` and pnpm store are user work outside this boundary.
- `src-tauri/gen/`, `tsconfig.tsbuildinfo`, app `dist/`, node modules, and the
  retained app test-run bin are ignored by the app-local `.gitignore`.
