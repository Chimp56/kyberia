# Phase 2 IE-explorer core focused re-review

## Scope and verdict

- Follow-up: `a83b83344d2bec9d6c7a4f76fd570ded2d0db6fa`, atop candidate `84c1876d0eae5297bc2e59f81d991c7efea1d961`.
- Dedicated detached rereview worktree: `/private/tmp/kyberia-phase2-ie-explorer-rereview-current`.
- **PASS — the prior minor finding is resolved; no new finding.**

## Finding rechecked

`ElementWarnings::has_warning()` now returns true for malformed elements or any repeated singleton cardinality violation. The regression constructs identical repeated SSID elements and confirms that both views expose the singleton violation, report identical payloads, are not contradictory, and do report a warning. A matching regression confirms repeated vendor IEs remain permitted, non-singleton, and warning-free. Existing contradictory-repeat metadata and raw bytes/order are unchanged by the follow-up diff, which touches only this predicate/comment, its focused test, ledger hashes, and the validation note.

`INS-005` and Phase 2 remain `IN_PROGRESS`. Association parsing, Lab UI/IPC, standards clause/help links, and broader Phase 2 acceptance remain explicitly open.

## Reviewer validation

- `cargo test -p kyberia-ieee80211 --locked --offline` — PASS: 31 unit, 3 fixture differential, and 3 compile-fail doctests; one tcpdump differential test remains explicitly ignored by the default run.
- `cargo clippy -p kyberia-ieee80211 --locked --offline --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS (5,396 source blocks; 438 explicit ID occurrences; 447 headings), including updated source/validation hashes.
- `python3 tools/architecture.py check` — PASS.
- `python3 tools/source_inventory.py check` — PASS (522 locked external packages).
- `git diff --check` — PASS.

## Ten-field handoff

1. **Verdict:** PASS; original MINOR finding resolved; no new findings.
2. **Reviewed commits:** fix `a83b83344d2bec9d6c7a4f76fd570ded2d0db6fa`, atop `84c1876d0eae5297bc2e59f81d991c7efea1d961`.
3. **Scope:** Only the singleton-warning fix and its regression/documentation/ledger updates.
4. **Identical singleton:** Warns through `has_warning()` and remains non-contradictory.
5. **Allowed vendor repeats:** Remain non-singleton and warning-free.
6. **Evidence fidelity:** Follow-up does not alter raw TLV, ordering, or repeat grouping behavior.
7. **Tests:** 31 unit + 3 differential + 3 compile-fail doctests pass; one external tcpdump test is intentionally ignored by default.
8. **Scoped gates:** Strict Clippy, formatting, ledger, architecture, source inventory, and diff checks all pass.
9. **Blockers/status:** No blocker; `INS-005` and Phase 2 remain in progress with wider product gaps open.
10. **Report tree:** This report is the only intended rereview-tree change; report commit and final clean state are supplied in the handoff.
