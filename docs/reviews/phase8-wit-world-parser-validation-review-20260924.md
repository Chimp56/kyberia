# Phase 8 WIT world parser-validation review — 2026-09-24

**Disposition: APPROVED WITH MINOR FOLLOW-UP for the bounded WIT syntax and world-resolution validation increment.** The current parser dependency is test-only and there is no runtime dependency defect. One MINOR architecture-policy hygiene finding is recorded below. This approval does not establish component execution, sandboxing, runtime enforcement, or Phase 8 exit.

## 1. Objective and scope

Independently review the WIT validation candidate at `a29e0aca55dc27419e6afa67295ae5f907859296` against base `7a0b1e74c1e6f805b4848de457b6a537b6d012c7`. The bounded questions were whether the checked-in WIT parses and resolves its intended worlds, whether changing the reserved world name `export` to `exporter` leaves stale references, whether the parser is test-only, and whether dependency, license, inventory, status, and source-qualified ledger records are accurate. The authoritative `plan.md` SHA-256 is `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`. Codebase-memory graph tools were unavailable in this review context; no graph/index-coverage claims are made.

## 2. Plan and traceability scope

Relevant requirements are plan §§10.13, 14.7, and 15.6; Phase 8; backlog `UX-012`; and Appendix I `EXT-001`, with `EXT-002` and `SEC-002` still open. The candidate and ledger keep the plugin SDK in progress. Its status and validation notes distinguish parser syntax/world-resolution checks from binding generation and runtime behavior.

## 3. Base and candidate

- Base: `7a0b1e74c1e6f805b4848de457b6a537b6d012c7`
- Candidate: `a29e0aca55dc27419e6afa67295ae5f907859296`
- Fresh detached review worktree: `/private/tmp/kyberia-phase8-wit-validation-review-a29e0ac`
- Author worktree: `/private/tmp/kyberia-phase8-wit-validation-20260924` (not modified)

## 4. Reviewed paths

Reviewed `schemas/plugin-wit/worlds.wit`, `crates/plugin-sdk/Cargo.toml`, `crates/plugin-sdk/tests/wit_worlds.rs`, `Cargo.lock`, `tools/architecture.json`, `docs/licenses/cargo-sources.json`, `docs/architecture/plugin-sdk-contracts.md`, `docs/validation/phase-eight-plugin-sdk-contracts.md`, `STATUS.md`, `docs/implementation/ledger.json`, and the corresponding traceability rows. Searched the repository for uses of the old world name.

## 5. Semantics and workspace implications

- `Resolve::push_path` parses the checked-in WIT package; the test asserts the exact world-name set `{collector, metric, exporter}` and calls `select_world` for each name. This exercises both parsing and resolver selection rather than merely checking text or invoking a parser on an unrelated fixture.
- `export` is a WIT reserved keyword, so the old declaration was invalid. Renaming it to `exporter` is consistent with the parser's grammar. No source or schema references to a world named `export` remain; role/interface identifiers (`export-host`, `export-plugin`) are unchanged.
- `wit-parser = 0.243.0` is declared under `[dev-dependencies]`. An independent `cargo tree -p kyberia-plugin-sdk --edges normal --locked --offline` confirms it is absent from normal/runtime dependency edges. It therefore adds test/build weight and lockfile state, not a parser dependency to shipped workspace targets.
- Architecture validation passes. The checker deliberately skips `kind == "dev"` dependencies when enforcing production dependency direction. The candidate nevertheless adds `wit-parser` to the SDK's undifferentiated `external_dependencies` allowlist. This does not create a runtime edge, but is unnecessary for the dev dependency and weakens the allowlist as a guard against accidentally promoting the parser to a normal dependency; see the MINOR finding in §9.
- The lockfile, Cargo source inventory, and package metadata agree on the new locked packages. Inventory checking verifies each cached crate archive against its Cargo.lock checksum and requires declared license metadata. The recorded `wit-parser`, `id-arena`, and `unicode-xid` versions, repositories, declared licenses, and archive hashes match Cargo metadata and the lock.
- The source-qualified ledger and generated traceability now include the parser test and its validation evidence; ledger checking confirms all recorded source hashes and current coverage generation.

## 6. Independent checks

Run from the fresh review worktree:

