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
