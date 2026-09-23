# Current-main desktop PNG map workflow candidate

This packet describes a manual, isolated adaptation from base
`bc51e80b14e30f927628f4ba9f2e92a4773423fe` on
`feat/phase0-desktop-map-current`. It is not integrated and has not received
independent review. It does not close Phase 0/1 or MAP-002/MAP-003/MAPB-001/
MAPB-002.

## Plan scope

- §5.3 floor-plan ingestion and coordinate calibration: bounded PNG path and
  numeric two-point calibration only.
- §6.2 MAP-002 and §6.3 MAP-003: initial floor baseline, operation-derived
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
  canonical operation set. Exact retries reuse immutable stored operation and
  artifact evidence after reopen.
- The strict current-main `map_asset.rs` parser is not replaced. Import stays
  container-only: PNG signature, chunk order/length/CRC and bounded metadata
  are checked; IDAT is not inflated and pixels are neither returned nor
  rendered.
- A durable receipt is authoritative after append. Canonical current-view
  readback is separate and can be reported as unavailable without describing
  the mutation as rolled back.
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
  native selection.

## Checks performed

- `cargo test --locked --offline --quiet -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application -- --test-threads=1` — PASS, all selected unit/integration/doc tests; existing explicitly ignored tests remained ignored.
- `cargo test --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --quiet` — PASS after the final Rust test edits: 23 desktop library tests, 6 binary tests and 2 IPC boundary tests. Tests include fresh-floor projection, opaque one-shot project-bound grants, Unix symlink rejection, grant/retry bounds and TTL, bounded/cancellable staging, live map-picker status/cancellation, and committed-receipt preservation when the separate readback reports an error.
- `cargo clippy --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application --all-targets -- -D warnings` — PASS.
- `cargo clippy --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings` — PASS.
- `npm run typecheck` — PASS before the final exact-retry comparison change and two new IPC contract assertions.
- `npm test -- --reporter=dot` — PASS, 15/15 tests before the two new map IPC contract assertions.
- `cargo check --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml` — PASS on macOS.
- `cargo check --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --target x86_64-pc-windows-gnu` — BLOCKED before compiling the desktop crate because `libsqlite3-sys` could not find `x86_64-w64-mingw32-gcc`; no Windows adapter compilation or runtime validation is claimed.
- `python3 tools/architecture.py` — PASS after removing a rejected direct `libc` dependency and retaining platform-specific no-follow flags only for macOS/Linux.
- `python3 tools/source_inventory.py generate` / `check` — PASS, 522 packages; no package versions were updated.
- `python3 tools/ledger.py generate` / `check` — PASS, 5,396 source blocks, 438 explicit ID occurrences and 447 headings; all map obligations remain `IN_PROGRESS`.
- `python3 -m unittest tests.test_ledger` — PASS, 31 tests.
- `cargo fmt --all -- --check` — PASS.
- `git diff --check` — PASS.

The npm commands used an ignored symlink at
`apps/desktop/node_modules` pointing to the already-installed root desktop
dependencies because offline installation in this worktree lacked the cached
`yallist` tarball. The package-lock checksum and installed TypeScript/Vitest
versions matched, but Vitest wrote
`apps/desktop/node_modules/.vite/vitest/da39a3ee5e6b4b0d3255bfef95601890afd80709/results.json`
through that symlink. The symlink itself was moved (not deleted) from
`apps/desktop/node_modules` to the ignored
`.trash/desktop-node-modules-root-link`; its root target was not touched. The
Vitest results cache was not cleaned or altered. Therefore
the reported TSC/Vitest result is not isolated from the shared package cache,
and frontend checks were stopped after that was discovered. The later contract
assertions, Playwright, frontend build and browser/native picker execution have
not been validated here.

Other remaining checks before integration include frontend revalidation in a
worktree-local dependency/cache environment, Playwright, final branch
cleanliness and fresh independent review. No live radio,
native two-OS, real PNG pixel
decoding/display, field calibration, raw export or Phase 0 exit evidence is
claimed.
