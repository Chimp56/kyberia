# Independent observation V2 migration review

Reviewer: `/root`. Author: `/root/domain`. Decision: **APPROVED** for the versioned canonical observation and point-snapshot decoding increment. No unresolved BLOCKER or MAJOR finding remains.

## 1. Scope completed

Read all changed implementation, tests, fixtures and ADR 0003 against plan canonical ownership, unknown provenance and migrations (§§10–11 and Appendix I). Re-ran the author suites and independently compiled adversarial decoder probes. This review does not validate storage migration orchestration or native/Kismet normalization.

## 2. Files reviewed

Author commit `b750a55` contains the reviewed implementation. Exact content:

| File | SHA-256 |
|---|---|
| `crates/domain/src/observation.rs` | `d0674a0411a6f090051cd4ae1101b637eb3df377b775d8a54a0d8a1be8aed2ad` |
| `crates/domain/src/observation/wire.rs` | `a1c7fca68117e203e1618d2e55b2b9409202dabf1bf553c50114d16c7b5eb017` |
| `crates/domain/tests/contracts.rs` | `07086c598462a8f3f317a6e16456fa21a40f6bd73df019201655da3d8cf3433d` |
| `crates/domain/tests/fixtures/observation-v1.json` | `d8eac4c4b797644bf257fcf9d5df3babe93ea4e7b7ceef1815691ab5c2b9c8ec` |
| `crates/domain/tests/observation_migration.rs` | `350ea6fd9b2aa7d9c457140dcfa8aa5300811840a76d955ad995257a8138fb55` |
| `crates/survey/src/state.rs` | `fd31cd48649f2db9a9b51c656ce0b3e8c50db68bf80822e1e45e8739d29093f8` |
| `crates/survey/src/state/wire.rs` | `d3861493363962ea3facbabfdab7f557a3cef7d11e0ef24be73b6d77e60248f8` |
| `crates/survey/tests/fixtures/point-observation-v1.json` | `9155d9a7bb2dca5317c26807a57f949dc049ff98641053754edeff11bbfe759b` |
| `crates/survey/tests/fixtures/point-v1.json` | `787b8ae60d0d7cd79a383930648229d39f26de18b52658f01ee0e08fae1bb4e8` |
| `crates/survey/tests/point.rs` | `797dfe08897c7883159e0521bd9b3ea0c1b888cbebbdd5b0a496217c946b972d` |
| `crates/survey/tests/snapshot_migration.rs` | `36b1d2d2809decb97eb2cab410188fd91bd81ec1fe357f7b8db34df5e641509a` |
| `docs/architecture/ADR/0003-versioned-observation-migration.md` | `cc15d99f9a615020f93003cae703095d7247cb5d83bfec1030eef376fd21383f` |
| `docs/architecture/domain-contracts.md` | `dd549e7f430dc734925f242c8c7e4cd3ba3d5747be3f8a4cdca89bde68e81194` |

## 3. Architecture decisions assessed

Only observation envelopes and point snapshots acquire independent V2 tags. Shared project/capability/calibration V1 stays closed. Upstream software version becomes typed known/unknown evidence, separate from source data-format version. Typed legacy decoding checks the enclosing schema against the source-version field shape before calling the same canonical invariant validators. Canonical serialization emits V2 only; original artifact bytes and IDs are preserved by policy. Explicit decoder receipts support outer migration provenance without adding side effects or foreign dependencies inward.

## 4. Tests added and examined

Fifteen migration tests cover exact legacy-field preservation, all unknown reasons, explicit receipt versions, shape/version mismatch rejection, duplicate fields, old and new semantic validation, current-only construction, unrelated schema rejection, actual survey admission of unknown source version and arbitrary-byte property tests. Three original V1 golden fixtures retain the exact SHA-256 values in ADR 0003.

## 5. Independent tests executed

`cargo test --offline` in the isolated domain worktree: PASS, 64 domain/project/survey tests plus seven compile-fail doctests; the explicit benchmark remains ignored during this run. Existing formatting and Clippy results were inspected; integrated formatting/Clippy are required on main before the review is committed.

A separately compiled Rust probe linked against the reviewed crates rejected duplicate nested Evidence `state`/`detail` fields (including reordered fields), explicit null snapshot version and a completed snapshot with all records removed. It also checked exact legacy-to-current observation identity preservation. All probes passed. These probes exercise real typed decoders, not a parallel JSON schema implementation.

## 6. Known limitations

Outer framing must bound bytes/depth before deserializer allocations. Storage must preserve original content, persist decoder receipts, enforce checksums and reconcile referenced observations. A receipt identifies decoder behavior and does not authenticate a source. Historical empty untagged point snapshots are necessarily classified as V1; they still pass complete state/configuration validation.

## 7. Requirements advanced

Unknown upstream version provenance, canonical version isolation, backward reads and explicit migration policy under §§10–11 and Appendix I. This does not complete all foundational schemas or migrations.

## 8. Requirements still open

Disk/project migration orchestration, native and external adapter mapping, richer payload families, general compatibility/export policy and outer import sandbox/resource gates remain separate work.

## 9. Findings and follow-up

**MINOR VM-001 — RESOLVED.** ADR fixture wording implied an established repository redistribution grant. It now records original synthetic provenance and `NOASSERTION`, consistent with the source ledger, pending the distribution decision. ADR status is accepted after this independent review. No code change was required.

No BLOCKER or MAJOR finding. Future source-field additions must retain dedicated version dispatch and must not broaden the shared V1 enum.

## 10. Suggested commit

`feat(domain): migrate observations and survey receipts to version two`
