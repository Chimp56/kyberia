# Phase 8 WIT architecture-allowlist follow-up re-review — 2026-09-24

**Disposition: APPROVED.** The prior MINOR architecture-policy finding is resolved. This approval remains limited to WIT syntax/world-resolution validation and its test-only dependency boundary; it does not establish component execution, sandboxing, runtime enforcement, or Phase 8 exit.

## 1. Objective and scope

Re-review the MINOR finding from `docs/reviews/phase8-wit-world-parser-validation-review-20260924.md` against follow-up candidate `43bff46ebae54113ccfc1270d9aea976aee48304` (parent `a29e0aca55dc27419e6afa67295ae5f907859296`). Verify that the test-only `wit-parser` is removed from the production architecture allowlist while retained as an SDK dev-dependency; verify the checker would require review if it became a normal dependency; rerun the focused parser and evidence checks; and confirm this focused edit preserves other architecture policy entries. The `plan.md` SHA-256 is `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`. Codebase-memory graph tools were unavailable; no graph or index-coverage claims are made.

## 2. Requirements and open gates

Scope remains plan §§10.13, 14.7, and 15.6; Phase 8; backlog `UX-012`; and Appendix I `EXT-001`. Keep Phase 8 and its related SDK/runtime/security requirements open. The validation candidate establishes parser syntax and package/world resolution only.

## 3. Base and candidate

- Parent: `a29e0aca55dc27419e6afa67295ae5f907859296`
- Follow-up candidate: `43bff46ebae54113ccfc1270d9aea976aee48304`
- Fresh detached review worktree: `/private/tmp/kyberia-phase8-wit-allowlist-rereview-43bff46-20260924`
- Follow-up author worktree: `/private/tmp/kyberia-phase8-wit-arch-followup-20260924` (not modified)

## 4. Reviewed changes

The candidate changes only `tools/architecture.json`, `STATUS.md`, the Phase 8 validation note, and the corresponding source-qualified ledger records. It removes `wit-parser` from the plugin SDK's generic production `external_dependencies` allowlist; `crates/plugin-sdk/Cargo.toml` remains unchanged with `wit-parser = 0.243.0` under `[dev-dependencies]`. `tools/architecture.py` is unchanged and explicitly skips Cargo metadata dependencies with `kind == "dev"`.

The architecture JSON diff changes only the `kyberia-plugin-sdk` entry, from `serde`, `serde_json`, `sha2`, and `wit-parser` to `serde`, `serde_json`, and `sha2`. Every other policy entry is unchanged. The Phase 0 `png` production allowance is not present in this WIT candidate's parent or head (nor in current main at review time); it belongs to the separate unintegrated Phase 0 candidate and is outside this WIT follow-up. This commit neither adds nor removes a Phase 0 entry. Reconcile that allowance separately if the Phase 0 candidate passes review.

## 5. Policy behavior and dependency invariant

- `wit-parser` remains a development dependency and does not appear on SDK normal dependency edges.
- The SDK's production external allowlist no longer contains `wit-parser`.
- I independently exercised the architecture check in memory with the Cargo metadata dependency kind changed from `dev` to normal: the checker then reports `kyberia-plugin-sdk -> wit-parser: external dependency requires architecture review`. The simulation wrote no repository files.
- The WIT integration test and all three expected world names are unchanged. Nothing in this remediation weakens or expands the WIT runtime claim.

## 6. Independent checks

Run from the fresh review worktree:

- `cargo test -p kyberia-plugin-sdk --locked --offline` — PASS: 23 contract tests and 1 WIT parse/resolution test.
- `cargo clippy -p kyberia-plugin-sdk --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt -p kyberia-plugin-sdk -- --check` — PASS.
- `python3 tools/architecture.py` — PASS.
- In-memory architecture-policy simulation — PASS: dev parser is accepted outside the production allowlist; a normal parser dependency is rejected until explicitly reviewed.
- `python3 tools/source_inventory.py check` — PASS: 525 locked external packages.
- `python3 tools/ledger.py check` — PASS: 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `git diff --check a29e0aca55dc27419e6afa67295ae5f907859296..43bff46ebae54113ccfc1270d9aea976aee48304` — PASS.

## 7. Status and traceability

The follow-up updates `STATUS.md` to record the prior bounded approval and the pending architecture follow-up re-review. It keeps Phase 8 open. Ledger and generated traceability hashes are current. Runtime/binding generation, third-party integrations, sandboxing, enforcement, and Phase 8 exit remain open requirements.

## 8. Review artifact and cleanliness

This detached worktree was clean before adding this report. Cargo output remains in ignored `target/`. Only this report is to be committed; no implementation, author-tree, or main-worktree source files were changed.

## 9. Findings and residual risks

No findings remain in the bounded allowlist remediation. The prior MINOR is resolved: a dev-only dependency is no longer pre-authorized as a production dependency, and the existing checker rejects a simulated promotion to a normal dependency absent an explicit policy change. Residual product obligations remain unchanged and are not approved by this review.

## 10. Blockers and limitations

No blocker prevents approval of this remediation. The checks do not load/invoke a component, generate bindings, demonstrate a sandbox, enforce capability grants or resource declarations, or meet Phase 8 exit criteria. The separate Phase 0 `png` allowlist is outside both WIT branches reviewed here and must be preserved when that independent candidate is later reconciled, if approved.

### Ten-field handoff

1. **Objective/scope:** Re-review the production architecture-allowlist MINOR from the WIT parser-validation increment.
2. **Requirements:** Keep `wit-parser` test-only, remove it from production allowlisting, preserve proof that normal promotion needs policy review, and keep Phase 8 open.
3. **Base/head:** `a29e0aca55dc27419e6afa67295ae5f907859296` → `43bff46ebae54113ccfc1270d9aea976aee48304`.
4. **Reviewed paths:** Four candidate paths: architecture policy, STATUS, validation note, and ledger; manifest/test/checker reviewed for boundary behavior.
5. **Semantics/invariants:** The dev edge is skipped; the parser is not allowlisted for production; simulated normal edge fails architecture check. No other policy entry changed.
6. **Checks:** 24 SDK integration tests, strict Clippy, formatting, architecture, policy simulation, source inventory, ledger, and diff check pass.
7. **Traceability/status:** Hashes pass; Phase 8 and related requirements remain open.
8. **Report commit/cleanliness:** Commit only this report in the isolated review worktree; author and main worktrees untouched.
9. **Findings/residual risks:** Prior MINOR resolved; no remaining finding in this bounded scope.
10. **Blockers/limitations:** No runtime, binding, sandbox, enforcement, or Phase 8 completion claim; Phase 0 `png` allowance remains a separate pending reconciliation item.
