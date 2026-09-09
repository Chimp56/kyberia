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
