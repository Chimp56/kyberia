# Bounded observation-ID queries

`Bundle::read_observation_selection_by_id` is the project-store query boundary
for a small analysis selection. It uses the primary key on
`observation_chunk_members.observation_id` to locate the immutable chunk(s)
that contain the requested IDs. It does not enumerate or decode unrelated
chunks.

The query accepts at most 4,096 distinct observation IDs, returns them in
canonical `ObservationId` order, and reports every missing ID explicitly.
Duplicate request IDs are invalid. Selected chunks are bounded to 128 chunks,
262,144 decoded rows, and 64 MiB of artifact bytes. These limits bound the
full-chunk decode required by the current Parquet reader even when a caller
selects only one row from a chunk.

Before an envelope is returned, the adapter validates the selected descriptor,
schema and codec versions, manifest artifact entry, provenance metadata,
artifact length and SHA-256, canonical decoded row metadata, and the selected
member index's observation/source/session/ordinal values. This is query-scoped
verification: a corrupt unrelated chunk does not make a valid selected query
fail, while `list_observation_chunks`, `read_observations`, and `verify` remain
the whole-bundle integrity paths.

The selection method returns canonical envelopes together with an
`ObservationQueryReceipt`. The receipt contains the project manifest revision
captured at selection start and the exact verified selected descriptors sorted
by chunk hash. The revision is checked again after selected verification; a
concurrent project change fails explicitly instead of producing a receipt that
spans revisions. An empty selection still reads and reports the current
project revision and has an empty descriptor list. The receipt contains
metadata only, never artifact bytes or unrelated project data.

`read_observations_by_id` and `read_observations_by_id_with_cancel` remain
compatibility wrappers that return only envelopes. The cancellation-aware
selection method accepts the existing cancellation port. Cancellation is
checked before database work, during indexed lookup, and between selected
chunk decodes. A cancelled request never returns a partial selection or
receipt, including when cancellation arrives during final revision checking.
All methods are read-only and are safe to use after reopening a bundle.

`ObservationChunkStore` is a sealed project-store adapter API. Only the bundle
can implement it because constructing a receipt requires verification of
authoritative SQLite metadata and selected artifact bytes. A future inward
application query contract should map this API and keep project-store types
out of domain code.

The returned `ObservationEnvelope` remains the canonical source record. This
query does not filter BSSIDs, assign spatial positions, reinterpret timestamps,
or construct analysis samples. Those policies belong to the outer survey and
analysis composition boundary, which must retain the selected chunk hash and
provenance when creating its immutable input manifest.

Focused validation lives in
`crates/project-store/tests/observation_chunks.rs`. It covers deterministic
ordering and reopen equality, stable exact receipts, empty-selection revision,
concurrent revision changes, final and empty-selection cancellation, missing
and duplicate IDs, selected index/provenance tampering, resource limits, and a
128-chunk fixture where unrelated artifacts are corrupt but a one-ID selected
query remains available. The latter demonstrates that selection work is
bounded by selected chunks rather than project-wide chunk decoding.
