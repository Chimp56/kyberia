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
