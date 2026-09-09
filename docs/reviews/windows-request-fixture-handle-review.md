# Windows oversized request fixture handle review

Author: root. Independent reviewer: Russell. Disposition: APPROVED.
Static review confirms only the fixture writer lifetime changes.

Hosted b4c4c33 passes Windows lint and typecheck but reports the failing test
stored_analysis::tests::request_acquisition_reads_regular_bytes_and_rejects_invalid_sources
(check 102415026590). Source inspection shows File::create retains a writable
handle while read_request deliberately opens with FILE_SHARE_READ only.
That conflicts on Windows before the expected oversized-file error. The test
now drops its fixture writer immediately after set_len, before admission.

This is a code-supported diagnosis; the public annotation supplies the test
name but not its exact assertion trace. Production sharing and size checks
are unchanged. The focused test, CLI all-target Clippy with warnings denied,
and formatting pass locally. Hosted rerun remains required.
