# Map asset PNG ordering correction re-review

Date: 2026-09-23

## 1. Verdict and scope

**PASS for the correction: the prior MAJOR PNG ordering finding is resolved;
no new finding in this bounded re-review.** This re-review is companion to the
broader review recorded in commit `b036a5454bdf2e17dc92a4e1bb15b21a95f5d70e`.
Together they leave no recorded source finding outstanding for this bounded
candidate. This is not product acceptance and does not close Phase 0.

## 2. Plan anchors

The authoritative plan had been read in full in the preceding review; for this
re-review I reread the relevant requirements: §5.3, floor-plan ingestion and
calibration (L337–348); MAP-002 (L803–820); MAP-003 (L822–831);
MAPB-001/MAPB-002 (L4268–4269); §15 threat model/project security (L3604–3620,
3681–3688); §16 parser testing (L3745–3769); and Phase 0 deliverables/exit
criteria (§17, L3987–4016). The fix is limited to PNG admission ordering;
the full import catalog, full calibration scope and Phase 0 exit remain open.

## 3. Exact revisions

- Correction candidate: `5d64d111915246f649fa6afecd9660385239909b`
- Parent/original candidate: `ca99d5055712861271a638b6cac684d9b3a9faab`
- Review branch before report commit: `review/map-asset-order-fix-rereview-20260923`

## 4. Reviewed paths

Reviewed the complete correction diff: `crates/application/src/map_asset.rs`,
`docs/architecture/ADR/0034-map-asset-admission.md`,
`docs/implementation/TRACEABILITY.md`, `docs/implementation/ledger.json`, and
`docs/validation/map-asset-admission.md`. Also rechecked `STATUS.md` and the
relevant plan sections above. This report is the only review-worktree edit.

## 5. Ordering and evidence findings

No ordering finding remains in the supported subset:

- `gAMA` and `sRGB` at `map_asset.rs` L231–250 require IHDR, a pre-IDAT
  position, and no previously seen `PLTE`; duplicate instances and invalid
  lengths/values are rejected. This matches PNG-3 §5.6 Table 7.
- `tRNS` at L190–205 remains before IDAT, is type/length constrained, and for
  indexed images requires an earlier PLTE. For truecolor, PLTE is optional;
  if tRNS was encountered first, the PLTE branch at L168–188 now rejects the
  later palette. Thus `tRNS` follows PLTE when one is present.
- `pHYs` at L251–259 is limited to one valid-length chunk before IDAT; Table 7
  imposes no ordering relative to PLTE, and the parser leaves that relative
  order unconstrained. PLTE is unique and before IDAT; IDAT chunks are
  consecutive; IEND is terminal with exact EOF. No other ancillary chunks are
  admitted; unknown, text, profile, EXIF and animation chunks fail closed.
- The positive fixture at L458–468 exercises `gAMA`/`sRGB`, PLTE, tRNS and
  pHYs in a valid order. Four malformed fixtures cover sRGB and gAMA after
  PLTE, truecolor tRNS before a later PLTE, and pHYs after IDAT. The shared
  fixture builder at L325–331 computes CRC over type+data using the PNG
  reflected CRC-32 polynomial and standard init/final complement; malformed
  cases therefore reach ordering rejection with CRC-valid chunks. The parser
  test filter independently passed all 11 map-asset tests.
- Documentation now describes only this strict admitted subset. It continues
  to state that admission **does not validate IDAT zlib/DEFLATE or pixel data,
  and does not establish decodability or displayability**.

No new implementation or documentation finding arose in this correction.

## 6. Independently rerun validation

All Rust checks used `--locked --offline`; local dependencies resolved without
an online retry.

- `cargo test --locked --offline -p kyberia-application map_asset::tests -- --test-threads=1` — PASS, 11 passed.
- `cargo test --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application -- --test-threads=1` — PASS, 334 unit/integration tests and 8 doctests passed; 2 explicit benchmark tests ignored.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application --all-targets -- -D warnings` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 locked packages.
- `python3 tools/ledger.py check` — PASS, 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `git diff --check ca99d5055712861271a638b6cac684d9b3a9faab...5d64d111915246f649fa6afecd9660385239909b` — PASS.

The full workspace suite was not run.

## 7. Status and evidence consistency

MAP-002, MAP-003, MAPB-001 and MAPB-002 remain `IN_PROGRESS`; broader leaf
requirements remain open. The validation packet, ADR and STATUS do not claim
Phase 0 exit, pixel decoding, displayability or a complete map workflow. The
candidate's review-pending labels and empty ledger review arrays are unchanged
because this re-review was authorized to edit only its report; reconcile them
when carrying this report into the integration evidence.

## 8. Report commit and worktree

Only this report is committed on the isolated review branch. The exact report
commit ID and resulting clean status are supplied in the reviewer handoff,
since a commit cannot contain its own hash.

## 9. Unresolved risks and limitations

- This re-review addresses the reported ancillary-ordering defect, not a new
  full audit of every source line already covered by the broader review.
- IDAT zlib/DEFLATE and pixel validity/displayability remain deliberately
  unverified. JPEG/TIFF/WebP, PDF, vector/CAD/geospatial formats, previews/UI,
  multi-point/residual/CRS calibration, evidence migration and Phase 0 exit
  remain open.
- Codebase-memory graph tools/resources remain unavailable; this report makes
  no graph or index-completeness claim.

## 10. Blockers

No blocker remains for the reported PNG-ordering correction in the tested
scope. This report does not replace integration-level reconciliation of the
review-pending status/ledger metadata, nor make a product-acceptance claim.
