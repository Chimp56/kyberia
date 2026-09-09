# Unassociated acquisition batch spool

This outward batch publication operation accepts an immutable
`ReceivedObservationBatch`, explicit provenance, publication UTC and
cancellation. The batch constructor has already validated native completion,
source reference hash/length closure, duplicate observation identities,
resource bounds and retention policy. The request cannot be built directly
from arbitrary observation arrays or an unchecked manifest.

`AcquisitionSpoolPort` publishes the existing versioned canonical capture
manifest, authorized retained raw artifacts, and an immutable observation
chunk/link. It shares the evidence publication implementation used by survey
persistence. It never creates a survey, snapshot, pose, capture clock or
transmitter identity. An empty native terminal persists its manifest only;
its original capability and terminal evidence remains inspectable.

Success means the batch is durable, not that native capture succeeded. The
outcome exposes the exact typed native completion, including reason and partial
flag. A nonempty successful capture has store status `Chunk`; partial/error
terminal captures may have status `Terminal` even when they include a chunk.
Receipts contain no snapshot. Publication
errors retain completed manifest/raw/chunk progress for retry. The existing
store checks exact content identity and immutable link closure. Reopening and
retrying a complete batch is idempotent. Payload-discard batches publish no
source-record bytes, while explicitly retained records retain verified closure.

This is not yet the CLI session coordinator or an in-flight crash journal.
A collector crash before it produces a validated batch remains a separate
supervisor outcome. Session identity allocation/mapping provenance, streaming
recovery, CLI commands, and survey UI acceptance are still required. No new
wire format or independent session identity is introduced by this increment.

Acceptance coverage in `spool_tests.rs` includes exact reopen/readback,
unchanged unknown fields, absence of snapshot association, idempotent retry,
raw-discard nonresolution, retained artifact closure, native empty/error/denied
terminals, negative time, read-only rejection and cancellation after the durable
manifest followed by retry. Existing associated-publication tests cover the
shared evidence path and its subsequent survey snapshot step. Independent
review is required before integration.

The retained-payload cancellation regression stops after raw evidence is
published but before normalized chunk publication. It drops and reopens the
bundle, checks exact retained bytes and absence of observation chunks, then
retries to one chunk with the same manifest hash and native completion. This
is a batch recovery check, not an in-flight process journal or completed
durable session identity gate.

## Additional independent failure regressions

The unexpected-trigger test invokes the production spool against an injected SQLite trigger, verifies Corrupt with no receipt and zero chunk/publication rows, removes only that injected trigger, then reopens and retries the exact batch.

The link-corruption test changes a published manifest row count at a cancellation checkpoint that returns false. Chunk publication succeeds but linkage rejects the mismatch. The error preserves the committed chunk receipt and leaves the publication unlinked. Repairing only the injected count and reopening allows retry with identical manifest/chunk hashes and no duplicate chunk. This is explicit test repair, not automatic production recovery.

Independent reviewer Laplace approved both additions without findings. All 11 spool tests, affected all-target Clippy with warnings denied, formatting and whitespace checks pass. Durable session integration and session-commit cancellation gates remain open.
