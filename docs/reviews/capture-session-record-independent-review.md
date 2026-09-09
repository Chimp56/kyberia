# Capture-session record independent review

Candidate: `a12ae63c2c57b6437b5b6019e7149fd44799e146` in the isolated capture-session-record worktree. Reviewer: Russell, independent of author Beauvoir. Disposition: REQUEST_CHANGES; no integration approval.

## Findings

- MAJOR: canonical record validation omits envelope clock epoch, source sensor/adapter metadata, observation radio/BSS/grouping evidence, and exact raw artifact reference closure. An adversarial probe accepted contradictory mapping evidence.
- MAJOR: the constructor accepts `Ok`, `partial=true`, and exit code zero, contrary to terminal-state semantics.
- MAJOR: generic `Bundle::verify()` does not inspect the capture-session inventory. A probe replaced session canonical bytes with `{}`; verification returned no failures although direct reading rejected the row.
- MINOR: stored revision 99 is accepted although the V1 projection marker is fixed at 1.
- NIT: add post-commit cancellation/retry recovery coverage and explicit revision/cache semantics.

Retained probes: `.worktrees/ci-rust-diagnostics/.trash/review-session-draft/probe` and `.trash/review-session-draft/verify-probe`. Probe output included `ACCEPTED_CONTRADICTORY_MAPPING`, `OK_WITH_PARTIAL_TRUE=true`, and `FAILURES=[]` respectively. These are local review support artifacts, not production fixtures or proof of acceptance.

## Validation and next action

Reviewer reports domain capture-session tests 8 passed, domain library 10 passed, project-store capture-session tests 8 passed, store library 32 passed, schema guards 14 passed; affected all-target Clippy, formatting, architecture, source inventory (241 packages), and whitespace checks passed. These green tests did not detect the reproduced defects. The author has been assigned corrections and adversarial regressions; re-review is required before integration.

## Frozen correction re-review

Russell independently reviewed `4de24dcc8554007f9f0e433113b7bb94c7d9c8ec`.
The mapping closure, session inventory verification, fixed revision marker,
post-commit cancellation recovery and idempotency corrections pass their scoped
regressions. Reviewer results: domain 11, store 34, schema guards 14 and native
composition 7 tests passed; affected Clippy, formatting, architecture and source
inventory checks passed. Integration remains **REQUEST_CHANGES**.

- MAJOR: the correction requires `partial == (terminal == Partial)`, contradicting
  the existing native protocol invariant
  `partial == (terminal != Ok && observation_count > 0)`. A valid error terminal
  with retained observations was rejected as `Inconsistent("capture session
  partial terminal flag")`. Preserve admitted error, permission, timeout and
  cancellation evidence rather than changing its meaning to fit persistence.
- MINOR: reading the SQLite canonical BLOB materializes it before the record's
  1 MiB limit is checked; the broader SQLite limit still bounds admission.
- NIT: align documented test counts and explain that inventory verification
  stops at the first invalid row.

Root review of follow-up `59ca394dd3891c541464e651d63d01ab38c472d7` also
requests changes. Narrowing the Rust adapter's completion contract leaves it
inconsistent with the Python producer contract and rejects previously admitted
evidence. The composition size check also clones all mapping strings, then
clones the mapping again, before checking the canonical byte limit. Add an
early checked borrowed-input size/count preflight, retaining the final exact
serialized-size check for escaping and encoding overhead. Neither a passing
constructor test nor the canonical byte limit proves bounded preflight allocation.

The author is correcting both issues. A fresh frozen-source independent review
is required before integration; no later candidate is approved by this packet.
