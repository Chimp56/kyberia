# Phase 8 parser-boundary correction re-review

**Disposition: APPROVED for the bounded parser/canonical-encoding correction.** Both findings from the prior independent review are resolved in the reviewed source. This approves neither a running plugin system nor the broader Phase 8 SDK exit.

## 1. Objective and scope

Re-review candidate `11065da8d800021a50cb2a91996df2f8d0e67310`, specifically the prior findings in `docs/reviews/phase8-plugin-sdk-current-review.md` (report commit `a1cb01f8d994295d553b901512a8ae413edd979d`): enforce original-byte and nesting bounds before JSON deserialization, retain strict parse behavior with malformed-input coverage, and pin canonical version-1 bytes/reference identity with a golden vector. Also verify the candidate's documentation, ledger/source hashes, status, and declared runtime/WIT limitations. The codebase graph MCP tools were unavailable; this review makes no graph or index-coverage claims.

## 2. Requirements and records

Scope remains `plan.md` §§10.13, 14.7, 15.6, Phase 8, `backlog:UX-012:1`, and `audit:EXT-001:1`. `audit:EXT-002:1` and `audit:SEC-002:1` must remain open. The candidate correctly labels the follow-up review as pending; it does not claim Phase 8 completion, WIT validation, runtime invocation, resource enforcement, or sandboxing.

## 3. Base and candidate

- Prior reviewed head/base: `14076ff8c35b5cdcab123ad5d68154302a7478b3`
- Correction candidate: `11065da8d800021a50cb2a91996df2f8d0e67310`
- Fresh review worktree: `/private/tmp/kyberia-phase8-plugin-sdk-rereview-20260923`
- Candidate author worktree: `/private/tmp/kyberia-phase8-plugin-sdk-current` (not modified)

## 4. Reviewed paths

Reviewed the exact correction diff and relevant source regions in `crates/plugin-sdk/src/{model.rs,validation.rs}` and `crates/plugin-sdk/tests/contracts.rs`, plus `docs/architecture/plugin-sdk-contracts.md`, `docs/validation/phase-eight-plugin-sdk-contracts.md`, `docs/implementation/{TRACEABILITY.md,ledger.json}`, `STATUS.md`, and the earlier independent report. `Cargo.lock`, `tools/architecture.json`, and source inventory were checked through the scoped validation commands.

## 5. Semantics and invariants

- `parse_and_validate_manifest` validates the host policy, rejects `bytes.len() > host.max_manifest_bytes`, scans nesting, and only then calls `serde_json::from_slice`. The host limit therefore applies to original wire bytes, including whitespace, rather than only re-serialized canonical bytes.
- The nesting scanner is allocation-free, ignores delimiters in quoted strings, accounts for escaped quotes/backslashes, and rejects depth beyond the documented 32-delimiter ceiling before Serde. Serde remains responsible for syntax and typed parsing.
- Structs use `deny_unknown_fields`; the raw-path tests explicitly cover duplicate and unknown top-level keys and malformed in-limit JSON. Oversized malformed bytes, whitespace, long-field and large-array inputs are rejected at the raw-length preflight.
- A fixed expected canonical JSON string and reference digest now pin the v1 Rust encoding. Capability and data-contract identifiers also have explicit golden string checks. Documentation appropriately does not claim cross-language compatibility from this Rust vector.
- The parser itself receives an already-buffered byte slice; documentation explicitly requires transport/file readers to bound reads before buffering. It does not claim a general heap quota.

## 6. Checks

Independently run from the fresh worktree:

- `cargo test -p kyberia-plugin-sdk --locked --offline` — PASS, 23 focused tests; unit/doc targets also pass (0 tests).
- `cargo clippy -p kyberia-plugin-sdk --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt -p kyberia-plugin-sdk -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS (5,396 source blocks; 438 explicit ID occurrences; 447 headings).
- `python3 tools/architecture.py` — PASS (dependency direction and external-package boundaries).
- `python3 tools/source_inventory.py check` — PASS (522 locked external packages).
- `git diff --check 14076ff8c35b5cdcab123ad5d68154302a7478b3..11065da8d800021a50cb2a91996df2f8d0e67310` — PASS.
- Checked for `wasm-tools` and `wit-bindgen`; neither is available.

No broad workspace build was run.

## 7. Traceability and status

Ledger source hashes/current generation pass `ledger.py check`. UX-012 and EXT-001 remain `IN_PROGRESS`; EXT-002 and SEC-002 remain `NOT_STARTED`. `STATUS.md` and the validation note say this independent follow-up is pending, accurately reflecting the candidate state before this report. The validation note records 23 focused tests and keeps WIT compiler validation, sample plugins, runtime enforcement, and Phase 8 exit open.

## 8. Re-review artifact and cleanliness

This worktree was created fresh at the exact candidate commit. Only this re-review report will be committed. No candidate, author worktree, or root checkout implementation files were changed; Cargo build output is ignored under this review worktree's `target/`.

## 9. Findings and residual risk

No findings remain in the bounded parser-boundary/canonical-encoding correction scope. Prior MAJOR raw-byte preflight and MINOR golden-vector findings are resolved. The raw slice is already buffered by its caller, so upstream file/transport limits remain necessary. This approval does not extend to the unimplemented runtime or the broader plugin ecosystem requirements.

## 10. Blockers and limitations

WIT syntax/world validation could not be run because `wasm-tools` and `wit-bindgen` are absent; this remains explicitly open. No WASM component load/invocation, signatures/trust, capability-grant enforcement, filesystem/network mediation, cancellation, atomic output publication, runtime resource enforcement, third-party plugin examples, or project-store integration were reviewed as implemented. These remain genuine plan work, not a blocker to approving this isolated correction.

### Ten-field handoff

1. **Objective/scope:** Re-review the two prior parser/canonicalization findings only.
2. **Requirements:** Raw byte preflight, nesting cap before Serde, strict duplicate/unknown/error handling, fixed v1 bytes and digest, accurate open-gate docs/status.
3. **Base/head:** `14076ff8c35b5cdcab123ad5d68154302a7478b3` → `11065da8d800021a50cb2a91996df2f8d0e67310`.
4. **Paths:** Focused correction and governance paths listed in §4.
5. **Semantics/invariants:** All requested correction semantics verified; see §5.
6. **Checks:** 23 tests, strict Clippy, fmt, ledger, architecture, source inventory, diff checks pass.
7. **Traceability/status:** Hash checks pass; UX-012/EXT-001 in progress; EXT-002/SEC-002 open.
8. **Report commit/cleanliness:** To be recorded after committing only this report.
9. **Findings/residual risks:** No findings in correction scope; callers still bound input before buffering.
10. **Blockers/limitations:** WIT tooling absent; runtime, enforcement, integrations and Phase 8 exit remain open.
