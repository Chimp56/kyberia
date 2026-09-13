# Capture session and spool integration review

Integration branch: `integrate/capture-session-spool`, based on Windows process
candidate `7c4be7fa314d79f9800a987a73fc12da382cb57a`.

The unassociated acquisition spool culminates in reviewed source commit
`9bb4d65`. Independent Luna xhigh re-review `/root/spool_rereview` reports no
BLOCKER, MAJOR, MINOR or NIT findings and approves that commit. Twelve focused
spool tests pass. The serial crate suite passes 47 tests with one ignored, the
two external-port tests pass, and formatting, affected Clippy, architecture and
diff checks pass. The final cancellation regression uses two observations,
cancels after one local envelope clone, proves that neither manifest nor chunk
was published, and verifies exact reopen/retry.

The canonical capture-session series culminates in reviewed source commit
`d079761`. Independent Luna xhigh re-review `/root/capture_session_rereview`
reports no BLOCKER, MAJOR or MINOR findings and approves bounded integration.
It confirms original terminal/partial semantics, borrowed checked preflight
before mapping clones, exact serialized admission, allocation-free SQLite BLOB
length admission, closure across session/source/radio/BSS/raw references, and
post-commit retry behavior. The reviewer retains two NIT observations: bundle
verification reports the first corrupt session row rather than aggregating all
rows, and a process fixture failed once under parallel execution before passing
in isolation, serially, three repeated runs and the broader workspace run.

The orchestrator replayed the reviewed histories onto the current base rather
than merging their older branches. Equivalent identity-mapping and drain-timing
commits already present on main were not replayed. One documentation conflict
in the ADR index was resolved by preserving both the existing renderer ADR 0026
and planning interchange ADR 0027, then adding capture-session ADR 0028. No
production conflict occurred.

On the integrated tree, formatting and architecture checks pass. The combined
domain, project-store and observation-pipeline suites pass, including 59 of 60
pipeline unit tests with the signed native collector test explicitly ignored,
all two external-port tests, and all project-store suites. Traceability evidence
digests require regeneration because the reviewed integrations modify files
already referenced by earlier evidence; this bookkeeping must pass before the
branch is promoted to main.

This review approves the bounded durable session and unassociated spool
increments. It does not claim a live capture product workflow, streaming crash
journal, desktop command integration, hardware validation, or completed Phase 0.
