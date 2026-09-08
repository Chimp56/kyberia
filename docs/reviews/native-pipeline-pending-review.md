# Native observation pipeline review — changes required

Candidate commits: fb789fd and 4e23874. Neither is integrated into main.
Independent reviewer: /root/channel_coupling_review_luna. Additional adversarial tests: /root.
Disposition: REQUEST_CHANGES. These are actionable implementation defects, not external blockers.

- MAJOR: empty-observation path lacks a cancellation check after manifest/raw publication and before snapshot publication.
- MAJOR: public persistence request constructor accepts a survey with no associations to the supplied observations. Root regression `persistence_request_rejects_unassociated_snapshot` fails on 4e23874 in an isolated worktree. Constructor must be internal/opaque or consume proven association output.
- MAJOR: successful receipt construction is private, preventing an external implementation of the public persistence port. Require a separate-crate successful port implementation test.
- MAJOR: public capture-manifest registration accepts arbitrary bytes plus independent counts; reads do not validate a versioned manifest. Require a typed validation boundary that preserves inward dependency direction.
- MAJOR (root review of correction draft): `Bundle::link_capture_chunk` validates row count without comparing the manifest's observation identities to chunk membership. `Bundle::link_capture_snapshot` validates project membership without proving the snapshot contains the capture's associations. Readback repeats these insufficient checks. A same-count unrelated chunk or same-project unrelated snapshot can therefore acquire a misleading capture-publication link. Require exact membership/association admission and readback validation, or an opaque validated binding API, with adversarial tests for both substitutions. The author has been notified; the correction draft is not approved.

Earlier root regressions for manifest/observation count mismatch and full raw-reference metadata mismatch are corrected in 4e23874. The reviewer reran 15 focused tests, full workspace tests, Clippy, formatting, architecture and source checks successfully; these checks did not establish the missing invariants above.

Correction ownership: /root/channel_coupling_review_luna, isolated native-observation-pipeline worktree. The correction author must receive a new independent review before integration. The observation-query implementation runs separately and may proceed without treating this candidate as approved.
