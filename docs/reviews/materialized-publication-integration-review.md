# Materialized publication integration review

Status: pending independent full storage review. Candidate storage commit:
`3ac3e10`; consumer corrections and writer tests included through `b825eb6` in
`.worktrees/materialized-project-publication`. Original publication author:
root; cumulative-budget implementer: Laplace. Root has reviewed the budget
changes, but is not the sole reviewer of the full publication feature.

## Executed candidate validation

Root ran `cargo test -p kyberia-project-store --locked --offline` at `b825eb6`:
121 passed, zero failed, one ignored throughput benchmark. Log retained at
`.worktrees/materialized-project-publication/.trash/root-storage-b825eb6-regression.log`.
All-target project-store Clippy with `-D warnings` also passes.

Coverage includes immutable baseline registration, exact historical retry,
current-pointer rollback detection, replay/result equality despite consistent
forged checksums, missing manifest inventory, corruption, legacy unknown state,
schema validation, cumulative history exhaustion, cancellation after replay
copy work, empty-budget rejection and decoder-stage quota admission.

## Independent review acceptance

Review the full storage diff, including optional SQLite schema groups,
file-before-transaction publication and projection recovery. Verify one budget
survives the entire history transaction; baseline and operation inventory reuse
must not skip historical validation. Check charges before metadata, BLOB,
decoder and prefix copies; preserve cancellation and resource errors through
all adapters. Confirm rejected publication or verification cannot publish a
partial project or advance the current pointer. Check default quotas preserve
their intended 64-bit values and are representable on smaller targets.

Earlier scoped approvals for current-pointer rollback and identity error
classification remain evidence for those corrections only. No project feature,
FND-011 completion or storage runtime gate is promoted by this pending review.
