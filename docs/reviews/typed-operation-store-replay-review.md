# Typed operation-store replay review

Reviewer: `/root`, independent of the implementation author.
Disposition: pending frozen candidate and runtime verification.

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
