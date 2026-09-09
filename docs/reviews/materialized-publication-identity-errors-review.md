# Publication identity-error classification review

Status: APPROVED for the scoped correction only. Candidate is the isolated
`.worktrees/materialized-project-publication` draft based on `7427cb1`.
Author: root. Independent reviewer: Luna agent Russell.

## Correctness

Direct identity resource errors and nested operation resource errors retain
`PublicationError::ResourceLimit`. Other caller-input identity failures remain
`Identity`; persisted baseline and operation identity validation reports `Corrupt`.
The initial review found two persisted operation call sites using the input
mapper. Both were corrected and independently inspected on re-review.

Tests cover a declared oversized baseline payload through the actual identity
decoder, malformed baseline input, direct and nested resource-error categories,
and noncanonical input versus persisted-validation classification.

## Independent validation

- Project-store library tests: 16 passed.
- Materialized publication integration tests: 11 passed.
- Clippy with warnings denied: passed.
- Formatting and whitespace checks: passed.

No unresolved findings in this scope. The full publication draft is not approved:
shared cumulative budgets, preallocation accounting and remaining transaction
integration review still apply. Future cancellation variants introduced by the
budget prerequisite must preserve cancellation rather than map it to corruption.
