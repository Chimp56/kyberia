# Kyberia implementation status

The authoritative specification is [plan.md](plan.md). The complete requirement
inventory is [TRACEABILITY.md](docs/implementation/TRACEABILITY.md), backed by
[ledger.json](docs/implementation/ledger.json) and the
[execution DAG](docs/implementation/execution-dag.json).

- Current phase: **Phase 0 — Research harness and architecture proof**.
- Current iteration: **4 — Integration audit remediation and architecture proofs**.
- Integration branch: `main`.
- Latest integrated feature source: `c3dd163`, Windows disk-file request admission (integration `2988bc6`, review documentation `1efa558`); native runtime gates remain open.
- Canonical CLI and single-snapshot queries: reviewed sources `57f8129`, `0ab4bc7`; integration `95b2f77`, `d9dfd0c`.
- Latest independently reviewed publication correction: `b77e553`, followed by
  separately approved read-admission tests in `7be5e9a`. See the
  [publication review](docs/reviews/materialized-publication-correction-review.md).
- Product acceptance: **not complete**. There is no complete usable mapper UI;
  Phase 0 exit criteria and Phases 1–8 remain open.

## Completed and reviewed foundation increments

These are implemented prerequisites, not claims that the surrounding complete
product capability is validated.

- Canonical typed identities, physical quantities, geometry/frame, observation,
  timing, covariance, provenance and explicit unknown contracts.
- Pure operation DAG, exact encoding, typed undo/redo and conflict semantics,
  canonical baseline/set identity, causal project materialization, and shared
  cumulative admission/replay/merge/ordering budgets. Merge integration is
  `7440ebd`; [independent review](docs/reviews/budgeted-operation-merge-review.md).
- Explicit immutable canonical baselines, replay-validated transactional project
  publication, exact historical retries, and cumulative verification budgets.
- Reviewed CLI baseline initialization and canonical current/baseline/legacy
  queries from one SQLite snapshot; concurrent-writer, corruption and reopen tests.
- Transactional SQLite metadata, immutable survey snapshots, canonical operation
  persistence/replay, normalized Parquet observation chunks, indexed queries,
  source-bound selection, corruption/recovery checks and non-overwriting exports.
- Point-survey state machine and receipt association that cannot fabricate strict
  measurement timing or pose; native normalized-observation publication and
  bounded collector process supervision.
- Versioned observed-RSSI metric registry, deterministic signal aggregation,
  fundamental RF power arithmetic, Wi-Fi channel geometry and spectral coupling,
  nearest/IDW analysis with unknown gaps, and reproducible analysis manifests.
- Stored observation/survey-to-RSSI analysis, canonical numerically replayed
  renderer scenes, and CLI analysis/scene export preserving numeric values,
  unknown reasons, frame geometry and source binding. Scene adapter integration
  is `6e13258`; CLI integration is `4a55d35`.
- Bounded planar intersection/polygon-boolean/rounded-offset adapter, including
  independently reviewed mixed-scale completeness correction `21b156e8`; provisional current-host
  OpenLayers/custom-WebGL comparison; synthetic RF fixtures; Kismet offline
  metadata/container adapters and bounded authenticated status transport;
  isolated Sionna CPU proof; active-process research proof.
- Source-qualified traceability, dependency direction checks, pinned source/license
  inventories, and reviewed Rust CLI SBOM/advisory tooling.

## Capabilities in progress

| Work | Owner / isolated worktree | Acceptance still required |
| --- | --- | --- |
| Neutral planning interchange | Reviewed source `3604e40` | Original schema proof passes 13 focused tests and independent review; external planner bridge, maintainer RFC and runtime round trips remain open |
| Canonical scene renderer input | Laplace correcting / `renderer-canonical-scenes` | Independent review of `1af1cba` requires valid PointValue/Nearest handling, mandatory Rust admission and worker deadlines; [findings](docs/reviews/renderer-wasm-independent-review.md) |
| Native capture session boundary | Integrated `1fb8d12` + `ecf9b59` | Reviewed shared normalization; parser fixture timing correction `73111b0`; full regression and remaining timing diagnostics under validation |
| Unassociated acquisition spool | Root correcting / `acquisition-spool` | Review requires durable session identity for empty captures; explicit terminal outcomes and early cancellation are implemented in the candidate, with fresh review pending |
| Hosted Rust diagnostics | Integrated `2452791` + `c9b4bee` | Hosted annotations now identify Windows unused argv and macOS descendant timing failure; reviewed Windows correction integrated as `00ca425`, timing correction in independent work |
| Windows native request runtime | CI / integrated `2988bc6` | Independently approved disk-handle/reparse/sharing boundary; native disk/device/pipe and full CLI execution remain required |


The publication review is approved for the bounded increment in
[its review packet](docs/reviews/materialized-publication-integration-review.md).
An active draft or passing author test is not integration approval.

## Current validation

The integrated neutral planning schema proof passes the complete Python suite:
**215 tests run, zero failures, 19 skipped**. Log:
`.tools/interchange-integrated-python.log`. Its [independent review](docs/reviews/planning-interchange-integration-review.md) approves only the bounded original schema proof.

