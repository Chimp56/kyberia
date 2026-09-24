# Phase 1 current-main history integration review

Disposition: **APPROVED** for the bounded survey-history API integration.

This record transcribes the independent, read-only 10-field review handoff from
`/root/phase2_ie_explorer_review`; the reviewer did not edit this report. The
review compared `f1a1caa00e6c37dd5281c1da017bcda75fae49be` through
`d200dbc83a3a51e28589c21616f9a9bfa83eb8e1` on a detached worktree at
`/private/tmp/kyberia-phase1-history-main-review-20260923`. The authoritative
`plan.md` digest was
`1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`.

The review verified that the public page types are exported from
`crates/application/src/lib.rs`, while
`ProjectSession::list_point_survey_snapshot_history` retains its legacy
`Result<Vec<_>, _>` signature and returns `ResourceLimit` if another page
exists. The separately named page method forwards cursor, limits, and
cancellation through the application port. Existing map import/calibration
APIs remain present; the integration adds history paging without losing those
current-main APIs.

The reviewer found no source or paging-logic blocker. The store retains its
64-item, 16 MiB, and 79,697-work-unit page ceilings; page failures are
all-or-error, and `Bundle::verify` drains pages. Filtering validates full
inventory/metadata, but does not read artifact bytes outside the selected
page; this limitation is documented. The 65-snapshot regression verifies
that the legacy wrapper errors rather than returning partial data and that the
page API traverses 64 + 1 entries.

One MINOR evidence-metadata finding was raised: the exact `d200dbc` checkout
had stale ledger source digests for `lib.rs`, `port.rs`, and `session.rs`. The
integration follow-up refreshes those digests in `docs/implementation/ledger.json`;
the clean-tree ledger check passes after that correction.

The reviewer independently ran formatting, architecture, source-inventory,
and diff checks, which passed. Cargo tests were not rerun by the reviewer.
After the review, the integration owner ran the combined locked/offline test
command for `kyberia-application`, `kyberia-project-store`, `kyberia-survey`,
and `kyberia-observation-pipeline` against the integrated `main`; it exited
successfully. Strict all-target Clippy and the formatting, architecture,
inventory, ledger, and diff checks also passed on the integrated tree.

`SUR-001`, Phase 1, and product acceptance remain `IN_PROGRESS`; this is
approval of the bounded history integration only. Codebase-memory MCP tools
were unavailable in the review session, so no graph coverage claim is made.
