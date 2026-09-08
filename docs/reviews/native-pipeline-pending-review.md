# Native observation pipeline review — corrected and approved

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

## Follow-up review of 8d72b7b

Independent reviewer `/root/operation_log_luna` reports REQUEST_CHANGES with one confirmed MAJOR. The original four corrections and exact chunk-ID membership are present, but snapshot linking and readback still compare only association observation IDs. Caller-supplied survey equality does not prove that copied capture time, pose, raw reference, channel, dwell, calibration, result age, source/parser versions and quality match the canonical linked envelope. Opaque IDs are not content hashes. Require comparison of every copied evidence field, with same-ID contradictory-envelope regressions on admission and readback.

The reviewer passed workspace tests, Clippy, formatting, architecture, inventory and external port tests on the frozen candidate. Those checks do not disprove this defect. Correction ownership is now `/root/operation_log_luna` in the same isolated worktree; `/root` will independently review the correction. No native pipeline candidate is integrated.

## Final correction review

Root independently reviewed `3d7e9aa8fc07565fd9843690e94f6e22d11ef861` after the nonauthor review of `8d72b7b`. APPROVED: all listed MAJOR findings resolved. Integration commits are `c502be6`, `ee39551`, `7d8076c`, and `9f03337`. Earlier REQUEST_CHANGES statements above are historical findings, not current disposition.

The shared pure association helper compares every copied envelope field; storage also checks survey source/collector/adapter identity and capture mode. Both public link admission and readback enforce closure. Root independently ran 23 pipeline unit tests, two external-port tests, survey tests, focused Clippy with warnings denied, formatting and commit diff checks successfully. The hostile SQLite snapshot-link test demonstrates that readback rejects a valid same-ID snapshot with contradictory canonical evidence. Receipt timing remains distinct from RF capture time. Product capture controls, live acquisition spool and end-user survey workflow remain open.
