# Stored RSSI CLI pending review

## Final scoped disposition

APPROVED by `/root` for the combined Unix CLI increment through
`00051e7321657c5b1e16bb948a4c37a6121caf1b` (including `ba52325` and
`22d3b59`). Root independently reran all 20 CLI tests successfully at the frozen
final candidate. Static review confirms SIGINT registration is limited to the
analysis dispatch and post-link sync failures retain a typed committed outcome.
The injected sync-failure test verifies both artifact links remain intact.
The real subprocess suite covers numerical point/nearest/IDW outputs, unknown
synthetic evidence, invalid requests, FIFO rejection and SIGINT cancellation.

The findings below are retained as history and corrected by this series.
This approval does not establish non-Unix support, crash durability on other
filesystems, or a usable desktop survey workflow. Signal-dispatch isolation is
covered by the dispatch predicate test and static call-path inspection; the
actual subprocess signal test exercises the analysis command.

Disposition: REQUEST_CHANGES on in-progress draft, SHA-256
`87e3594e09c2f796226b0e1aac0cfeff7f915f7400e353801033fd51aaa585cc`. Reviewer: /root.

The Unix regular-file reader now opens with no-follow/nonblocking flags and
checks handle metadata before bounded reading; this corrects the earlier FIFO
blocking concern. Non-Unix capability behavior needs equally explicit semantics.

## MAJOR: final publication can overwrite a concurrently created file

The draft stages `.analysis.json.pending`, syncs it and uses `fs::rename` to
`analysis.json`. On Unix that rename replaces an existing final path. Creating
a new destination directory does not make subsequent file creation exclusive.
Use atomic no-overwrite publication (a same-filesystem hard link is suitable
for this single-file artifact) and retain the pending entry under the trash
policy. Add a deterministic final-conflict test preserving sentinel bytes.
This is a static finding; root did not mutate candidate files or publish output.

## Remaining acceptance

Run real subprocess tests for known values, unknown gaps, synthetic admission,
request/resource failures and existing outputs. Cancellation is still an
implementation gate: a bounded backend wired to `NeverCancel` does not provide
CLI signal cancellation. Treat any initial bounded command acceptance as an
increment and implement cancellation next, with honest documented guarantees.
No product workflow completion is claimed by this draft review.

## Frozen candidate checks

At `ba52325`, root independently passed all 12 CLI tests. Root decoded the
retained known artifact and confirmed -55 dBm with Observed classification.
Two additional real CLI invocations using that original fixture moved the grid
center one meter from the sample: nearest and IDW each returned -55 dBm with
Interpolated classification. These are independent runtime spot checks, not
persistent regression coverage. The author was asked to add numerical/class
assertions and actual method-selection subprocess cases to the ongoing follow-up.
The hard-link publication correction is present; final independent review of
the frozen candidate is still pending, and signal cancellation remains open.

Fresh independent reviewer /root/operation_log_review_luna also passed the
12 CLI and six stored-analysis tests plus Clippy, formatting, architecture
and inventory. Disposition remains REQUEST_CHANGES: missing persistent numeric
and real nearest/IDW command assertions is MAJOR, despite root's spot checks.
The author is preparing a separate corrective test commit before integration.

## Cancellation candidate 22d3b59

Root independently passed all 18 tests, including actual SIGINT and permanent
numerical/method regressions. The earlier numeric test-strength finding is
corrected. REQUEST_CHANGES remains for two publication/signal findings:

- MAJOR: main installs the SIGINT flag handler for every CLI command, but only
  stored analysis polls the flag. Other commands lose normal interruption
  behavior. Limit registration to the analysis command and verify unrelated
  commands retain their default behavior.
- MAJOR: final hard-link creation is the commit point, but a subsequent
  directory-sync failure propagates as an ordinary error without the committed
  state. Return an explicit committed/durability-failure outcome and test the
  post-link failure path; do not imply an absent output after publication.

Corrections are isolated in `fix/cli-cancellation-scope`; no candidate is
integrated before these findings are resolved and reviewed.
