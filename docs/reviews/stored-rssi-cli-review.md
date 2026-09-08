# Stored RSSI CLI pending review

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
