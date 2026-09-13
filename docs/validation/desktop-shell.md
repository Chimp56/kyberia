# Desktop shell validation

The desktop boundary is a Tauri 2 + React/Vite shell. React receives project
state only through the versioned `kyberia.desktop-ipc/1` adapter. Blank-project
creation and current-project queries use the canonical application boundary;
long operations run in bounded `spawn_blocking` jobs. The renderer supplies a
validated UUID before invocation, polls strict progress responses, and can invoke
Cancel while the original command is pending. Opening a project first uses a native folder selector,
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
| `npm test -- --run` | PASS (14 tests) |
| `npm run build` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --workspace --locked --offline` | PASS (workspace, including desktop; refreshed after corrections) |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | PASS |
| `python3 tools/architecture.py` | PASS |
| `python tools/source_inventory.py check` | PASS (522 locked external packages) |
| `python tools/ledger.py check` | PASS |
| `npm run e2e` | PASS (10 Chromium tests) |
| `npm run tauri:build` | PASS (`tauri build --no-bundle`; `target/release/kyberia-desktop`) |
| `KYBERIA_TOOL_PYTHON=/Users/vincent/code/kyberia/.tools/venv/bin/python python3 tools/dev.py desktop` | PASS (the complete root desktop gate, including Playwright and native build) |
| Native binary launch | PASS: release process remained live until the controlled interrupt; host process inspection was sandbox-denied, so this is a launch/liveness smoke rather than visual native automation |
| CSP inspection | PASS: no `unsafe-eval`/`unsafe-inline`; `object-src 'none'`, `base-uri 'self'`, and bounded `connect-src` are present |

Browser plugin not available. Playwright Chromium is
the recorded fallback. The E2E suite covers empty, loading, error, unsupported,
opaque grant open, command-palette arrows/Enter/Escape/focus restoration,
strict combobox/listbox semantics, progress/cancellation during create and the
native picker, operation serialization, operation-specific create/open retry,
non-retryable recovery, distinct-identity replacement, and 390×760 Inspector
toggle behavior. Fresh screenshots are retained at
`apps/desktop/evidence/desktop-shell-1586x960-v3.png` and
`apps/desktop/evidence/mobile-shell-390x760-v3.png`; the earlier evidence files
remain unchanged.

## Fidelity ledger

| Comparison point | Concept evidence | Render evidence | Result |
| --- | --- | --- | --- |
| Three-rail layout | 72 px tools, 250 px layers, 328 px Inspector | Root desktop grid keeps the same proportions at 1586 px | Matched |
| Empty-state copy | “Import floor plan”, drag/drop sentence, “New blank floor” | Canvas preserves import wording but says “New project” because the command creates a project, not a floor | Intentional honesty correction |
| Canvas treatment | Dark navy surface, 20/100 px grid, numeric axes | Layered CSS grid, axes, map-dominant empty card, numeric status | Matched |
| Chrome palette | Blue accent, slate surfaces, cool type, amber unavailable marker | Shared CSS tokens and text/icon unsupported state | Matched |
| Inspector anatomy | Next steps, warning section, keyboard shortcuts | Componentized steps, capability notice, nine-row shortcut list | Matched with added Zone `Q` row |
| Desktop Inspector fit | Shortcut list reaches the bottom of the concept panel | Compact rows keep Pan visible above the status bar | Corrected |
| Responsive continuation | Concept is desktop-first | At 390 px layers collapse, primary actions remain visible, Inspector opens as a drawer | Intentional responsive continuation |
| Scale semantics | Concept shows a scale bar | Empty project says “Scale unavailable” and only calibrated projects receive a bar | Intentional honesty correction |

Above-the-fold copy diff: the empty canvas keeps the accepted heading, support
sentence, Import action, and divider. The former New blank floor action is now
truthfully labeled New project and confirms before replacing an active session. The shell adds
`Kyberia` product chrome, the platform-aware `⌘K`/`Ctrl K` label, and explicit
state copy for unsupported commands. The scale label changes from the concept's
illustrative `10 m` to `Scale unavailable` until the application reports a
calibrated floor. This is a deliberate semantic correction; no RF metric is
shown as measured.

## Boundary decisions

- `project_select_open` uses the OS selector adapter (`/usr/bin/osascript` on
  macOS, an absolute Windows PowerShell path, and `/usr/bin/zenity` or
  `/usr/bin/kdialog` on Linux). Rust
  canonicalizes the selected root, rejects symlink roots, requires a `.rfatlas`
  directory, and stores the path only behind an opaque grant ID.
- `project_open_grant` consumes the grant once and checks the optional expected
  display name. Forged, reused, mismatched, traversal, and symlink selections
  are rejected before application open.
- `project_select_open`, `project_create_blank`, `project_open_grant`, and
  `project_current` admit one caller-identified job at a time. The renderer
  reserves the whole create/open operation before the native picker and keeps
  that picker job visible and cancellable. Job IDs are canonical UUIDs;
  progress and cancellation responses are strictly validated by the renderer.
  Join failure always releases admission, cancellation never swaps the active
  session, and an open grant is not consumed when job admission fails.
- Current-project queries retain a shared session owner outside the blocking
  worker. The worker holds its mutex only for an immutable query; if that query
  unwinds, the narrowly scoped poison recovery preserves the identical session
  and project while unconditional cleanup releases the job admission slot.
- Application create/open are short atomic storage calls without an internal
  cancellation seam. The desktop checks cancellation immediately before and
  after that call and again before response publication/session swap. A cancel
  arriving during create drops the session and moves the resulting bundle to
  the owning `.trash` recovery bin before reporting `cancelled`; cleanup failure
  is reported as a retryable storage error. Its latency is bounded by that
  atomic call.
- Native open grants expire after five minutes and the in-memory grant table is
  capped at eight entries. Valid grants remain single-use after successful job
  admission.
- The project response carries an explicit `calibrated` flag. The current
  application projection cannot prove map calibration, so it returns `false`
  and the renderer keeps the scale unavailable.
- `apps/desktop/src-tauri` is a member of the root Cargo workspace. The root
  `desktop`/CI gate now runs frontend unit/type/build, Playwright Chromium,
  desktop Rust test/clippy, `tauri build --no-bundle`, output existence,
  architecture, inventory, and ledger checks.
  The former nested lock was moved to `.trash/desktop-lock-forward/` with an
  origin note; root `Cargo.lock` is canonical.
- Frontend configuration remains self-contained under `apps/desktop`; its
  exact direct dependency versions and `package-lock.json` pin the React/Vite
  and Tauri CLI graph because the root
  untracked `package.json` and pnpm store are user work outside this boundary.
- `src-tauri/gen/`, `tsconfig.tsbuildinfo`, app `dist/`, node modules, and the
  retained app test-run bin are ignored by the app-local `.gitignore`.
