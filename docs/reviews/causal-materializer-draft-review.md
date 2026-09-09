# Causal materializer early review

Reviewer: `/root/operation_log_review_luna`, independent of author.
Scope: evolving `feat/causal-materializer` draft based on `5eefe02`.
Disposition: REQUEST_CHANGES. Source review only; no runtime approval.

## MAJOR findings

1. `CausalReplay::apply` builds redo effects using the target Apply operation
   ID. Reapplying that ID to the causal project can cause `DuplicateOperation`.
   Use the redo ID and verify Apply/Undo/Redo followed by a descendant and by
   resolution.
2. Raw-payload causal replay duplicates the operation-log effect state machine.
   The redo mismatch demonstrates semantic drift. Establish a shared canonical
   replay boundary or equivalent reviewed contract; test nested resolutions,
   criss-cross histories, permutations and typed calibration restoration.
3. `before_projects` retains cloned full projects but the inspected consumer
   only checks key membership. Replace this with compact validated references.
   Any required full-state retention needs a byte-aware admission bound.
4. The invocation-wide visit accumulator fixes the earlier reset issue, but
   does not charge full candidate scans, pair comparisons, project cloning,
   event growth or outer aggregate-conflict loops. Charge dominant work before
   performing it and verify deterministic boundary rejection.

## Corrections already observed and review limits

Project versioning is now scoped to `ProjectSchemaVersion`, leaving unrelated
capture/evidence V1 contracts unchanged. Exact legacy serialization tests remain
required. The review's missing-document observation was overtaken by the author
adding draft ADR-0022 and materializer documentation; those documents still need
review against final code and executed tests.

Root separately identified historical conflict checks that can reject resolved
descendants, and the need to define common-causal-subgraph resolution priors
without assuming a unique maximal common ancestor. These remain acceptance
cases in the implementation packet. No materializer code has been integrated.

## Frozen candidate verification

Candidate `5b56a9e` is now frozen. Root independently ran
`cargo test -p kyberia-causal-materializer --locked --offline`: 17 passed,
zero failed. The source inventory also passes with the existing root virtual
environment: `/Users/vincent/code/kyberia/.tools/venv/bin/python
tools/source_inventory.py check` reports 241 locked external packages. The
author's Python environment issue is therefore not an external blocker.

Final independent review remains pending. In particular, the frozen clone
estimate charges `operation_count + 1` copies while reconstructing each causal
ancestor closure can invoke many more domain clones. A linear chain of `n`
operations replays `n(n-1)/2` ancestral effects, in addition to baseline and
final-application copies. Distinguish a peak-memory bound from cumulative copy
work; the current numeric estimate alone does not prove both. Existing passing
tests do not discharge this resource-model concern.