- `cargo test -p kyberia-plugin-sdk --locked --offline` — PASS: 23 existing contract tests plus the WIT parse/resolution test.
- `cargo clippy -p kyberia-plugin-sdk --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt -p kyberia-plugin-sdk -- --check` — PASS.
- `cargo tree -p kyberia-plugin-sdk --edges normal --locked --offline` — PASS; `wit-parser` does not appear in normal dependency edges.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS: 525 locked external packages; archive checksums and license metadata validated.
- `python3 tools/ledger.py check` — PASS: 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `git diff --check 7a0b1e74c1e6f805b4848de457b6a537b6d012c7..a29e0aca55dc27419e6afa67295ae5f907859296` — PASS.

No broad workspace build or component runtime test was run; neither is needed to establish this isolated parser-validation claim.

## 7. Status and evidence limits

`STATUS.md` correctly describes this candidate as awaiting independent review and current-main reconciliation at its own commit. On promotion, update that note to record this report. Keep Phase 8, `UX-012`, and `EXT-001` in progress. The parser test does not generate bindings, load or invoke a component, enforce declared grants or resource limits, or demonstrate third-party plugin integration. `EXT-002`, `SEC-002`, and the broader Phase 8 exit criteria remain open.

## 8. Review artifact and cleanliness

This review worktree was detached at the exact candidate and was clean before this report was added. Cargo output remains in the ignored review-worktree `target/`. Only this report is to be committed; no candidate implementation or author-tree files were changed.

## 9. Findings and residual risks

### MINOR — test-only parser appears in the production architecture allowlist

`tools/architecture.json` lists `wit-parser` in the SDK's generic `external_dependencies`, but `tools/architecture.py` intentionally skips development dependencies when it enforces production dependency policy. The added entry is therefore not needed for the current architecture check. Because that allowlist is also consulted for normal dependencies, leaving the entry would let a future accidental move from `[dev-dependencies]` to `[dependencies]` pass the architecture check without a new policy decision.

Remove `wit-parser` from that production allowlist, or change the policy schema/checker to represent test-only allowances separately. The current `Cargo.toml` placement and normal-edge tree are correct, so this is not a runtime dependency defect and does not block approval of the WIT validation increment.

Residual product risks and obligations are the unimplemented runtime, trust/capability enforcement, resource enforcement, binding generation, sample plugins, and project integration; this report does not approve or claim them.

## 10. Blockers and limitations

No blocker prevents approval of this isolated correction. The executable `wasm-tools`/`wit-bindgen` workflow remains unverified; the pinned offline `wit-parser` test provides syntax and package/world-resolution evidence only. No sandbox, component invocation, signature/trust policy, capability-grant enforcement, filesystem/network mediation, cancellation, atomic output publication, or CPU/memory/time enforcement is implemented or claimed.

### Ten-field handoff

1. **Objective/scope:** Independent review of the Phase 8 WIT parser-validation increment.
2. **Requirements:** Parse the checked-in package, resolve the intended worlds, validate the rename, keep parser test-only, and preserve accurate open-gate records.
3. **Base/head:** `7a0b1e74c1e6f805b4848de457b6a537b6d012c7` → `a29e0aca55dc27419e6afa67295ae5f907859296`.
4. **Paths:** WIT schema, parser integration test, Cargo/architecture/license metadata, STATUS, validation, ledger, and traceability as listed in §4.
5. **Semantics/invariants:** Exact three-world set asserted; every world selected through the resolver; `export` corrected to `exporter`; parser has no normal dependency edge.
6. **Checks:** Focused tests (24 integration tests total), strict Clippy, fmt, normal dependency tree, architecture, 525-package source inventory, ledger, and diff checks pass.
7. **Traceability/status:** Ledger hashes pass; Phase 8/UX-012/EXT-001 remain in progress and broader runtime/security gates remain open.
8. **Report commit/cleanliness:** Commit this report only in the detached review worktree; author and implementation trees untouched.
9. **Findings/residual risks:** One MINOR architecture-allowlist follow-up; no BLOCKER or MAJOR; no runtime or Phase 8 completion is approved.
10. **Blockers/limitations:** Component/binding toolchain, execution, sandboxing, resource enforcement, and integrations remain unvalidated or unimplemented.
