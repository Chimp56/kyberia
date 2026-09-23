# Phase 8 plugin SDK contract review

**Disposition: CHANGES REQUIRED** — one MAJOR finding must be resolved before this contract proof is promoted. The declaration-only boundary is otherwise clearly documented, the role/capability checks are coherent, and the focused checks pass.

## 1. Objective and scope

Independently review candidate `14076ff8c35b5cdcab123ad5d68154302a7478b3` for the Phase 8 plugin contract foundation: `backlog:UX-012:1` and `audit:EXT-001:1`, with `audit:EXT-002:1` and `audit:SEC-002:1` remaining open. I read the complete authoritative `plan.md` and repository `AGENTS.md` before reaching architectural conclusions. The requested codebase graph/index tools were unavailable in this context; this review makes no graph or index-coverage claims and uses bounded source/configuration inspection instead.

## 2. Plan and traceability records

Relevant plan scope: §§10.13, 14.7, 15.6; Phase 8; backlog row UX-012; Appendix I rows EXT-001, EXT-002, SEC-002. The candidate correctly limits its claim to portable declarations and validation. The ledger marks UX-012 and EXT-001 `IN_PROGRESS`; EXT-002 and SEC-002 remain `NOT_STARTED`. `STATUS.md` likewise says independent review, WIT compilation, sample plugins, runtime invocation, enforcement, sandboxing, and project integration remain open.

## 3. Base and candidate

- Base: `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`
- Candidate: `14076ff8c35b5cdcab123ad5d68154302a7478b3`
- Review worktree: `/private/tmp/kyberia-phase8-plugin-sdk-current-review-20260923`
- Candidate author worktree was separate; this review changed no implementation files.

## 4. Reviewed paths

Reviewed the complete candidate diff: `crates/plugin-sdk/{Cargo.toml,src/lib.rs,src/model.rs,src/registry.rs,src/validation.rs,tests/contracts.rs}`, `schemas/plugin-wit/worlds.wit`, `docs/architecture/plugin-sdk-contracts.md`, `docs/validation/phase-eight-plugin-sdk-contracts.md`, `STATUS.md`, `docs/implementation/{TRACEABILITY.md,execution-dag.json,ledger.json}`, `docs/licenses/cargo-sources.json`, `tools/architecture.json`, `Cargo.lock`, and the complete plan/instructions.

## 5. Semantics and invariants

- The closed Rust enums reject unknown capability and contract identifiers; role validation requires the exact input/output pair and ordered capability vocabulary for collector, metric, and export roles.
- Compatibility ranges are half-open and negotiation records the host-offered version only when it lies within the plugin's declared range. Duplicate host offers are rejected.
- Component length/SHA-256 verification is separate from manifest negotiation. The plugin reference binds the canonical manifest and component identity; the in-memory registry is deterministic, rejects duplicate IDs and resolves exact references.
- Resource requests are declarations checked against host-advertised ceilings only. The crate explicitly does not load components, make trust decisions, enforce grants/limits, or implement a runtime; documentation and status do not overclaim these behaviors.
- The WIT worlds expose role-specific host interfaces and version-tagged payload envelopes. Since the WIT source was not compiler-checked here, its syntax/ABI correspondence is not independently established.

## 6. Findings

### MAJOR — manifest byte ceiling is applied after deserialization

`validate_manifest` accepts `&PluginManifest`, so callers must deserialize JSON before invoking it (`crates/plugin-sdk/src/validation.rs:84-90`). Its manifest-size check serializes the already-built value and compares that canonical size to `host.max_manifest_bytes` (`validation.rs:231-236`). There is no SDK entry point that checks the original wire-byte length before deserializing. Consequently this limit cannot bound parser allocation/work for untrusted manifests; even whitespace-heavy input can exceed the declared wire size while its re-serialized representation stays below the limit. The tests exercise a too-small host limit on an already-constructed Rust value and use `serde_json::from_value` directly for strict-field checks, so they do not cover a bounded parsing boundary.

