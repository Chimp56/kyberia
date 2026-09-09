# Shared materialization budget review in progress

Status: REQUEST_CHANGES; no integration approval. Candidate is the actively
edited `.worktrees/shared-materialization-budget` based on `4adb446`.
The observations below describe inspected snapshots, not an immutable final diff.
Implementer: Laplace. Independent reviewer: root.

## Required review evidence

1. Charge identity serialization before retaining its bytes. The new baseline
   budgeted writer addresses the previous charge-after-allocation path. Verify
   operation identity admission, temporary payload copies and an already exhausted
   budget; returning an error after allocating is insufficient.
2. Charge every yielded operation during set admission, including exact
   duplicates. Unique-map length cannot bound repeated-input work. Cancellation
   must be observed before the duplicate early-continue path. Independent
   duplicate-input probing is assigned to Russell.
3. Charge replay payload clones as well as structural collection entries. A
   fixed 128-byte entry estimate does not account for arbitrarily larger bounded
   mutation payloads. Independent Apply-only replay must poll cancellation too.
4. Preserve cancellation through direct and nested identity/operation/merge
   errors. Test cancellation during graph replay; the existing small poll count
   now stops in baseline serialization and cannot prove graph propagation.
5. Preallocation tests must fail for charge-after-allocation implementations.
   Error category and unchanged usage alone do not establish allocation order.
6. Verify shared-limit failure leaves both local and shared counters unchanged;
   local-limit failure coverage alone does not prove both failure paths.
7. Keep wire-size counting equivalent to actual encoding across versions,
   operation variants, escaped strings and nonempty parent sets.

## Executed checks

Root independently ran locked/offline tests on the evolving candidate:
resource-budget: 5 passed; materialization-identity: 17 passed;
causal-materializer: 28 passed. These are scoped regression evidence, not approval
of the missing accounting or cancellation properties. Re-run affected suites on
an immutable reviewed candidate before integration.

The storage publication adapter must subsequently share one budget across its
entire verification transaction and preserve resource/cancellation outcomes.
No product capability or external runtime gate is completed by these checks.

## Duplicate-admission follow-up

Russell independently approved the corrected raw-input admission loop. The
retained probe `/private/tmp/kyberia-duplicate-budget-probe-20260909` now rejects
the 100,001st raw input with the count limit and admits 100,000 unique operations.
Every input, including duplicates, is charged and cancellation-checked before
deduplication. The reviewer ran 38 operation integration tests successfully, plus
Clippy, formatting and whitespace checks. This closes item 2 for the inspected
consumer candidate; other findings and complete consumer integration remain open.

## Replay follow-up

Independent review approved direct replay accounting at `bd2ba0e` plus
`941440b`. The strengthened regression compares short and large forward and
inverse payloads and expects three times the canonical-size delta for Apply,
Undo and Redo. The retained independent probe at
`/private/tmp/kyberia-replay-target-budget-probe-20260909` also rejects large
Undo and Redo payloads at independently calculated pre-copy quotas and verifies
the Resolve payload delta (1,019 bytes). The reviewer ran 39 operation tests,
Clippy and formatting successfully. Root's immutable combined consumer run
passed 92 tests with no failures or ignored tests.

Whole-consumer approval remains withheld: conflict inspection calls
`effect_event` and creates additional effect/frontier copies without equivalent
payload accounting. A separate correction must account for those copies before
allocation and receive independent review. Direct replay approval does not
establish bounded conflict inspection.

Root separately verified cancellation after replay has started: the regression
asserts nonzero witness and project-copy usage before the cancelled result.
Current direct/nested cancellation mappings preserve the cancellation category.
Architecture and external source checks pass (241 packages). These scoped
checks do not approve the complete consumer candidate.
