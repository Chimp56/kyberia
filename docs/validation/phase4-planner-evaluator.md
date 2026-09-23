# Phase 4 bounded planner-evaluator validation

Implementation candidate: `d9444c94b32d77b7988a259a6c5d15b4f06245a5`.
Integrated code: `3370d17`; independent review report: `015b86f`.

## Scope

This is a deterministic verifier for small caller-proposed AP plans. It checks
only represented constraints from caller-supplied coefficients, assignments,
capacity and budget inputs. One whole-cell assignment per demand and one radio
configuration per candidate are explicit contract limits. This crate does not
generate candidates, optimize, model airtime/interference, repair proposals, or
prove optimality.

## Mainline checks

- `cargo test -p kyberia-planner-evaluator --locked --offline` — PASS, 7
  integration tests; no unit or doctests.
- `cargo clippy -p kyberia-planner-evaluator --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt -p kyberia-planner-evaluator -- --check` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 locked external packages.
- `python3 tools/ledger.py check` — PASS, 5,396 source blocks, 438 explicit ID
  occurrences, and 447 headings.
- `git diff --check` — PASS.

The full workspace suite, physical-site validation and real-RF evaluation were
not run. The independent reviewer found no blocker or major correctness issue;
recommended test follow-ups are exact RSSI-threshold and resource-limit
boundaries and a case where missing selected-candidate coefficients cannot
change an already-satisfied coverage result. Those are recorded as residual
test coverage opportunities, not product acceptance. Physical-site/mounting
identity, aggregate-capacity limitations, solver/repair behavior and broader
Phase 4 acceptance remain open.