Add a public bounded JSON parse/admission entry point that checks the original byte slice against the host's byte ceiling *before* invoking Serde, preserves strict unknown/duplicate-field behavior and a documented depth bound, then performs the existing structural and compatibility validation. Add tests proving oversized raw input is rejected before parsing (including whitespace and large-field/array cases), as well as malformed, duplicate, unknown, and excessive-depth inputs. If parsing is intentionally outside this crate, rename/document the current check as canonical in-memory size validation and require/capture the raw-size preflight at every trust boundary; do not imply that this validator bounds untrusted parsing.

### MINOR — canonical-reference tests do not pin a golden encoding

`canonical_manifest_encoding_is_repeatable` compares two calls in the same build, but does not assert fixed canonical bytes or a fixed project-reference digest. These values are persisted and described as stable; a serializer/field-order change could therefore alter identities without a regression test. Add a version-1 golden manifest byte sequence and expected reference digest, and retain it as a compatibility fixture.

## 7. Test and check results

Independently run in the assigned review worktree:

- `cargo test -p kyberia-plugin-sdk --locked --offline` — PASS, 15 contract tests; unit/doc targets also pass (0 tests).
- `cargo clippy -p kyberia-plugin-sdk --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt -p kyberia-plugin-sdk -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS (5,396 source blocks, 438 explicit ID occurrences, 447 headings).
- `python3 tools/architecture.py` — PASS (dependency direction and external-package boundaries).
- `python3 tools/source_inventory.py check` — PASS (522 locked external packages).
- `git diff --check 05953134d24666e8483cbfdb7d9aacd0ce4e6e48..14076ff8c35b5cdcab123ad5d68154302a7478b3` — PASS.

No broad workspace build was run. No parser-bound test currently exists, which is the MAJOR finding above.

## 8. Traceability and status

Ledger hashes and current source-qualified evidence pass the repository checker. UX-012 and EXT-001 correctly remain `IN_PROGRESS`; the candidate validation document accurately labels this as an unintegrated candidate and does not claim Phase 8 exit. EXT-002 and SEC-002 remain open. After this report is integrated, update the status/ledger review record with the finding disposition; do not promote the plugin feature until the MAJOR finding is resolved and independently re-reviewed.

## 9. Review artifact and cleanliness

This commit contains only this review report. The review worktree was clean before report creation; generated Cargo output is ignored under this worktree's `target/`. No implementation, author worktree, or root checkout files were changed.

## 10. Residual risks and blockers

`wasm-tools` and `wit-bindgen` are not installed in the review environment, so WIT parsing/component-world validation could not be run; this matches the candidate's recorded limitation and remains a follow-up. No plugin runtime, signature/trust policy, capability-grant enforcement, filesystem/network mediation, cancellation, atomic output publication, resource enforcement, sample third-party integrations, or project-store integration was tested or is claimed. These are plan work remaining, not evidence that the current declaration-only code implements them.

### Ten-field handoff

1. **Objective/scope:** Independent Phase 8 declaration-contract review.
2. **Plan/records:** §§10.13, 14.7, 15.6, Phase 8, UX-012, EXT-001/002, SEC-002.
3. **Base/head:** `05953134d24666e8483cbfdb7d9aacd0ce4e6e48` → `14076ff8c35b5cdcab123ad5d68154302a7478b3`.
4. **Reviewed paths:** Full candidate diff listed in §4.
5. **Semantics/invariants:** Role/capability contracts, negotiation, component/reference binding, registry, WIT boundary summarized in §5.
6. **Checks:** 15 tests, strict Clippy, fmt, ledger, architecture, inventory, and diff checks pass; see §7.
7. **Traceability/status:** Ledger current; UX-012/EXT-001 in progress; EXT-002/SEC-002 open.
8. **Review report commit/cleanliness:** To be recorded in the handoff after committing this report only.
9. **Findings/residual risk:** One MAJOR pre-deserialization size-bound gap and one MINOR missing golden encoding vector.
10. **Blockers/limitations:** WIT compiler tools unavailable; runtime and enforcement explicitly out of candidate scope.
