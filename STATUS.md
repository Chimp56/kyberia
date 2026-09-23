# Kyberia implementation status

The authoritative specification is [plan.md](plan.md). The complete requirement
inventory is [TRACEABILITY.md](docs/implementation/TRACEABILITY.md), backed by
[ledger.json](docs/implementation/ledger.json) and the
[execution DAG](docs/implementation/execution-dag.json).

- Current phase: **Phase 0 — Research harness and architecture proof**.
- Current iteration: **6 — Lab validation, desktop shell integration, and hosted Windows closure**.
- Integration branch: `main`.
- Latest reviewed integrated features: the authenticated Kyberia Lab MCP through `964a5ae` with merge correction `eb3574d`; the desktop shell and deterministic cancellation/advisory corrections through `74d0f89`; the Windows loopback fixture correction through `f019928`; safe bootstrap-stage diagnostics through `c4a1219`; WIFI-001 at `1d8e68f`; and bounded map import/PNG admission at `2670207`. Runtime and documentation reviews of the Sionna follow-up `d0a88eb` pass. Final renderer and product gates remain open.
- Most recent integration change: bounded PNG map admission and operation-backed map import at `2670207`, with the PNG-ordering correction independently re-reviewed. This does not close a roadmap phase; map UI/decoder, physical and hosted product gates remain open.
- Canonical CLI and single-snapshot queries: reviewed sources `57f8129`, `0ab4bc7`; integration `95b2f77`, `d9dfd0c`.
- Latest independently reviewed publication correction: `b77e553`, followed by
  separately approved read-admission tests in `7be5e9a`. See the
  [publication review](docs/reviews/materialized-publication-correction-review.md).
- Product acceptance: **not complete**. There is no complete usable mapper UI;
  Phase 0 exit criteria and Phases 1–8 remain open.

## Current execution checkpoint — 2026-09-23

The CPU/LLVM-only Sionna correction is integrated on `main` at `84a5bcb`.
Independent runtime and docs/ledger reviews of follow-up `d0a88eb` pass with no
findings; their scopes and evidence limits are recorded in the
[review artifact](docs/reviews/cpu-only-sionna-removal-review.md). A Lab MCP
test exposed shared-fixture mutation in its portable-`wgpu`
acceptance assertion; the fixture is now isolated and all 33 Lab MCP tests pass
(32 passed, one Windows-only skip) using the already-installed dependencies.
Formatting, TypeScript checks/build, runner build, package inventory and supply-
chain checks also pass when invoked directly; the `pnpm` wrapper could not be
started because Corepack attempted a network fetch for pnpm and DNS is
unavailable. Focused Sionna/research/ledger Python tests pass (117 run, two
skipped), as do ledger, architecture and source-inventory checks. This does not
establish execution of the pinned worker-0.2.0 Sionna engine or close hosted,
physical-radio, or spectrum gates.

The bounded Phase 2 `WIFI-001` parser is integrated on `main` at `1d8e68f`;
promotion validation is recorded at `7d126cc`. Typed-field tcpdump comparison,
deterministic mutation and focused quality gates pass, but this does not close
other Phase 2 items or the product gates.

The bounded PNG admission and operation-backed map import/two-point calibration
increment is integrated on `main` at `2670207`. The broader independent review
and PNG-ordering correction re-review are recorded in
[`map-asset-admission-current-review.md`](docs/reviews/map-asset-admission-current-review.md)
and [`map-asset-order-fix-rereview-20260923.md`](docs/reviews/map-asset-order-fix-rereview-20260923.md);
the identified ordering finding is resolved. This does not close the remaining
import-format catalog, pixel decoding, map UI/desktop workflow, multi-point/CRS
calibration, or the Phase 0 exit criteria.

## Historical execution checkpoint — 2026-09-22

