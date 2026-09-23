# Phase 4 bounded planner-evaluator validation

Integrated code baseline: `3370d17`; independent review report: `015b86f`.
Boundary-test follow-up base: `bc51e80b14e30f927628f4ba9f2e92a4773423fe` on
`feat/phase4-evaluator-boundaries-current` (separate, not integrated).

## Scope

This is a deterministic verifier for small caller-proposed AP plans. It checks
only represented constraints from caller-supplied coefficients, assignments,
capacity and budget inputs. One whole-cell assignment per demand and one radio
configuration per candidate are explicit contract limits. This crate does not
generate candidates, optimize, model airtime/interference, repair proposals, or
prove optimality.

## Candidate checks

- `cargo test -p kyberia-planner-evaluator --locked --offline` — PASS, 7
  baseline tests before the follow-up. The follow-up suite now has 11
  integration tests; no unit or doctests.
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
production evaluator change was needed.

The full workspace suite, physical-site validation and real-RF evaluation were
not run. Physical-site/mounting identity, aggregate-capacity limitations,
solver/repair behavior and broader Phase 4 acceptance remain open. The follow-up
is not integrated until separate independent review and promotion.
