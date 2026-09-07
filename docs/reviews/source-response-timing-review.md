# Source response timing: independent architecture/domain review

Review date: 2026-09-07. Reviewer: independent Sionna integration agent; implementation author: principal agent. Decision: **APPROVE** the reviewed domain contract and ADR-0005 decision. No BLOCKER, MAJOR or MINOR findings remain. This approval does not validate native acquisition, storage integration or a usable point survey.

## 1. Scope

Review against plan §7 evidence semantics and §10 inward contracts: source result emission, API-call windows, monotonic clock epochs, unknown evidence, strict decoding and compatibility with the existing V2 observation envelope. The review used a separate `review/source-response-timing` worktree based on `24c2fe5`; the frozen implementation was read from the main worktree without editing it.

## 2. Files and immutable review inputs

SHA-256 of the main-worktree files reviewed and tested:

| File | SHA-256 |
| --- | --- |
| `crates/domain/src/observation/reception.rs` | `05f6f2375eb09f46c5492b3b064269a922da1a60566c0c6308f18cd0e08372a5` |
| `crates/domain/tests/reception.rs` | `76e363b7d071f3f6701ba8b7a7b366583b6329a074b94333e01b44e2b38fc9ef` |
| `crates/domain/src/observation.rs` | `bf81161a1076f6858aed74979f62fa2284938df97beafc8070a7aff1622dec59` |
| `docs/architecture/ADR/0005-source-response-timing.md` (accepted status) | `09ec3562a92ba2eb88f8e4b05f99daba1f44c6c0f7bde9463aa6438328cf4ef3` |

The ADR was reviewed with status Proposed (SHA-256 `0d4bd9b3a3474dba333c890393062b0a561faf96cf0533334366058d3a7c4c42`). The author then changed only its status to Accepted following this approval; the final status hash is recorded above. Substantive implementation or decision changes require further review. The only tracked file authored by this reviewer is this review record.

## 3. Architecture assessment

`SourceResponseTiming` gives source emission and the API window explicit identities. It neither reads a clock nor substitutes receipt evidence for capture, cache age, dwell or pose. Constructors and serde conversion share the same invariants. Response and API timestamps must agree on epoch and ordering; a supplied synchronization model must agree with each known response/API epoch. Capture is compared with response only when their monotonic epochs match. Cached capture may legitimately precede the API window, and a separate hardware clock is not implicitly aligned.

Private validated fields protect the new contract after construction. Separately versioned `ReceivedObservation` leaves V2 envelopes and existing V1 migration intact. Dependencies remain inward and the domain contains no native capture or transport objects. UTC readings are retained as evidence without inventing an ordering between independent clocks.

## 4. Independent tests added

An ignored local probe crate at `.tools/reception-probe` uses a path dependency on the main domain crate and the existing cached `serde_json` version. Its seven tests cover:

- Backdated response UTC, extreme signed UTC readings and a separate hardware monotonic epoch.
- Exact `u64::MAX` monotonic values, equal endpoints and reversed extreme windows.
- Distinct unknown capture, response and API-window evidence through serialization, without creating pose, dwell or result age.
- Forged nested response/window epochs and reversed response/API timestamps.
- Synchronization-model mismatch when either response or API evidence is unknown, plus a valid model with an extreme reference timestamp.
- Missing and duplicate nested fields, unsupported wrapper versions and forbidden response fields.
- Existing nested V1 envelope decoding and migration to V2 without changing wrapper version.

Probe source SHA-256: `22948bc9d9492571411eb62a3b9d7796f1974c49fd3a831b0a42f45800e44718`. The probe and its Cargo lock/build outputs remain ignored; they are not product code or portable checked-in test coverage.

## 5. Executed results

Commands executed from `.worktrees/reception-review`:

```text
cargo test --locked --manifest-path /Users/vincent/code/kyberia/Cargo.toml --target-dir .tools/reception-target -p kyberia-domain --test reception
cargo test --offline --manifest-path .tools/reception-probe/Cargo.toml
```

The authored reception suite passed 6/6 tests. Independent probes passed 7/7 tests; probe doc tests passed with zero tests. No test failures or unresolved findings occurred. Workspace checks and Clippy reported by the author were not independently rerun in this bounded review.

## 6. Limits

This is a domain contract review using deterministic constructed and legacy fixture evidence. It does not demonstrate native CoreWLAN timing accuracy, host ingestion semantics, durable storage roundtrip, calibration or live point-survey behavior. It does not assign positional validity using source receipt time.

## 7. Requirements satisfied

The reviewed contract preserves available source-response evidence and absent RF evidence separately, performs only explicitly justified monotonic comparisons, validates decoder-created values, retains exact integer timestamps, preserves unknowns and keeps observation migration independent. The six durable tests and seven independent probes support these claims.

## 8. Open integration gates

Native adapter normalization against the original source fixture, storage roundtrip of the canonical pair, and any position-assignment policy require separate validation. The strict point-survey requirement for actual capture evidence remains unchanged. These are downstream gates, not defects in this bounded contract.

## 9. Risks

Adapters must not relabel host ingestion or API completion as source emission unless their source actually defines it that way. A shared epoch identifier asserts a shared clock domain and must be truthful. This contract validates that assertion's internal consistency; it cannot independently prove the clocks' origin. Consumers must retain the wrapper when source-response provenance matters rather than dropping it while storing only the envelope.

## 10. Suggested review-only commit

`docs(review): approve source response timing domain contract`

The implementation author may accept ADR-0005 and integrate the reviewed domain files after this independent approval. This review document is committed separately with principal approval.