The most recent product/tooling change is `adb2b69`. Hosted run
[`34775283924`](https://github.com/Chimp56/kyberia/actions/runs/34775283924)
confirms that the Windows Corepack startup failure is resolved: the pinned
toolchain bootstrap, frontend dependency install, and Chromium install all
passed on Windows. Ubuntu and macOS passed the complete workflow. Windows now
fails at **Compile foundation and CLI** (exit 101); subsequent Windows gates
were skipped. The public check annotation exposes only the failed stage and
exit code, so the compiler diagnostic and cause are not yet established.

A local `x86_64-pc-windows-gnu` workspace check was attempted, but stopped in
`libsqlite3-sys` because this Mac host lacks `x86_64-w64-mingw32-gcc`. That
cross-check does not explain the hosted Windows MSVC failure and is not treated
as an external blocker. The Windows compile failure remains actionable. On
this main revision, the 11 developer-command regressions, architecture check,
ledger check (5,392 source blocks), and 522-package source-inventory check all
pass locally. The current traceability matrix records 66 VALIDATED, 88
IN_PROGRESS, and 3,181 NOT_STARTED leaf obligations; these counts are source
coverage annotations, not a product-completion percentage.

The product assessment remains unchanged: no Phase 0–8 exit or complete mapper
workflow is established by this CI run. Hardware, field, live-radio and later
product validation remain distinct from these foundation checks.

## Current execution checkpoint — 2026-09-13

Main is at `c4a1219`. The complete reviewed Lab MCP is integrated with exact
resources and ten allowlisted tools, immutable Git revisions, authenticated
hosts, signed requests/results, bounded execution and sanitized text artifacts.
Its fresh package-local bootstrap passes 32 Node tests with one explicit
Windows-only skip, three fixed-helper parser tests, package/SBOM checks and an
independent no-findings review. The reviewed Tauri/React desktop shell is also
integrated; deterministic cancellation handling, the exact Vitest 4.1.11 fix,
native Rust tests, Clippy, Playwright and a no-bundle release build all pass.

Hosted run `34772736694` at `a482ac6` passes the entire expanded suite on Ubuntu
and macOS, including Lab and desktop gates. Windows fails in the combined
bootstrap step before runtime tests execute. Independently reviewed diagnostics
now emit only one of five fixed bootstrap stage IDs and a bounded numeric exit,
while suppressing command text, paths, environment and child output. A new
hosted run must identify and correct that stage before the reviewed refusal
fixtures receive native Windows execution. Physical-radio, Kismet and
spectrum executions remain runtime gates rather than simulated evidence.

The regenerated candidate traceability matrix currently reports **66
VALIDATED, 88 IN_PROGRESS, and 3,182 NOT_STARTED leaf obligations**, with zero
BLOCKED_EXTERNAL or DEFERRED_BY_ADR. These are source-coverage records rather
than a product completion percentage.

## Windows active loopback correction — 2026-09-13

Hosted run `34770478017` at diagnostic integration `86fe0e2` passes Ubuntu and
macOS. Windows passes the success-only loopback path and fails the independently
named refusal-only and same-connector mixed success/refusal paths. This proves
the remaining defect is Windows refusal completion rather than successful
connect handling. The current implementation already uses writable-only Mio
interest and bounded transient peer rearming; the isolated correction is now
focused on the exact refusal completion semantics. Process timeout,
cancellation and descendant-drain tests pass on the hosted platforms in this
run. Local focused tests and a Windows-target Cargo check passed before this
native result; independent review and a successful hosted rerun remain required.

## Prior progress audit — 2026-09-12

The [progress review](docs/reviews/plan-progress-2026-09-12.md) inspects `fca5b3f`
and supersedes pending CI statements in the historical checkpoints below.
Hosted run `34350467190` at `966f92a` completed: Ubuntu and macOS passed;
Windows regression failed. Public annotations identify Sionna lifecycle,
active-process and SBOM portability tests; full failure causes remain to be
established. Fresh ledger, architecture and source inventory checks pass.
The newer hosted run `34726569253` at `fca5b3f` completed with Ubuntu and
macOS success and another Windows regression failure. Its public annotations
repeat the active-process and Sionna lifecycle failures; full causes remain
under investigation in the isolated Windows correction worktree.

The application worktree contains no implementation changes yet. Capture
session, acquisition spool and active-process test corrections remain isolated
and unfinished. The ledger reports 64 validated, 75 in-progress and 3,196
not-started leaf records; these counts are not a product completion percentage.

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
| Phase 8 plugin contract foundation | Candidate `feat/phase8-plugin-sdk-current`; `phase8-plugin-sdk` | Focused declaration/negotiation and bounded raw-manifest parsing tests pass, including raw-byte/depth preflight and fixed v1 canonical golden vector. Follow-up independent review, WIT compiler validation, third-party sample collector/metric/export, runtime invocation, resource enforcement, sandboxing, and project integration remain open; no Phase 8 exit or SEC-002 completion is claimed |
| Neutral planning interchange | Reviewed source `3604e40` | Original schema proof passes 13 focused tests and independent review; external planner bridge, maintainer RFC and runtime round trips remain open |
| Canonical scene renderer input | Root integration / reviewed source `125133e` | Independent canonical browser probes and 8-workload benchmark pass on a fresh server. Integration rebuild exposed absolute-path-dependent WASM bytes; a shared-workspace build now reproduces identical bytes at three checkout roots. Complete integrated regression and independent build-environment review pass. Final renderer/product gate remains open; [follow-up](docs/reviews/renderer-wasm-correction-followup.md) |
| Native capture session boundary | Integrated `1fb8d12` + `ecf9b59` | Reviewed shared normalization; timing corrections `73111b0` and `1711f13`; exact identity mapping retention sources `ff70362` + `4c47283` independently approved and integrated, including custom and empty-capture identity regressions |
| Durable capture session and unassociated acquisition spool | Integrated and independently approved at `cec2efd` | Bounded session closure, allocation preflight, SQLite storage/recovery and cancellation-safe publication are validated; product coordinator, streaming journal and UI wiring remain open |
| Project application boundary | Integrated and independently approved at `c61afc0` | Typed create/open/current-snapshot use cases are validated; operation-backed mutations and the reviewed desktop product path remain open |
| Barrier-aware measured interpolation | Integrated and independently approved at `425edfe` | Direct finite-segment path cost, stable weights, unknown support, budgets and cancellation are validated; polygon shortest paths, floors, calibrated uncertainty and publication remain open |
| Active TCP measurement foundation | Integrated through `e4eec6d`; reviewed Windows fixture correction `f019928` | Reviewed scheduling, attribution, budgets, cancellation and statistics are integrated. Native Windows hosted validation of refusal completion remains |
| Desktop instrument shell | Integrated and independently approved through `74d0f89` | Native lifecycle, opaque grants, project commands, command palette, responsive shell, deterministic cancellation proof, zero-finding npm audit and release build pass locally; hosted multi-OS and later Phase 1 workflow gates remain |
| Kyberia Lab MCP | Integrated through `964a5ae`; CPU-only Sionna correction at `84a5bcb`, independently reviewed follow-up `d0a88eb` | Runtime and docs/ledger reviews pass for the bounded correction. The change removes the accelerator suite/selector; pinned worker-0.2.0 execution and provisioned authenticated hosts plus physical Windows/Kismet/spectrum executions remain |
| Phase 5 spectrum evidence contract | Integrated on `main` at `9641431`; correction re-review `16a9111` | Signature rules v2 fail closed for dBm/Hz and require 80% per-bin coverage across event sweeps; 14 synthetic contract tests pass. Both prior MAJOR findings are resolved; one non-blocking v1 decoder-diagnostic MINOR is documented. No SoapySDR/vendor adapter, bandwidth normalization, hardware, remote-sensor or labeled-trace execution; Phase 5 remains open. See [validation](docs/validation/phase5-spectrum-contract.md) |
| Hosted validation diagnostics | Bootstrap diagnostic integrated through `c4a1219` | Ubuntu/macOS pass run `34772736694`; Windows stops during bootstrap. Five fixed, redacted stage IDs will identify the failing dependency step on the next run |
| Windows native request runtime | CI / integrated `2988bc6` | At `c02212d`, macOS and Ubuntu pass; Windows next identifies a Unix-biased missing-path test. Platform-absolute retained fixture correction passes focused local validation and independent review; native Windows confirmation remains open |
| WIFI-001 bounded Beacon/Probe IE parser | Integrated on `main` at `1d8e68f`; promotion validation recorded at `7d126cc` | Mainline focused tests, direct typed-field tcpdump comparison, deterministic mutation, formatting, strict Clippy, ledger, architecture and source-inventory checks pass. Independent correction re-review is PASS in `ffcaa8d` (review source commit `622452b`). Coverage-guided campaign is prior hash-matched evidence, not rerun here; other WIFI items, `INS-005`, Phase 2 exit, Kismet/physical capture and broader TST-002 gates remain open |


The publication review is approved for the bounded increment in
[its review packet](docs/reviews/materialized-publication-integration-review.md).
An active draft or passing author test is not integration approval.

## Current validation

At combined integration `c4a1219`, the locked Python environment passes **288 tests with 22 skips**;
the Lab MCP passes **32 Node tests with one explicit Windows skip** plus three
Windows helper parser tests; and the merged developer-command regressions pass
19/19. Ledger and architecture checks pass, and source inventory covers **522
locked Cargo packages** after desktop integration. Desktop typecheck, production
build, 15 Vitest tests, 10 Playwright workflows, 25 Rust tests, Clippy and the
no-bundle Tauri release build pass. The full serial Rust workspace and full
workspace Clippy pass. The desktop npm audit and Lab production pnpm audit both
report zero current vulnerabilities. The Windows target active-measurement
check passes; native hosted execution remains open.

At `b4c4c33`, `.tools/venv/bin/python tools/dev.py check` passes the complete
integrated check: Rust regression, formatting/lint/typecheck, 215 Python tests
run (19 skipped), source inventory, traceability and original fixture checks.
Log: `.tools/post-cli-platform-complete-check.log`. This does not close
pending hosted Windows or product/runtime gates.

At identity-retention integration `5066c6d`, the full workspace regression
passes **635 tests, zero failures, nine ignored**. Log:
`.tools/mapping-integrated-workspace.log`.

At `1711f13`, the complete Rust workspace regression passes **634 tests, zero
failures, nine ignored**. Log: `.tools/drain-integrated-workspace-loopback.log`.
The initial sandboxed run failed when local Kismet HTTP fixtures could not bind;
the authorized loopback rerun completed successfully. Hosted macOS and Linux
also passed at this source; Windows test-import correction `50f43e4` is in its
next hosted run, with results pending.

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

The renderer integration candidate also passes `.tools/venv/bin/python tools/dev.py check`: 635 Rust tests passed, 9 ignored; 215 Python tests ran with 19 skipped and no failures. Formatting, lint, typecheck, architecture, 241-package inventory, ledger and original fixture checks pass. Log: `.tools/post-renderer-workspace-check.log`. Separate renderer checks pass 31 Node tests, byte-identical WASM rebuilding and the main-tree Chromium matrix. These results do not close the final renderer decision or product UX gate.

## Blocked capabilities and technical debt

No entire capability is classified externally blocked. Requirement-specific
external evidence belongs in [BLOCKERS.md](docs/implementation/BLOCKERS.md).
Hardware, credentials and unavailable OS execution must be distinguished from
runnable implementation and fixture work.

Remaining work includes product capture/point-survey wiring and acquisition
spooling; final renderer choice and broader Tauri/WebView host/driver tests;
Lab host provisioning and physical integration gates; remaining Windows active
runtime validation; geometry imports/3D/CRS/CAD and broader offset numerical/platform validation;
complete metric families, regulatory/PHY/airtime/capacity semantics; live Kismet
and PCAPNG normalization parity; full Sionna runtime/calibration gates; storage
live-load/failure/fuzz/portability gates; authorization/coordinator/conflict UX;
and supply-chain coverage for every distributable environment. The legacy
project-receipt replay path still has quadratic growth. Resource byte counters
are deterministic proxies, not resident-memory guarantees. Source requirement
IDs must remain section-qualified.

## Next executable work

1. Push the reviewed Lab, desktop and Windows-fixture integration and repeat the
   three-host CI gate, using native Windows results to accept or reject the
   refusal-race correction.
2. Provision authenticated Lab runner configuration on available Windows,
   Kismet and spectrum hosts and execute the signed runtime gates.
3. Execute the pinned CPU/LLVM worker-0.2.0 and close its numerical/runtime
   evidence before any Sionna product-quality claim; keep authenticated-host,
   Windows, Kismet and spectrum gates explicit.
4. Implement defensive map-asset admission and operation-backed floorplan
   calibration through application and desktop boundaries, with parser/security
   review and real UI evidence.
5. Connect reviewed native capture, point-survey, active diagnostics, measured
   interpolation, and project persistence into the usable Phase 1 workflow.
6. Audit Phase 0/1 acceptance against the complete ledger, then continue the
   remaining roadmap phases in dependency order.

Earlier detailed checkpoints and validation histories are retained in
[the historical snapshot](docs/implementation/history/status-4a55d35.md) and Git.
Tests and trash artifacts remain retained for manual cleanup. Agents use isolated
worktrees, and no author is the sole reviewer of a meaningful feature.
