# Independent rereview: bounded map-preview IPC normalization

## 1. Disposition

**APPROVE the bounded IPC-shape fix, with native WebView transport validation still open.** The previously reported MAJOR macOS response-shape mismatch is resolved in the reviewed source: valid macOS numeric arrays are normalized to an `ArrayBuffer` before the UI consumes them. I found no new BLOCKER or MAJOR issue in this rereview.

This approval applies only to the isolated response-normalization increment. It does not approve or integrate the preview feature as a whole, and does not close MAPB-001, MAPB-002, or Phase 0.

## 2. Candidate and ancestry

- Reviewed exact HEAD: `2b8becc197141ee4b84b2e5410a4d76d73841b0f`.
- Fix commit: `2d40f45180f933eb5af74bd8f9b20eb70c23101c`.
- Fix parent / previously reviewed candidate: `0777ca14d1e783405e300556bec8e3c8a72a3be4`.
- Initial base/merge-base: `a8cf7dccbe1a1c40d6adc51868e90dfe2cdcd452`.
- Plan SHA-256 verified unchanged: `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`.

## 3. Review scope and evidence limits

Reviewed the focused diff in `apps/desktop/src/lib/contracts.ts`, its tests, the Tauri adapter in `apps/desktop/src/lib/ipc.ts`, the preview consumer, and associated status, traceability, ledger, and validation updates. This is targeted source review; the codebase-memory graph was unavailable, so no graph coverage or completeness claim is made.

## 4. Response-shape and allocation bounds

`assertMapPreviewBytes` accepts only `ArrayBuffer` or `Array.isArray` values. It checks minimum and maximum encoded lengths (33 bytes through 1 MiB) before allocating for the array case. It then visits each index and requires a number that is an integer in `[0,255]` before assigning into a newly allocated `Uint8Array`; ordinary sparse holes, fractional values, strings, nulls, NaN, and out-of-range integers are rejected. Typed arrays and strings/base64 are rejected by the top-level type check. The copy cannot exceed the 1 MiB cap.

Both forms then use the same existing PNG signature, IHDR, RGBA8, and maximum-edge checks. The `ArrayBuffer` branch preserves the original buffer; the numeric-array branch returns the newly normalized buffer. I found no unbounded allocation or coercion path in this validator.

## 5. IPC invocation and UI contract

The Tauri `mapPreview` adapter invokes `project_map_preview` through `invokeProject<ArrayBuffer>(..., assertMapPreviewBytes)`. `invokeProject` passes the raw `invoke<unknown>` result directly to the supplied validator, so the numeric-array result is normalized before leaving the adapter. The public `DesktopIpc.mapPreview` contract remains `Promise<ArrayBuffer>`.

The project-session wrapper returns that promise without transforming the bytes. `startMapPreview` revalidates the result and passes its `ArrayBuffer` to `Blob` as `image/png`. Thus the UI continues receiving the same normalized type it expects. Existing cleanup and failure fallback semantics are unchanged.

## 6. Regression tests

The added contract tests cover successful `ArrayBuffer` preservation and numeric-array normalization, equality of normalized bytes, unsupported typed-array and string inputs, invalid PNG signature, the size cap, sparse arrays, fractional/non-number/NaN/out-of-range entries, and oversized arrays. The sparse-hole check exercises the ordinary JSON-array case. The focused regression models Tauri's macOS JSON numeric-array payload at the pure validator; it does not call a native WebView.

## 7. Native transport limitation

Actual macOS WebView invocation is not essential to approve this bounded code fix: source inspection shows the raw invoke result is passed directly through the validator, and the regression exercises the exact JSON number-array shape identified in the first review. However, the candidate has not demonstrated a native macOS WebView round trip, and Windows/Linux transport behavior also remains unvalidated. Keep those runtime checks as explicit product-validation gates; do not describe this candidate as cross-platform verified.

## 8. Tracking and documentation

`STATUS.md`, the validation packet, traceability, and ledger all retain MAPB-001, MAPB-002, and Phase 0 as `IN_PROGRESS`, state that independent rereview/integration remain pending, and disclose the lack of native WebView invocation and broader cross-platform validation. The validation packet accurately distinguishes author-host test claims from product acceptance. SHA-256 checks matched ledger records for `contracts.ts`, `ipc.ts`, `contracts.test.ts`, and the validation packet. The plan hash matches the prior review.

## 9. Independent checks and limitations

Passed independently in this rereview tree: `python3 tools/ledger.py check` (5396 blocks, 438 explicit ID occurrences, 447 headings), `python3 tools/architecture.py check`, `python3 tools/source_inventory.py check` (522 packages), plan/source hash checks, and `git diff --check` against the reviewed parent.

Vitest and strict TypeScript could not be independently rerun: this isolated review tree has no `apps/desktop/node_modules`, and this review was constrained to report-only changes. The candidate records 23 Vitest tests and strict TypeScript passing on the author macOS host. Rust suites were not rerun because the rereview diff is limited to the frontend validator/tests and tracking.

## 10. Findings and conclusion

- **BLOCKER:** none found in the reviewed fix.
- **MAJOR:** the initial macOS `Vec<u8>` JSON-array incompatibility is resolved by bounded normalization.
- **MINOR:** native WebView invocation and non-macOS runtime transport remain unverified; already documented as open validation limits, not a defect in this isolated normalization change.

Approve this fix for its bounded scope. Preserve the broader phase and runtime-validation gates as open, and do not treat this report as approval of integration or Phase 0 completion.
