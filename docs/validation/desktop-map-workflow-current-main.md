# Current-main desktop PNG map workflow candidate

This packet describes a manual, isolated adaptation from base
`bc51e80b14e30f927628f4ba9f2e92a4773423fe` on
`feat/phase0-desktop-map-current`. It is not integrated and has not received
independent approval. Its first independent source review requested changes:
the MAJOR committed-readback recovery and MINOR retry-frontier findings are
addressed in this author follow-up, which still requires fresh independent
rereview. It does not close Phase 0/1 or MAP-002/MAP-003/MAPB-001/MAPB-002.

## Plan scope

- §5.3 floor-plan ingestion and coordinate calibration: bounded PNG path and
  numeric two-point calibration only.
- §6.2 MAP-002 and MAP-003: initial floor baseline, operation-derived
  causality, durable mutation/readback semantics, opaque file selection and
  metadata-only map projection.
- Phase 0 §17: contributes only a first project/map/calibrated-coordinate
  persistence slice. Two-platform deterministic fixture, live capability
  matrix and raw export without UI are still unproven and remain exit gates.
- §18.4 MAPB-001/MAPB-002: focused admission and scale-calibration path only.
- §15 security, resource budgets and testing gates: no-follow native file open,
  one-shot project-bound grants, bounded/cancellable staging and retained
  retry TTL/count limits. Fixtures are synthetic or mocked.

## Design evidence

- Fresh desktop projects have a persisted revision-zero site/building/floor;
  their projection has `floorId`, no map and `calibrated: false`.
- Application map intents accept operation/actor/device identity, but derive
  causal parents, Lamport/logical value, causal depth and revision from the
  canonical operation set. Exact import retries verify the existing immutable
  operation and artifact before deriving a new frontier; an application
  regression reopens and retries with nine current DAG heads.
- The strict current-main `map_asset.rs` parser is not replaced. Import stays
  container-only: PNG signature, chunk order/length/CRC and bounded metadata
  are checked; IDAT is not inflated and pixels are neither returned nor
  rendered.
- A durable receipt is authoritative after append. Canonical current-view
  readback is separate and can be reported as unavailable without describing
  the mutation as rolled back. The renderer retains the full committed receipt
  and originating project ID, enters an explicit stale/error state, disables
  stale map actions, and exposes a query action. Recovery clears only when a
  query returns that same project at or beyond the committed revision; a
  lagging query keeps recovery actionable. Query retry never repeats import or
  calibration.
- The native selector returns an opaque, expiring, one-shot grant bound to the
  project and floor active at selection. A regular file is opened without
  following Unix symlinks or Windows reparse points; staging checks
  cancellation in bounded 64 KiB reads and caps source bytes at 32 MiB.
- Selection responses and project/map projections contain no source path or
  raster bytes/pixels. Calibration controls are numeric, finite, distinct and
  strictly within admitted image bounds; a calibrated scale is projected only
  from canonical persisted calibration state.
- Renderer project/map operations are single-flight, use the same visible
  status/cancel route, preserve exact retry payloads, and discard stale
  completions. Cancellation or terminal selection failure requires a new
  native selection. A mocked desktop E2E regression models lagging and then
  current project queries after one committed import and asserts import is
  invoked once.

## Checks performed

- `cargo test --locked --offline --quiet -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application -- --test-threads=1` — PASS, all selected unit/integration/doc tests; existing explicitly ignored tests remained ignored.
- `cargo test --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --quiet` — PASS after the final Rust test edits: 23 desktop library tests, 6 binary tests and 2 IPC boundary tests. Tests include fresh-floor projection, opaque one-shot project-bound grants, Unix symlink rejection, grant/retry bounds and TTL, bounded/cancellable staging, live map-picker status/cancellation, and committed-receipt preservation when the separate readback reports an error.
- `cargo clippy --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application --all-targets -- -D warnings` — PASS.
- `cargo clippy --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings` — PASS.
- `npm --cache ../../.trash/test-runs/npm-cache run typecheck` (from
  `apps/desktop`) — PASS on the readback-recovery follow-up.
- `npm --cache ../../.trash/test-runs/npm-cache run test -- --reporter=dot`
  (from `apps/desktop`) — PASS, 18/18 Vitest tests across 3 files, including
  stale-view recovery and committed-receipt state assertions.
- Focused exact-retry regression:
  `cargo test --locked --offline -p kyberia-application --test project_session intent_workflow_creates_floor_derives_causality_and_retries_after_reopen -- --exact --nocapture` — PASS with nine concurrent heads.
- `npm --cache ../../.trash/test-runs/npm-cache run e2e -- --config
  .trash/test-runs/isolated-playwright.config.ts --grep 'committed map receipt
  exposes a query recovery path without repeating import'` (from
  `apps/desktop`) — PASS, one Chromium test. The serialized browser fixture
  receives its baseline explicitly, verifies a lagging read keeps recovery
  actionable, then verifies a same-project current read reconciles with one
  import call. The ignored one-off config directs the Vite cache and Playwright
  artifacts into the author worktree.
- `cargo check --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml` — PASS on macOS.
- `cargo check --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --target x86_64-pc-windows-gnu` — BLOCKED before compiling the desktop crate because `libsqlite3-sys` could not find `x86_64-w64-mingw32-gcc`; no Windows adapter compilation or runtime validation is claimed.
- `python3 tools/architecture.py` — PASS after removing a rejected direct `libc` dependency and retaining platform-specific no-follow flags only for macOS/Linux.
- `python3 tools/source_inventory.py generate` / `check` — PASS, 522 packages; no package versions were updated.
- `python3 tools/ledger.py generate` / `check` — PASS, 5,396 source blocks, 438 explicit ID occurrences and 447 headings; all map obligations remain `IN_PROGRESS`.
- `python3 -m unittest tests.test_ledger` — PASS, 31 tests.
- `cargo fmt --all -- --check` — PASS.
- `git diff --check` — PASS.

Offline npm dependency installation lacked a cached `yallist` tarball, so the
TSC/Vitest executables read the already-installed desktop dependency tree
through the author worktree's temporary symlink
`apps/desktop/node_modules` → `/Users/vincent/code/kyberia/apps/desktop/node_modules`.
The symlink was moved back to ignored `.trash/desktop-node-modules-root-link`
after the checks; its root target was not touched. Before testing, Vitest's
resolved cache directory was verified as
`/private/tmp/kyberia-phase0-desktop-map/.trash/test-runs/desktop-vitest-cache`,
and npm's cache was explicitly set to `.trash/test-runs/npm-cache`. Playwright
used the same npm cache plus the retained isolated config under
`apps/desktop/.trash/test-runs/`; its Vite cache and browser artifacts remained
in the author worktree. Vitest's generated `results.json` appeared only in the
author worktree cache. The
pre-existing root cache
`/Users/vincent/code/kyberia/apps/desktop/node_modules/.vite/vitest/da39a3ee5e6b4b0d3255bfef95601890afd80709/results.json`
retained the same modification time and SHA-256 before and after the final
runs. No `npm install`, chmod or root-cache cleanup was performed.

The full Playwright suite, frontend production build, native picker runtime, Windows adapter
compilation/runtime, and two-platform determinism remain unvalidated. There is
no live-radio, real PNG pixel decoding/display, field calibration, raw export
or Phase 0 exit evidence. Fresh independent rereview of the author follow-up
is required before integration.
