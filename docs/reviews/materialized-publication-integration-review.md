# Materialized publication integration review

Status: **APPROVED for the bounded increment** through integration candidate
`7be5e9a`. Original publication author: root; cumulative-budget implementer:
Laplace; full independent reviewer and preflight-correction author: Russell.
Root independently reviewed Russell's correction, and Russell independently
approved root's subsequent read-admission tests. See the
[correction review](materialized-publication-correction-review.md).

The earlier REQUEST_CHANGES findings and candidate results below are retained
as history. Both MAJOR findings are corrected; the generic bundle-wide artifact
budget limitation remains scoped debt. Product workflows and full Windows
runtime validation remain open.

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

## Independent findings

- **MAJOR STORE-BUDGET-001:** Operation, publication, baseline and current-state
  row decoding can materialize attacker-controlled text before strict text-size
  preflight. Fixed per-row charges do not bound strings in an externally modified
  SQLite database whose CHECK constraints were bypassed. Require scalar length
  validation before all relevant list, lookup and retry row materialization.
- **MAJOR STORE-BUDGET-002:** Registered artifact reads charge declared bytes,
  but the reader admits actual file bytes up to its global limit before comparing
  declared length. An enlarged file can therefore allocate beyond the charged
  amount. Validate actual length on the opened no-follow handle before allocating
  and read through that same handle; avoid the metadata/open race.
- **MINOR:** General bundle artifact verification has per-file limits but no
  cumulative artifact-byte budget. The new publication-verifier scope does not
  establish a complete bundle-wide resource guarantee.

Russell independently ran 13 publication tests, 14 schema tests, Clippy,
formatting and diff checks successfully. Those tests do not cover the two hostile
preallocation cases above. Both MAJOR corrections and adversarial regressions
are assigned before integration; no BLOCKER finding was reported.
