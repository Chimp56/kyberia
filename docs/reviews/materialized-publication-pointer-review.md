# Materialized publication current-pointer review

Status: scoped correction approved; complete publication draft remains unapproved.
Candidate: `.worktrees/materialized-project-publication`, uncommitted draft based
on `7427cb1`. Reviewer: independent Luna agent Russell; implementer: root.

## Finding and correction

A current pointer could be rewritten to a valid older publication after a newer
operation revision had been published. Full history verification reported no
failures. The regression failed before the correction. Verification now checks,
after validating every retained publication and the current state binding, that
no retained publication has a higher operation revision than the current pointer.
This detects inconsistency within retained history; it does not authenticate a
bundle or detect rollback of the entire bundle.

## Independent disposition

APPROVE for this correction, with no BLOCKER or MAJOR findings. This is not
approval of the complete publication implementation. Aggregate verification
resource accounting remains a separate MAJOR prerequisite.

The reviewer ran all eleven publication tests successfully, formatting and
whitespace checks. A MINOR requested the exact rollback diagnostic assertion;
root changed the test accordingly and reran it successfully. A fixture-location
NIT was resolved by retaining this test's files under the worktree's
`.trash/test-runs`. An unrelated redundant closure initially failed Clippy; root
corrected it and Clippy with warnings denied subsequently passed.

## Remaining review scope

The subsequent baseline resource-error mapping and shared-budget integration
require review. The full storage suite passed 117 tests with one benchmark
ignored after the pointer guard and before the additional error-mapping test.
No product capability, phase, or integration approval follows from this scope.
