# Plan progress review — 2026-09-12

Reviewed integration snapshot: `fca5b3fd7ad3bbd9e8542093e829fe47c24f8f2a`.
Specification: complete `plan.md`, SHA-256
`57b6bb686456a567f882a8e4a1eec7bcdcfafea843dd060f224e76c7122328ea`.
This is a progress audit, not final acceptance or approval of unfinished branches.

## Assessment

Kyberia remains in Phase 0, iteration 4. Substantial foundations and bounded
research proofs exist, but no delivery phase has passed its full acceptance
criteria. The Phase 1 home dead-spot mapper is not usable: `apps/` contains only
the CLI, and main has no application crate or desktop shell. Phases 2–8 have
some reusable prerequisites, not completed product workflows.

Integrated foundations include typed domain/provenance contracts, operation
history and causal materialization, transactional project storage, Parquet
observation chunks, point-survey and capture normalization boundaries, stored
RSSI analysis, numerical scene export and a Rust/WASM-validated research
renderer. Geometry, active measurements, Kismet and Sionna have bounded proofs.
These do not satisfy the user workflow, field validation, reporting and runtime
requirements of the surrounding capabilities.

## Findings

1. **MAJOR — Product integration remains missing.** Plan §17 Phase 1 and §23
   require launch/import/calibrate/survey/analyze/repeat workflows with numeric
   provenance. CLI operations and research rendering do not implement that
   workflow. The isolated `application-project-session` tree is clean at
   `966f92a`; its assignment has not produced source changes.
2. **MAJOR — Windows regression is failing.** Hosted run
   [34350467190](https://github.com/Chimp56/kyberia/actions/runs/34350467190)
   at `966f92a` passed Ubuntu and macOS but failed the Windows Rust/Python
   regression step. Public annotations identify eight Sionna lifecycle tests,
   `test_active_process.ContractTests.test_zombie_group_permission_error_requires_post_reap_absence`,
   and `test_supply_chain.SupplyChainTests.test_workspace_bom_refs_are_portable`.
   Full tracebacks were not obtained; the annotations establish failing tests,
   not their complete causes. Windows build, lint and typecheck passed.
   A newer run, [34726569253](https://github.com/Chimp56/kyberia/actions/runs/34726569253)
   at `fca5b3f`, was in progress during the final metadata check; it is not
   credited as passing.
3. **MAJOR — Capture persistence is not ready to integrate.** The
   [independent review](capture-session-record-independent-review.md) requests
   corrections to terminal/partial semantics and allocation preflight.
   `capture-session-record` and `acquisition-spool` retain uncommitted changes;
   neither is credited as integrated or approved. A test-reliability correction
   also remains uncommitted in `active-process-test-reliability`.
4. **MAJOR — Architecture/runtime proofs remain narrower than their gates.**
   Renderer Gate B remains provisional without native desktop acceptance;
   storage and geometry gates retain broader workload/platform work. Kismet
   status/offline adapters do not establish live local/remote capture parity.
   Sionna's reviewed empty-space CPU profile does not implement material,
   reflection, diffraction or calibrated P2/P3 behavior. See ADRs 0004, 0006,
   0007, 0008, 0010 and 0019.
5. **MAJOR — Barrier-aware IDW remains missing.** Phase 1 requires it at
   `plan.md:4029`. The current spatial model uses Euclidean distances and
   nearest/plain IDW (`crates/spatial-analysis/src/model.rs:191` and `:258`).
   `docs/architecture/spatial-analysis.md` explicitly retains wall-aware
   support as planned. Existing geometry intersections alone do not satisfy
   this numerical requirement.
6. **MAJOR — Windows native collector supervision remains unsupported.**
   `crates/observation-pipeline/src/process.rs:816` returns
   `UnsupportedPlatform` on non-Unix targets; the native collector build under
   `collectors/macos/` is macOS-only. This is a product platform implementation
   gap independent of the hosted regression failures above.
7. **MINOR — Tracking needs reconciliation.** The status narrative contains
   superseded CI checkpoints. All nine roadmap heading records remain
   `NOT_STARTED`, including Phase 0, despite tracked foundational work. The
   ledger preserves source coverage but does not provide a reliable percentage
   of product completion; status/evidence mapping requires further reconciliation.

Independent reviewer `/root/progress_audit` (Luna, xhigh) read the complete plan
and inspected source without authoring changes. The reviewer agreed with the
bounded assessment, identified findings 5 and 6, and requested current Git
status clarification. Those findings are incorporated here. This review does
not approve any unfinished implementation branch.

## Validation evidence

Fresh audit commands passed: `.tools/venv/bin/python tools/ledger.py check`,
`.tools/venv/bin/python tools/architecture.py check`, and
`.tools/venv/bin/python tools/source_inventory.py check`.
The source inventory covers 241 locked external packages. No complete test
suite was rerun for this read-only source audit.

Retained integrated check log `.tools/post-renderer-workspace-check.log` records
635 Rust passes, zero failures, nine ignored; 215 Python tests, 19 skipped, no
failures; formatting/lint/typecheck and evidence checks passed. The subsequent
Python diagnostics integration rerun records 225 tests, 19 skipped, no failures
in `.tools/python-diagnostics-integrated-retry.log`. Its first run had an active
process timeout assertion failure, retained in the corresponding initial log;
a passing retry does not establish the cause or fix that reliability defect.

The ledger has 3,335 leaf obligation records: 64 `VALIDATED`, 75 `IN_PROGRESS`,
3,196 `NOT_STARTED`, and zero `IMPLEMENTED`, `BLOCKED_EXTERNAL` or
`DEFERRED_BY_ADR`. These are source-derived records of unequal scope, not feature
counts or a completion percentage. Inventory validation covers 5,392 source
blocks, 438 explicit ID occurrences and 446 headings; the execution DAG has
55 nodes.

## Next dependencies

Correct and independently review the Windows failures and pending capture
increments. Implement the application create/open/query boundary using existing
canonical storage, then connect real capture, calibration, point survey and
stored analysis to the desktop workflow. Implement barrier-aware interpolation
with numerical acceptance tests. Resolve the
Windows collector execution boundary. Finish the remaining architecture gates
with explicit evidence and audit Phase 1 usability before claiming delivery.
Keep hardware-only runtime gates separate while completing their runnable
surrounding work. Missing product code is not an external blocker.

At inspection, main was one commit ahead of the locally tracked `origin/main`;
tracked files were clean, with untracked `.pnpm-store/` and `package.json`
preserved. Isolated unfinished changes were left intact. No files were deleted.
Before the report commit, `origin/main` advanced to the same `fca5b3f` snapshot;
main's source did not change during the audit.