After normalization and parser-test integration at `73111b0`, the full Rust
workspace suite passes **634 tests, zero failures, nine ignored**. Log:
`.tools/normalization-final-workspace.log`. Affected Clippy and traceability
checks pass. This local result does not resolve the hosted CI diagnostics.

Complete `.tools/venv/bin/python tools/dev.py check` passes after Windows integration:
Rust tests, workspace lint/typecheck, 189 Python tests run (19 skipped), source
inventory, traceability and original scientific fixtures. Log:
`.tools/windows-integrated-complete-check.log`. Native Windows runtime remains open.

Corrected polygon offsets also pass [ten native/WASM execution cases](docs/validation/offset-wasm-21b156e8.json),
including rejection of mixed-scale partial loss at two origins; broader Gate E remains open.

After offset integration, workspace tests and Clippy pass; logs are
`.tools/offset-integrated-workspace-tests.log` and `.tools/offset-integrated-clippy.log`.

Canonical CLI workspace regression log: `.tools/canonical-cli-integration-network-tests.log`.
The initial sandboxed run could not bind local Kismet HTTP fixtures; rerunning
with loopback access passed. Workspace Clippy also passed.

The Rust scene validator passed native/WASM execution for the canonical fixture
and rejection of trailing whitespace and duplicate schema keys;
[three-case evidence](docs/validation/scene-wasm-6e1e3c5.json). This supports
browser reuse of core validation, not full renderer acceptance.

CI stage diagnostics (`5c5ffc1`, reviewed source `a979d3c`) preserve the same
ordered checks while exposing lint, typecheck, regression, source inventory and
evidence failures separately. Independent review found no blocking findings;
the command-discoverability follow-up is included in README.

For integrated source `1efa558`, `cargo test --workspace --locked --offline` passes **632 tests,
zero failures, nine ignored tests**, recorded in
`.tools/publication-final-integration.log`. Existing local TCP fixtures ran
with authorized listener access. This count excludes unintegrated worktrees.

The publication candidate passes project-store all-target Clippy with `-D warnings`,
`cargo fmt --all -- --check`, `python3 tools/architecture.py check`,
`.tools/venv/bin/python tools/source_inventory.py check` (241 locked packages),
and `.tools/venv/bin/python tools/ledger.py check` (5,392 source blocks,
438 explicit ID occurrences and 446 headings). The prior complete `python3 tools/dev.py check` at the scene-CLI checkpoint passes workspace lint and
typecheck, the Rust suite, 187 Python tests (19 skipped), source/ledger checks
and synthetic fixture verification; log: `.tools/post-scene-complete-check.log`.
These checks do not replace browser, native hardware, cross-platform, Sionna or
Kismet runtime gates.

Native CI has passed the complete workspace build on Windows, macOS and Linux
for `d5050f3`. Complete macOS and Linux validation passed; Windows validation
failed and its detailed cause remains under investigation. See the
[CI checkpoint](docs/validation/ci-publication-checkpoint.md).

Latest hosted run at `9952691` passes Linux but fails Windows workspace Clippy
and macOS Rust regression tests. Public annotations identify the commands and
exit 101, but not the underlying diagnostics. Both failures remain under
investigation; local passing checks do not close these hosted gates.

## Blocked capabilities and technical debt

No entire capability is classified externally blocked. Requirement-specific
external evidence belongs in [BLOCKERS.md](docs/implementation/BLOCKERS.md).
Hardware, credentials and unavailable OS execution must be distinguished from
runnable implementation and fixture work.

Remaining work includes product capture/point-survey wiring and acquisition
spooling; final renderer choice, Tauri/WebView and broader host/driver tests;
remaining Windows publication runtime validation; geometry imports/3D/CRS/CAD and broader offset numerical/platform validation;
complete metric families, regulatory/PHY/airtime/capacity semantics; live Kismet
and PCAPNG normalization parity; full Sionna runtime/calibration gates; storage
live-load/failure/fuzz/portability gates; authorization/coordinator/conflict UX;
and supply-chain coverage for every distributable environment. The legacy
project-receipt replay path still has quadratic growth. Resource byte counters
are deterministic proxies, not resident-memory guarantees. Source requirement
IDs must remain section-qualified.

## Next executable work

1. Complete the active isolated increments, independently review them,
   address findings, integrate and run affected plus workspace regression suites.
2. Wire verified stored scenes into the renderer workflow and record actual
   interaction evidence without silently selecting or promoting a renderer.
3. Connect the reviewed native capture and point-survey boundaries into product
   commands and UX while preserving explicit identity, privacy and capabilities.
4. Close remaining Phase 0 architecture gates using measured evidence, then
   deliver and audit the usable Phase 1 mapper before advancing its delivery gate.

Earlier detailed checkpoints and validation histories are retained in
[the historical snapshot](docs/implementation/history/status-4a55d35.md) and Git.
Tests and trash artifacts remain retained for manual cleanup. Agents use isolated
worktrees, and no author is the sole reviewer of a meaningful feature.
