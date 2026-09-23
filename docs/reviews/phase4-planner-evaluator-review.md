# Independent Phase 4 planner-evaluator review

## 1. Objective and scope

Independently reviewed the isolated Phase 4 candidate as a bounded, hardware-independent plan-evaluation contract. Review was limited to candidate changes against the specified base, with particular attention to plan §§9.3, 9.4, 9.12, 16.11, 18.8 and `OPTB-005`/`OPTB-006`. No author or integration worktree was edited.

## 2. Requirements and records

The full authoritative `plan.md` and applicable `AGENTS.md` were read; `plan.md` is unchanged from the base (SHA-256 `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`). The review covers hard-constraint evidence, objective components, infeasibility explanations, deterministic validation, and optimizer backlog records. The candidate ties the incremental contract to the relevant source occurrences and backlog entries in `TRACEABILITY.md`, `ledger.json`, and `execution-dag.json`.

## 3. Base and head

- Base: `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`
- Candidate head: `d9444c94b32d77b7988a259a6c5d15b4f06245a5`
- Merge base verified equal to the requested base.

## 4. Reviewed paths

- `crates/planner-evaluator/Cargo.toml`
- `crates/planner-evaluator/src/lib.rs`
- `crates/planner-evaluator/tests/evaluator.rs`
- `STATUS.md`
- `docs/implementation/TRACEABILITY.md`
- `docs/implementation/execution-dag.json`
- `docs/implementation/ledger.json`
- `docs/licenses/cargo-sources.json`
- `tools/architecture.json`
- `Cargo.lock`

## 5. Invariants checked

- Public measurements use explicit integer units: tenths of dBm, caller-defined common area weights, decimal kbps, and integer cents. Threshold checks use integer comparisons, with both downlink and uplink required.
- Input order does not affect evaluation: demands, candidates, selected IDs, coefficients, assignments, evidence lists, and findings are normalized or emitted through ordered maps/sets and sorted traversal.
- Each demand has at most one whole-cell assignment; each candidate record models one radio configuration. These simplifications are explicit in source comments and the API shape; splitting a demand or modeling multiple radios/configurations at one physical candidate is outside this contract.
- Missing selected-candidate coefficients are `Unknown`; they fail closed when the unknown value could affect required coverage, and a missing coefficient on the assigned selected radio also makes assignment status `Unknown`.
- Coverage evidence distinguishes eligible selected/unselected candidates, unknown selected candidates, and known below-threshold selected links, including direction-specific deficits. Feasibility is false for any `Violated` or `Unknown` finding.
- Candidate/demand/link/assignment counts are bounded; aggregate additions use checked arithmetic. Per-radio load and client totals are checked independently against capacity/client ceilings.
- The evaluator reports only represented constraints and component values. It does not solve, rank, optimize, model airtime/interference, produce repairs, or prove optimality.

## 6. Checks run

All commands were run from the isolated review worktree; no broad workspace build was run.

- `cargo fmt -p kyberia-planner-evaluator -- --check` — passed.
- `cargo test -p kyberia-planner-evaluator --locked --offline` — passed, 7 integration tests; no unit/doc tests.
- `cargo clippy -p kyberia-planner-evaluator --all-targets --locked --offline -- -D warnings` — passed.
- `python3 tools/ledger.py check` — passed: 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `python3 tools/architecture.py check` — passed.
- `python3 tools/source_inventory.py check` — passed: 522 locked external packages.
- `git diff --check BASE...HEAD` — passed.
- Parsed the modified JSON documents; verified the execution DAG has 60 unique nodes and no missing dependency IDs. Its ordering is graph-edge based, not a strict topological list; the candidate node correctly depends on `contracts`.
- Verified `docs/licenses/cargo-sources.json` `cargo_lock_sha256` equals the actual `Cargo.lock` SHA-256 (`b1718752125bc59f05810f46215ca4a86151bb38cd830bd50de911da30f7e27c`). The new crate adds no external dependency.
- Source SHA-256 values match the ledger entries: implementation `5f06e0779073b445f07aaf0f3129e1d4163c756e83d1d20894e2284218092c57`; tests `dfbc1fc91c1c96ef44d930b079210987109ec252d9e950c710725464f829ea7e`.

## 7. Traceability and status

The candidate marks relevant plan occurrences and `OPTB-005`/`OPTB-006` as `IN_PROGRESS`, not complete. Ledger records identify the implementation/tests and leave validation/review arrays empty. `STATUS.md` says the candidate is isolated and pending review, that Phase 4 remains open, and explicitly disclaims solver, airtime/interference, repair-loop, and optimality claims. The feature status is consistent with the observed implementation; it does not present these checks as full optimizer acceptance.

## 8. Report commit and cleanliness

This report is the only file added by the review. It is committed separately in the isolated review worktree; the candidate source and author/integration trees remain unchanged. Review-worktree Git status is clean after commit.

## 9. Findings and residual risks

No blocker or major correctness finding in the reviewed bounded contract.

Residual scope limits are intentionally significant: no physical candidate-site/mounting identity or geometry is modeled, so distinct IDs cannot be checked for co-location or mounting/exclusion conflicts; the one-radio-per-candidate input convention must be honored by callers. Capacity is a caller-supplied aggregate kbps ceiling, not airtime, association, wired-uplink, or concurrency/scenario validation. There is no solver, repair generation, or full nonlinear rescore, so this is not the plan's complete evaluator/repair loop.

Test coverage is appropriately small for the current contract but could be strengthened with exact RSSI-threshold and resource-limit boundary cases, plus a case where a missing selected-candidate coefficient is provably non-decisive because known eligible candidates already satisfy required coverage. The current implementation handles those branches consistently by inspection; they are not independently regression-tested here.

## 10. Blockers and limitations

No execution blocker. Codebase-memory graph MCP tools were unavailable in this session; review used exact-source/Git inspection and makes no graph-coverage claim. No full-workspace build, real RF input validation, optimizer comparison, physical-site test, or platform/hardware gate was attempted, since those are outside this bounded review scope.
