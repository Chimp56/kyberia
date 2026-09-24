# Focused ledger correction re-review — ecb9ab1

## 1. Verdict and scope

**APPROVED — prior minor finding resolved.** This is a focused review of the
ledger validation-record correction only. No product code, application
semantics, or browser behavior changed in this candidate; browser rerun was
not warranted.

## 2. Plan anchors

The correction retains the bounded desktop map workflow validation record for
§5.3, §6.2 MAP-002/MAP-003, Phase 0 §17, and §18.4 MAPB-001/MAPB-002. It does
not imply product-gate completion.

## 3. Exact revisions

- Candidate: `ecb9ab1084aa112b8785d179e8ec1a15f8c8414d`.
- Parent: `b92ade419c415ae48076b40ba937f351352d35c0`.
- Integration main comparison: `77347586dd94a494684a073d9a0756a78757b22d`.

## 4. Review worktree

`/private/tmp/kyberia-phase0-desktop-rereview-ecb9ab1-20260923`, detached at
the exact candidate. Candidate files were not edited.

## 5. Inspected paths

Reviewed the entire candidate-to-parent diff in
`docs/implementation/ledger.json`, the referenced
`docs/validation/desktop-map-workflow-current-main.md` checks and isolation
notes, plus the corresponding current-main ledger area and status/diff state.

## 6. Finding resolution

The prior minor stale-evidence finding is resolved. Ledger lines 30509–30514
now record TSC/Vitest and targeted Playwright recovery checks as passing with
worktree-local caches, Tauri `23+6+2`, the unchanged root Vitest cache, and the
temporary dependency symlink restored to ignored trash. These statements
agree with the packet at lines 65 and 68–82 and its cache/symlink notes at
lines 92–108. The packet SHA-256 matches the ledger value:
`934979ef36b0f346e5a495790685b8b15ad8355bb56be494bcf8383e218f8b20`.
The ledger continues to state that desktop/product exits remain open.

## 7. Independent checks

- `python3 tools/ledger.py check` — **PASS**: 5,396 source blocks, 438
  explicit ID occurrences, 447 headings.
- `git diff --check b92ade419c415ae48076b40ba937f351352d35c0..HEAD` — **PASS**.
- Candidate-to-parent diff is limited to the two description/scope lines in
  `docs/implementation/ledger.json`.
- Browser, TSC, Vitest, and Rust tests were not rerun because this commit only
  changes the ledger text; the validation packet is recorded evidence, not a
  result independently reproduced in this focused rereview.

## 8. Current-main compatibility and tracking

The correction is governance/evidence text, not an implementation/API change.
The requested integration baseline has later divergence in `STATUS.md`,
`TRACEABILITY.md`, `ledger.json`, and validation packet presence; carry the
accurate validation wording into the live ledger when reconciling that
governance history rather than replacing the current-main ledger wholesale.
Open MAP/Phase 0 statuses and the remaining runtime/product gates are
unchanged. Graph MCP tools/resources were unavailable; this report makes no
graph-coverage or completeness claim.

## 9. Report commit and cleanliness

Report-only commit: recorded in the final handoff. Only this report is
committed in the assigned review worktree; it is clean afterward.

## 10. Remaining risks and blockers

No remaining finding in the ledger correction. The validation packet still
marks the full Playwright suite/build, native picker runtime, Windows adapter,
two-platform determinism, real pixel decoding/display, broader calibration,
raw export, and Phase 0 exit evidence as unvalidated/open.
