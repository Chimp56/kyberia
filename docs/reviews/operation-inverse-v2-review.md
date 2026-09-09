# Operation inverse V2 review

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
