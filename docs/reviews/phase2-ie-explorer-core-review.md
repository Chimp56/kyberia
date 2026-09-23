# Phase 2 IE-explorer core independent review

## Review scope

- Candidate: `84c1876d0eae5297bc2e59f81d991c7efea1d961`
- Base: `bc51e80b14e30f927628f4ba9f2e92a4773423fe`
- Plan: §6.1 / INS-005 and Phase 2 roadmap, including the scoped parser/explorer, status, validation note, ledger, and generated traceability changes.
- Source reviewed: `crates/ieee80211/src/lib.rs`; documentation and metadata reviewed: `docs/validation/phase2-ie-explorer-core.md`, `STATUS.md`, `docs/implementation/ledger.json`, and `docs/implementation/TRACEABILITY.md`.
- Graph MCP tools/resources were unavailable in this session; no graph coverage claim is made.

## Verdict

**APPROVE WITH MINOR FOLLOW-UP.** No blocker or major issue was found in the bounded core increment. The implementation is additive to the existing crate API. The single minor finding below concerns the meaning of the convenience warning predicate; consumers can currently inspect the more specific repeat metadata directly.

## Evidence

- `InformationElementExplorer` and `ExplorerElement` borrow from the parsed `ManagementFrame`; iteration preserves source order and duplicates. Each entry exposes a slice of the original MPDU for the full TLV, its raw payload, numeric element/extension identity, and the existing decoded value. FCS framing is explicit and validated when requested.
- `ElementDecode` keeps recognized, opaque `Unknown`, and typed `Malformed` states distinct. Numeric IE/extension identity is not presented as a clause citation. Repeated singleton cardinality and contradictory payload evidence are represented separately; vendor-specific repetitions are not classified as singleton violations.
- `diff_information_elements_with_usage` keys by `(element ID, extension ID, occurrence)`, compares raw payloads, returns added/removed/modified occurrences, and separately reports wire-sequence and management-subtype changes. It does not claim to align duplicate values across insertions; ordinal pairing can yield multiple modifications after an insertion, with wire order still reported separately.
- Parse and diff paths enforce frame/element/payload, work, and logical-allocation limits with cancellation checks. Diff occurrence ordering uses bounded in-place heap sort; the default 1,024-element boundary and limit/cancellation cases have regressions.
- Candidate metadata is appropriately scoped: `INS-005` and Phase 2 remain `IN_PROGRESS`; the notes explicitly leave association frames, desktop Lab UI/IPC, standards clause/help links, and full Phase 2 acceptance open. Existing `WIFI-001` source identity was refreshed without extending its prior parser review/fuzz claims.

## Validation run by reviewer

- `cargo test -p kyberia-ieee80211 --locked --offline` — PASS: 30 unit, 3 fixture differential, and 3 compile-fail doctests. One external tcpdump differential test is explicitly ignored by the default suite.
- `cargo clippy -p kyberia-ieee80211 --locked --offline --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS (5,396 source blocks; 438 explicit ID occurrences; 447 headings).
- `python3 tools/architecture.py check` — PASS.
- `python3 tools/source_inventory.py check` — PASS (522 locked external packages).
- `git diff --check` — PASS.

## Finding

### MINOR — `has_warning()` omits identical singleton repetitions

At `crates/ieee80211/src/lib.rs:503-504`, `ElementWarnings::has_warning()` returns true only for malformed payloads or contradictory repeats. However, `summarize_repeats` marks repeated known singleton IDs with `violates_singleton_cardinality = true` even when their raw payloads are identical, while `contradictory` remains false. In that case the element's `repetition()` exposes a cardinality violation but `has_warning()` returns false. A consumer that uses the convenience predicate for a warning icon could therefore miss a repeated singleton.

Suggested follow-up: either include `repetition.violates_singleton_cardinality()` in `has_warning()` or rename/document the predicate narrowly, and add a regression for identical duplicate singleton IEs. The finding does not invalidate the raw or repeat metadata and is non-blocking for this increment.

## Residual scope / limitations

The review does not establish full `INS-005` or Phase 2 completion, association-frame parsing, UI/IPC behavior, clause-level standards references, external tcpdump agreement, hardware capture correctness, broader parser fuzz campaign, or product-level integration. The disclosed ordinal duplicate-pairing behavior remains a known limitation rather than a blocker because both raw views and the independent wire-order flag preserve the relevant evidence.

## Ten-field handoff

1. **Verdict:** APPROVE WITH MINOR FOLLOW-UP.
2. **Candidate/base:** `84c1876d0eae5297bc2e59f81d991c7efea1d961` / `bc51e80b14e30f927628f4ba9f2e92a4773423fe`.
3. **Scope:** Rust IE explorer/diff core plus scoped status, validation, ledger, and traceability updates.
4. **Raw fidelity:** Exact original TLV/payload slices; source order and duplicates preserved.
5. **Decode/warnings:** Unknown and malformed remain distinct; repeat metadata retained. One MINOR convenience-warning omission is recorded above.
6. **Diff semantics:** Deterministic numeric-key/ordinal pairing, raw payload comparison, separate wire-order and subtype indicators; insertion alignment remains ordinal.
7. **Resource safety/API:** Bounded counters, allocation budget, cancellation and heap sort covered; source diff is additive with no removed lines.
8. **Validation:** Crate tests, strict Clippy, fmt, ledger, architecture, inventory, and diff checks all PASS; external tcpdump remains intentionally ignored.
9. **Blockers/residuals:** No blocker. Association, UI/IPC, clause/help catalog, hardware/external validation, and Phase 2 exit remain open.
10. **Reviewer tree/report:** Dedicated worktree `/private/tmp/kyberia-phase2-ie-explorer-review-current`, branch `review/phase2-ie-explorer-current`; this report is the only intended review-tree change. The report commit and final clean status are provided in the handoff.
