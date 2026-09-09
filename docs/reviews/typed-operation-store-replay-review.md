# Typed operation-store replay review

Reviewer: `/root`, independent of the implementation author.
Disposition: APPROVED for the bounded storage adapter at `3ebd11dd89096f65ece8030d4cf6823964bc7136`.

The independent reviewer inspected the frozen source, tests and documentation
and ran `cargo test -p kyberia-project-store --test operation_store --locked
--offline` in the candidate worktree: 19 passed, zero failed. Both requested
corrections below are present: new fixtures use retained `.trash/test-runs`
directories and unresolved typed replay fails before resolution is appended.
No unresolved BLOCKER or MAJOR findings remain for this adapter.

This approval does not establish causal prior correctness. The conflict fixture
records an unknown prior on a branch whose parent activates a calibration;
that is admissible to this structural storage layer but must be rejected by
the future causal aggregate validator. It is not a valid aggregate replay
fixture. Aggregate materialization and causal validation remain open.

## Review history

The draft adds a narrow `Bundle::replay_operation_effects` boundary delegating
to the existing persisted operation-set validation and typed replay. This keeps
unknown calibration restoration explicit without treating replay effects as a
canonical materialized project. Legacy mutation-only replay remains available.

Before freeze, the new tests must retain fixtures under the assigned worktree's
`.trash/test-runs`, as required by AGENTS.md. Their initial use of system
`tempdir().keep()` avoids deletion but does not satisfy the required fixture
location. The author has been asked to correct only the new fixtures.

The resolved-conflict storage test must also assert that typed replay rejects
the unresolved conflict before appending its resolution. Reopen-and-compare
coverage then establishes persistence of the explicit unknown selected state.
Final acceptance requires the real operation-store suite and independent review
of the frozen delta. Causal prior validation and aggregate materialization remain
open downstream requirements.
