# Native observation pipeline

kyberia-observation-pipeline is the composition boundary between the
outward macOS normalizer, the inward receipt-association contract and the
project-store adapters. Its public batch contains only canonical
ReceivedObservation values. NormalizedCapture is unwrapped at the
composition edge; source-record bytes and capture-adapter DTOs do not cross
the inward contract.

The V1 pipeline validates the complete bounded batch and constructs every
receipt association in memory before invoking storage. It also carries a
typed capture manifest containing schema, collector build, capability
evidence, terminal status, source-record references and observation IDs in
source order. A normalized envelope is published to the immutable
observation-chunk path first. The resulting point state, which contains
receipt-only associations, is then saved as a survey snapshot. This order
prevents a committed survey association from referencing an observation that
is absent from durable normalized storage. The survey association contract
remains responsible for session, source, clock, active-window, duplicate and
unknown-evidence rules. The pipeline does not turn a source receipt into RF
capture time, pose, dwell or strict metric evidence.

`CapturePersistenceRequest` is an opaque application-owned value: its only
constructor is the association path in `ingest`, so a caller cannot pair a
public persistence port with an unrelated survey or manifest. A replacement
port can inspect the validated request through read-only accessors and return
validated manifest, chunk, snapshot and progress receipts through their public
constructors. The canonical `CaptureManifest` wire type lives in
`kyberia-domain`; project-store accepts that typed value and derives its
canonical bytes, hash, counts and terminal status rather than accepting
caller-supplied free-form manifest metadata.

CapturePersistencePort is a versioned inward port. Its receipts contain only
canonical content hashes, row counts, snapshot identities and project
manifest revisions. The Bundle implementation adapts those values to the
reviewed SQLite/Parquet chunk and survey-snapshot APIs; domain and survey
crates have no storage dependency. The port requires exact-idempotent
operations. Retrying the same batch and snapshot identity therefore converges
without a second chunk or snapshot when the underlying adapter can verify the
existing bytes. The project store keeps a bounded capture-publication row
linking the manifest hash to the chunk hash and survey snapshot ID. Link
updates are idempotent and can be replayed after a process crash between
separate storage transactions. The store decodes the typed manifest while
linking and checks the chunk's complete observation-ID set. Snapshot linking
also verifies the exact survey bytes supplied by the caller, every manifest
association's copied envelope fields against the linked canonical envelope,
and the survey session/source/collector/adapter identity and capture mode.

Chunk and snapshot publication remain separate storage transactions in this
bounded increment, with a recoverable manifest and link row. If the chunk
commits and the snapshot fails or cancellation arrives before the snapshot
call, the pipeline returns a partial progress receipt containing the manifest
and chunk. The caller can diagnose that state and retry the same request. A
future storage increment may join both metadata writes under one transaction;
the current protocol instead makes every intermediate state explicit and
replayable.

The UTC value in PipelineRequest is metadata publication time for the bundle
manifest. It is independent of the source response monotonic clock, the
unknown RF capture time in a native managed-mode observation and the
project-store manifest revision. Interleaved artifacts can advance the
manifest revision between observations and snapshots; no adapter may treat
that linear revision as a capture timestamp or a causal survey clock.

The pipeline accepts normalized partial, error, permission and empty captures.
Their terminal status, reason, partial flag, observation count and capability
evidence are persisted in the capture manifest; an empty capture publishes a
terminal manifest and an unchanged survey snapshot without manufacturing a
chunk. Duplicate, malformed, unsupported or source-mismatched observations
fail before durable publication. Each source record reference is checked
against its exact bytes. When the envelope policy is Retained, those bytes
are registered as immutable raw artifacts before normalized publication. When
the policy is Discarded or NotApplicable, the manifest records
NotRetained and the known raw references remain deliberately nonresolving.
