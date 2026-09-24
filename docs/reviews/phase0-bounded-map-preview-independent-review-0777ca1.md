# Phase 0 bounded PNG map-preview independent review — 0777ca1

## 1. Verdict and scope

**REQUEST CHANGES — one unresolved MAJOR finding.** The CPU decoder, per-request bounds, canonical artifact resolution, and stale-result cleanup are otherwise well-scoped. The preview is not usable on macOS as wired because the renderer accepts only `ArrayBuffer`, while pinned Tauri 2.11.5 returns this raw `Vec<u8>` as a JSON number array on macOS. Do not integrate this candidate until the response contract handles the platform transport and a regression covers it. This is not Phase 0, `MAPB-001`, or `MAPB-002` acceptance.

## 2. Plan and records

Reviewed against the unchanged plan SHA-256
`1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`:
§5.3 plan ingestion/calibration, §6.2 MAP-002/MAP-003, §13 rendering UX,
§15.2 import/decompression limits, §16 validation, Phase 0 §17, and §18.4
MAPB-001/MAPB-002. The validation packet correctly states that this PNG-only
preview does not close the format/import backlog or Phase 0. The source-qualified
ledger and status keep both backlog IDs `IN_PROGRESS` and the review/integration
gates open.

## 3. Exact revisions

- Candidate: `0777ca14d1e783405e300556bec8e3c8a72a3be4`.
- Base and merge-base: `a8cf7dccbe1a1c40d6adc51868e90dfe2cdcd452`.
- Product source commit: `f3ffa1893b0c1a1614d13af54a0a7ef4ab8bb82b`; the candidate HEAD adds the final tracking/evidence commit.
- Plan digest: `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`.

## 4. Review worktree

Reviewed in `/private/tmp/kyberia-phase0-raster-preview-review-0777ca1`,
detached at the exact candidate. No candidate/source or author-tree file was
edited. Only this report is intended to be committed in the review tree.

## 5. Inspected paths

Reviewed the candidate diff and source/tests in `crates/application/src/map_preview.rs`,
`crates/application/src/{port.rs,session.rs}`, `crates/application/tests/project_session.rs`,
`apps/desktop/src-tauri/src/{lib.rs,main.rs}`, desktop
`src/lib/{contracts.ts,ipc.ts,map-preview.ts,useProjectSession.ts}`,
`src/components/{CanvasStage.tsx,InspectorPanel.tsx}`, associated frontend
tests, `Cargo.lock`, architecture/license inventory, `STATUS.md`, ledger,
traceability, and `docs/validation/phase0-map-preview.md`. Also inspected the
locked Tauri 2.11.5 raw-response implementation under Cargo's local registry.

## 6. Findings and invariants

### MAJOR — macOS Tauri response shape is rejected by the frontend

`project_map_preview` returns `tauri::ipc::Response` containing `Vec<u8>`
(`apps/desktop/src-tauri/src/main.rs:907-947`). On macOS, pinned Tauri 2.11.5
routes a raw response through its JSON callback path
(`tauri-2.11.5/src/ipc/protocol.rs:392-405`); `format_result` serializes the
`Vec<u8>` to JSON, yielding a numeric array (`tauri-2.11.5/src/ipc/format_callback.rs:111-118`).
But `assertMapPreviewBytes` accepts only `ArrayBuffer`
(`apps/desktop/src/lib/contracts.ts:194-215`), and `tauriIpc.mapPreview` applies
that assertion directly to `invoke`'s value (`apps/desktop/src/lib/ipc.ts:57-60`).
Thus macOS preview responses fail validation and `startMapPreview` converts the
failure into “Preview unavailable” (`src/lib/map-preview.ts:21-30`). The Rust
IPC DTO test and package compilation do not exercise this host-specific JS
payload shape.

Normalize the response at the IPC adapter boundary: retain `ArrayBuffer` for
platforms returning raw bytes and accept a bounded, dense byte array on macOS,
checking length ≤ 1 MiB and every element is an integer in `[0,255]` before
converting it to an `ArrayBuffer`. Add a macOS response-shape regression (and
verify other supported desktop runtimes); describe the transport as
platform-dependent rather than asserting it is never JSON.

### Verified invariants and residual limits

- The request accepts only the schema and canonical map ID; no path or hash is
  renderer-controlled. Application lookup starts from the active session's
  verified canonical snapshot, requires exact registered `MapSource` kind,
  `image/png`, and byte-length metadata, then reads by trusted SHA-256. The
  store validates registration, file type/no-follow access, size and content
  hash. The Tauri command checks the active session identity again after decode.
- Per request, source size is capped at 32 MiB, source preview pixels at 16M,
  decoder/output allocation at 64 MiB, thumbnail edge at 1,536 pixels,
  thumbnail RGBA storage at 9 MiB, and encoded output at 1 MiB through a capped
  writer. The bounds and arithmetic checks are internally consistent for the
  admitted dimensions. Decode failure does not mutate or replace the original
  artifact; original map/calibration dimensions are preserved.
- The frontend ignores completion after cleanup and revokes any created Blob
  URL. The backend also suppresses results if the active project changed.
  Cleanup does not cancel an inflate/encode already in progress; this is stated
  in the packet. That remains a bounded-work limitation, not an additional
  blocker in this review.

## 7. Independent checks and retained evidence

Independently reran in the review worktree:

- `cargo test --locked --offline -p kyberia-application -- --test-threads=1` —
  PASS, 48 tests (19 unit, 8 snapshot, 21 project-session).
- `cargo test --locked --offline --manifest-path apps/desktop/src-tauri/Cargo.toml -- --test-threads=1` —
  PASS, 32 tests (24 library, 6 binary, 2 IPC boundary).
- Strict application and Tauri all-target Clippy — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS: 5,396 source blocks, 438 explicit-ID
  occurrences, 447 headings.
- `python3 tools/architecture.py check` and
  `python3 tools/source_inventory.py check` — PASS; 522 locked packages.
- Candidate `git diff --check` — PASS; plan digest matches the recorded value.

I did not rerun Vitest or TypeScript. Their candidate-local passing results are
recorded in the validation packet, but they do not cover the macOS raw-response
transport. No desktop webview invocation/runtime was exercised in this review.

## 8. Tracking and compatibility

The candidate is based on an earlier tree, not current main. Its status,
traceability, and ledger accurately retain Phase 0/MAPB-001/MAPB-002 as open;
the feature source hashes match the ledger and the plan hash matches. The
ledger's current-main review entry is about the prior metadata/import workflow,
not an approval of this preview candidate. The new finding must be resolved and
reviewed before updating that candidate's review evidence or integrating it.

Codebase-memory graph tools were unavailable in the review context; this report
makes no graph-coverage or exhaustive call-graph claim.

## 9. Report commit and cleanliness

Report-only commit: recorded in the parent handoff. No candidate or author-tree
files were changed. The review worktree should be clean after this report is
committed.

## 10. Remaining blockers and limits

The macOS response-shape MAJOR finding blocks approval for integration. After
the adapter normalization and host-specific regression, request a focused
independent rereview. Windows/Linux runtime, two-platform deterministic image
fixtures, native selector runtime, capability probes, broader raster/vector
formats, raw export, and all Phase 0 exit criteria remain open as the packet
states. This review does not promote Phase 0 or either MAPB backlog item.
