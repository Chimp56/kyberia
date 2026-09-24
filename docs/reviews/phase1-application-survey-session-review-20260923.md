# Phase 1 application/session point-survey snapshot review

## Review result

APPROVE for this bounded application/session persistence increment. No scoped
blocking or non-blocking correctness findings were identified. This is not an
integration or Phase 1 acceptance decision.

Candidate `a6ee85d0fa098049279c043c7f93925bf3d571f9` was reviewed against
base `afc5070fadac46411f02d355a0d00351f388205f` in an isolated detached review
worktree. The candidate's source-qualified state keeps both the Phase 1
roadmap work and `SUR-001` `IN_PROGRESS`.

## Scoped evidence

- Public session operations accept a typed `PointSurveySnapshotRequest`,
  `SnapshotId`, and optional typed `SessionId`; they return application-owned
  receipt, loaded-view, and history types. `Bundle`, `StoreError`, and
  project-store record types are confined to the private production adapter
  and internal conversion functions. Existing application APIs are additive.
- The save path rejects read-only sessions before reaching the store and
  forwards the optional expected bundle revision to the store's transactional
  optimistic check. Stale revisions map to `ErrorKind::Conflict`; a regression
  test confirms the conflict does not publish a new revision. Negative commit
  times are rejected at the application boundary.
- Load forwards the optional expected survey-session identity. The known
  unknown-snapshot and session-mismatch store failures map to
  `InvalidRequest`; the integration tests cover both, plus successful
  read-only reopen/load and decoder receipt preservation.
- History accepts an optional survey-session filter. The underlying store
  validates the complete bounded inventory and replays evidence before
  filtering; the application projects results into typed history entries.
  Tests cover each of two sessions and the unfiltered ordered history.
- The integration fixture is parsed into a validated completed `PointSurvey`
  and is written to, reopened from, and loaded through a real retained bundle.
  Test directories are kept under `.trash/test-runs/`; no cleanup is performed.
- The `Cargo.lock` candidate diff is exactly one inserted `kyberia-survey`
  dependency line. Its SHA-256 is
  `7f893f1b84073058a5f3363ac80741a1795da443a8a1cd3b9257de0b2d99660c`,
  matching `docs/licenses/cargo-sources.json`. The plan SHA in generated
  traceability matches the candidate plan digest. Ledger check validates the
  generated source-qualified inventory and hashes.
- `STATUS.md` and the ledger explicitly state that snapshot persistence is a
  foundation only: capture orchestration, manual continuous-survey support,
  desktop flow, and Phase 1 acceptance remain open. Neither Phase 1 nor
  `SUR-001` is marked complete.

## Validation

Run in the isolated review worktree with locked dependencies offline and build
outputs/cache kept under that worktree:

- `cargo test --locked --offline -p kyberia-application -p kyberia-project-store -p kyberia-survey` — PASS, exit 0.
- `cargo clippy --locked --offline -p kyberia-application --all-targets -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS: 5,396 source blocks, 438 explicit-ID occurrences, 447 headings.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS: all 522 locked external packages represented.
- Candidate `git diff --check` — PASS; `Cargo.lock` changed by one insertion.

The codebase-memory MCP graph/index tools were unavailable for this review;
the review used targeted source inspection and tests only and makes no graph
coverage or source-completeness claim.

## Residual boundaries

The history operation intentionally inherits the store's finite snapshot and
artifact bounds and full-inventory validation behavior. This review did not
establish desktop/IPC behavior, live capture orchestration, continuous survey
support, Phase 1 exit criteria, or product-level acceptance. No UI or capture
integration is claimed.

## Ten-field handoff

1. **Verdict:** APPROVE this bounded candidate; no scoped correctness finding.
2. **Candidate/base:** `a6ee85d0fa098049279c043c7f93925bf3d571f9` /
   `afc5070fadac46411f02d355a0d00351f388205f`.
3. **Plan scope:** Application/session snapshot save/load/history for point
   surveys; Phase 1 and `SUR-001` remain `IN_PROGRESS`.
4. **Public boundary:** Additive typed API; no public Bundle/store adapter or
   project-store record/error leakage found.
5. **Write semantics:** Read-only guard, negative timestamp rejection, and
   transactional expected-revision conflict mapping verified.
6. **Read/history semantics:** Expected-session check and unknown-ID mapping
   verified; filtered/unfiltered history validates and preserves commit order.
7. **Fixture/provenance:** Valid completed survey round-trips through retained
   real bundles; one-line lock delta and generated lock/plan digests match.
8. **Validation:** Focused locked/offline tests, strict Clippy, fmt, ledger,
   architecture, source inventory, and diff checks all pass.
9. **Limits/blockers:** No blocker. Graph tools unavailable; no completeness
   claim. No capture/UI/desktop wiring or Phase 1 acceptance established.
10. **Reviewer tree/report:** Dedicated detached review worktree
    `/private/tmp/kyberia-phase1-application-review-current-20260923`; this
    report is the only tracked change. Commit and final cleanliness are
    recorded in the parent handoff.
