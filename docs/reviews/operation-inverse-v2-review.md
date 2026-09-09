# Operation inverse V2 review

## Corrected scoped disposition

APPROVED by `/root` for the versioned inverse, typed replay/conflict and typed
resolution increment through `1530866d61346eeb7b4c231d12d99667b7d21b43`, including
`4beaa74` and `2f00aa9`. The reviewer did not author this code. Historical
REQUEST_CHANGES findings below are retained and corrected by the series.

Root independently passed 30 operation tests and one architecture test on the
frozen correction. Typed unknown resolution is now represented by `ResolveV2`
and exercised through actual typed replay. The original intent-conflict probe,
retained against the correction at
`.trash/review-probes/v2-frontier-corrected-227tjsly`, reports one conflict both
before and after adding the equal-value branch for IDs 11 and 13. No implicit
resolution occurs in those reproduced cases. Static review confirms distinct
toggle entries survive equal ordinary value branches. V1 operation golden
bytes remain covered by the passing compatibility test.

This is not approval of full FND-011 completion. Baseline binding, causal prior
validation, aggregate materialization, typed storage replay, and product conflict
UX remain open. Mutation-only replay explicitly cannot represent unknown
restoration. Resolution operations remain non-toggle targets; undo behavior for
resolution must be addressed in the complete product workflow.

Candidate: `4beaa746c791f324ec22defb558d0ffc0a820052`.
Reviewer: `/root` (not the author). Disposition: REQUEST_CHANGES.

The candidate preserves V1 encoding and introduces typed calibration priors,
explicit irreversible floor binding, and typed replay effects. These are useful
contract changes, but they do not yet provide complete V2 merge behavior.

## MAJOR: unknown-state conflicts cannot be inspected or resolved

`OperationSet::conflicts` converts both frontier effects through
`EffectValue::as_mutation` when constructing a conflict. An undo restoring
`Unknown(NotMeasured)` cannot make that conversion and returns
`TypedPriorRequired`. `replay_effects` calls this conflict path too, so its typed
boundary does not solve concurrent unknown-state edits. Resolution validation
also retains the mutation-only `effective_mutation` path.

Provide a typed conflict and resolution path that preserves explicit unknowns.
Acceptance must exercise concurrent unknown restoration and known activation,
inspect the conflicting states, and resolve them deterministically. Keep V1
bytes stable. A documented limitation alone does not close this requirement.

## Equal-effect representation audit

Known calibration activation is represented as `EffectValue::Mutation`, while
a V2 inverse restoring the same known calibration is represented as
`EffectValue::Calibration`. Derived equality distinguishes those variants.
Review semantic frontier equality and add a regression demonstrating that
equivalent known values do not manufacture an edit conflict merely because
their internal representations differ. Preserve intentional toggle-conflict
semantics where operation intent actually differs.

The author is preparing an isolated correction. Causal prior validation,
baseline-bound project materialization and storage replay integration remain
separate open work under FND-011. No full undo/merge completion is claimed.

## Follow-up candidate `2f00aa9`

Root independently reran the focused suite: 29 operation tests and one
architecture test passed. The correction carries typed conflict arms and removes
the mutation conversion from conflict inspection. It also normalizes known
calibration representations. These checks do not yet justify integration.

MAJOR: `try_resolve_v2` still accepts `Mutation`, and `OperationPayload::Resolve`
still carries a mutation-only selected value. An unknown/known calibration
conflict can therefore be inspected, but the resolver cannot choose its unknown
arm. The new test chooses a known calibration. Add a typed selected resolution
value with explicit unknown restoration and tests choosing either arm; preserve
the legacy V1 operation bytes. This is actionable implementation work.

The frontier identity comparison also needs an adversarial review: two toggles
with distinct targets compare unequal, yet each can compare equal to a plain
value with the same effect. That relation is nontransitive. Establish whether
representative collapse can hide an intent conflict when a third equal-value
branch is present. Test operation-ID and arrival-order permutations; retain
intent semantics independently from any safe value deduplication. This is a
review concern pending a concrete behavioral reproduction, not a claimed
reproduced failure.

The candidate remains frozen for independent review; the author is preparing
the typed-resolution follow-up in a separate worktree.

## Reproduced intent-conflict disappearance

Root's standalone probe against frozen `2f00aa9` constructs two equal-valued
apply roots, each activating calibration Y with prior X. Two concurrent undo
operations target the separate roots; both list the two roots as parents.
Their merged set reports one conflict. Adding a concurrent ordinary activation
of X with the same parents makes the conflict count zero, without any explicit
resolution operation. This reproduced with the added operation ID both between
and after the undo IDs. The test does not establish an arrival-order defect;
it establishes that an equal-value branch can erase a distinct-toggle intent
conflict.

Retained harness: `.trash/review-probes/v2-frontier-amgtu0nf`.
Command: `cargo run --offline --quiet`. Output for each tested ID is
`without value: 1`, then `conflicts 0` after adding the value branch.
Treat this as MAJOR until the implementation establishes consistent value and
intent semantics and tests that ordinary concurrent edits cannot implicitly
resolve conflicts that require explicit resolution.
