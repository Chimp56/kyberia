# Phase 4 bounded planner-evaluator validation

Integrated code baseline: `3370d17`; independent review report: `015b86f`.
The test-only boundary follow-up is integrated at `141fc0e` from base
`bc51e80b14e30f927628f4ba9f2e92a4773423fe`; independent review is recorded in
[`phase4-evaluator-boundaries-follow-up-review-current.md`](../reviews/phase4-evaluator-boundaries-follow-up-review-current.md)
and the wording re-review in
[`phase4-evaluator-boundaries-f1-rereview-20260923.md`](../reviews/phase4-evaluator-boundaries-f1-rereview-20260923.md).

## Scope

This is a deterministic verifier for small caller-proposed AP plans. It checks
only represented constraints from caller-supplied coefficients, assignments,
capacity and budget inputs. One whole-cell assignment per demand and one radio
configuration per candidate are explicit contract limits. This crate does not
generate candidates, optimize, model airtime/interference, repair proposals, or
prove optimality.

## Candidate checks

- `cargo test -p kyberia-planner-evaluator --locked --offline` — PASS, 11
  integration tests (7 existing + 4 added); 0 unit tests and 0 doctests.
- `cargo clippy -p kyberia-planner-evaluator --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt -p kyberia-planner-evaluator -- --check` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 locked external packages.
- `python3 tools/ledger.py check` — PASS after regeneration, 5,396 source
  blocks, 438 explicit ID occurrences, and 447 headings.
- `python3 -m unittest discover -s tests -p 'test_ledger.py'` — PASS, 31 tests.
- `git diff --check` — PASS.

The follow-up adds exact and one-tenth-dBm adjacent threshold cases for both
downlink and uplink; just-under/equal/just-over cases for AP count, installation
budget, aggregate radio capacity, radio client count, and eligible-AP coverage;
a missing selected-candidate coefficient alongside enough known eligible APs;
and admitted-at-ceiling / ceiling-plus-one resource tests for candidates,
selected-candidate entries, demands, assignments, and link coefficients. These
are verifier contract tests only, not optimizer or product acceptance. No
production evaluator change was needed at that test-only checkpoint.

## OPTB-007 caller-supplied scenario evaluation candidate

This implementation was integrated on `main` at `873d202` from reviewed
candidate `3b25bed2e6ac08623b72cd3cb0091d785ccdd946`, based on
`a8cf7dccbe1a1c40d6adc51868e90dfe2cdcd452` and the unchanged plan digest above.
Independent review approved the stated contract with no findings; see
[`phase4-scenario-evaluation-current-review.md`](../reviews/phase4-scenario-evaluation-current-review.md).
The implementation adds `evaluate_scenarios`, which evaluates a
non-empty set of uniquely identified complete `PlannerProblem` + `ProposedPlan`
inputs using the existing single-plan verifier. A caller expresses an AP/radio
failure by supplying its altered problem and plan, including any reassignment;
coefficient uncertainty is expressed by supplying perturbed link estimates.
The evaluator does not generate outage combinations, infer uncertainty,
reassign, optimize, or repair.

Results are sorted by `ScenarioId`, so input permutation cannot change the
returned value. Robustness policy is an exact positive rational no greater
than one:
the number of individually feasible cases must meet that fraction of all
supplied cases. The minimum required case count is computed by integer ceiling
division, and decisions use integer cross-multiplication only. A case that is
infeasible or unknown is not counted as feasible; its full existing findings
remain in the corresponding result.

Admission is fail-closed before any plan evaluation: at most 64 cases; each
case retains the evaluator's caps of 128 candidates, 4,096 demands, 131,072
link coefficients, 128 selected-candidate entries, and 4,096 assignments; and
the aggregate work estimate is capped at 1,057,024 units. Per case, units are
`demands × candidates + demands + candidates + link coefficients + selected
candidates + assignments`. Empty input, duplicate IDs, invalid fractions,
per-case limit errors, aggregate exhaustion, or a case evaluator error return
only an error, never partial scenario results. This deterministic accounting
is an admission bound, not a wall-clock guarantee.

The added integration tests cover an explicitly supplied candidate outage with
caller-supplied reassignment, a failure exposing a violated coverage
constraint, perturbed coefficients at/above/below a threshold, exact rational
rounding, permutation invariance, invalid/duplicate/empty input, the 64-case
boundary and plus-one, aggregate-work exact-ceiling and plus-one, and a later
case error after an earlier valid case. These tests prove only this bounded
scenario-evaluation contract. Full planner generation, N-1 case generation,
uncertainty modeling, airtime/interference, capacity optimization, repair,
Pareto selection, and all Phase 4 exit criteria remain open.

### Candidate validation

- `cargo test -p kyberia-planner-evaluator --locked --offline` — PASS, 19
  integration tests (11 existing plus 8 scenario tests); 0 unit tests and 0
  doctests.
- `cargo clippy -p kyberia-planner-evaluator --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt -p kyberia-planner-evaluator -- --check` — PASS.
- `python3 tools/architecture.py check` — PASS.
- `python3 tools/source_inventory.py check` — PASS.
- `python3 tools/ledger.py generate` and `python3 tools/ledger.py check` — PASS
  after recording this candidate's source hashes and validation evidence.
- `git diff --check` — PASS.

The full workspace suite, physical-site validation and real-RF evaluation were
not run. Physical-site/mounting identity, aggregate-capacity limitations,
solver/repair behavior and broader Phase 4 acceptance remain open. The integrated
follow-up is test evidence only; it does not add production evaluator behavior.
