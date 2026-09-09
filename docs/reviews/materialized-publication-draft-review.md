# Materialized publication draft review

Disposition: **REQUEST_CHANGES**. Independent reviewer:
`/root/operation_log_review_luna`. Scope: evolving, uncommitted storage draft
in `.worktrees/materialized-project-publication`, based on `7427cb1`.
These findings do not describe integrated production code.

- **BLOCKER:** Publication accepts a caller-owned baseline and stores it without
  comparing it to an authoritative persisted baseline. A valid same-project-ID
  aggregate is not sufficient proof. Define explicit baseline registration and
  transactionally validate the registered identity on publication.
- **MAJOR:** Read/verify does not reconstruct the historical operation set to
  validate its recorded identity. Define immutable prefix membership after
  later appends, and compare exact bytes, count and operation revision.
- **MAJOR:** Current-set stale checks precede exact publication lookup. An
  uncertain successful commit followed by another append must still allow an
  exact retry to return the verified existing record without new revisions.
- **MAJOR:** Persist and validate explicit publication, materializer, identity
  and result schema/version bindings. Do not infer result schema from counters.
- **MAJOR:** Recompute publication identity on read; matching a forged row ID
  to a forged current pointer does not establish integrity.
- **MAJOR:** Align admission and stored row limits; reject historical bundle
  revisions beyond the committed manifest before reading artifacts.

The author has been asked to provide the baseline-registration, historical-set,
retry and version contract before expanding the migration further. Dependency
direction and optional schema-group wiring appear aligned, but the draft has
no frozen test evidence and is not approved. No external dependency blocks
these corrections.
