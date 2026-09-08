# ADR-0018: Compose native observations with receipt association and durable chunks

Status: Accepted bounded composition increment; native runtime and co-transaction optimization remain open.

## Context

The macOS normalizer already produces canonical ReceivedObservation values.
The survey core already records a receipt/API-window association without
claiming that source timing is RF capture timing. The project store already
publishes immutable normalized observation chunks and checksummed survey
snapshots. Before this increment, no reviewed composition boundary connected
those contracts. Saving a receipt association before its canonical observation
would create a durable reference to missing evidence.

## Decision

Add kyberia-observation-pipeline as a composition-layer crate. It unwraps a
completed NormalizedCapture into a bounded, duplicate-checked
ReceivedObservationBatch, then exposes only canonical observations and
survey state through the versioned CapturePersistencePort. The Bundle
adapter implements that port using the existing observation-chunk and survey
snapshot APIs.

The pipeline performs all PointSurvey::associate_received transitions before
any write. It persists a typed capture manifest first, publishes retained raw
records only when the capture policy permits them, publishes the complete
normalized envelope batch, and then persists the resulting point state and its
receipt-only associations. A bounded SQLite publication row links the
manifest hash to the chunk hash and snapshot identity. Every step is
exact-idempotent and a failure after any committed step carries progress so a
retry can complete the links without erasing history.

The canonical capture-manifest DTO is owned by `kyberia-domain`, which keeps
the project-store dependency pointed inward. Its bounded constructor and
strict canonical decoder reject unsupported schema/method versions, duplicate
observation identities, contradictory completion counts and duplicate source
hashes. `kyberia-project-store` accepts only this DTO with validated
publication context and derives all stored manifest metadata from it; free-form
manifest bytes and caller-supplied counts/status are not a storage contract.
`CapturePersistenceRequest` is created only by the association path, while
public validating receipt builders keep replacement persistence ports
implementable outside the composition crate.

Capture output links are also admitted from the canonical manifest: chunk
linking compares the complete observation-ID set, and snapshot linking checks
the exact saved survey plus presence of every manifest observation association.

PipelineRequest.published_utc_ms is bundle metadata publication time. It is
not source response time, RF capture time, monotonic survey time or project
revision. The manifest revision is a linear storage commit counter shared by
interleaved artifact groups. No pipeline adapter may reinterpret it as a
causal or measurement clock.

## Alternatives

* Pass NormalizedCapture into survey or domain: rejected because adapter DTOs
  and raw source retention policy would leak inward.
* Save the survey snapshot first: rejected because it could reference
  observations that are not durably indexed.
* Copy source-record bytes into the normalized chunk: rejected because raw
  retention is an explicit policy and the reviewed chunk contract stores
  canonical envelopes.
* Substitute receipt/API-window timing for capture time or strict admission:
  rejected by the existing survey and observation contracts.
* Add a new combined SQLite transaction in this crate: deferred because the
  current store APIs are separate reviewed transactions; the recoverable
  project-store publication row provides explicit crash recovery first.

## Evidence

* [lib.rs](../../../crates/observation-pipeline/src/lib.rs) defines the V1
  batch, typed capture manifest, source-order metadata, port and
  partial-publication error.
* [bundle.rs](../../../crates/observation-pipeline/src/bundle.rs) adapts the
  existing Bundle chunk and snapshot APIs without exposing their storage types
  in the inward port.
* [tests.rs](../../../crates/observation-pipeline/src/tests.rs) proves
  normalization, association, unknown preservation, rejection, cancellation,
  exact retry, partial failure and reopen/replay.
* [native-observation-pipeline.md](../../validation/native-observation-pipeline.md)
  records the focused and repository validation gates, including empty/terminal
  and raw-retention cases.
* [survey-association.md](../survey-association.md) and
  [project-bundle.md](../project-bundle.md) define the source timing,
  association and durable chunk/snapshot contracts consumed here.

## Consequences

Native normalized observations and empty terminal captures can reach durable
canonical storage and a receipt-based point snapshot through one typed
composition call. Unknown capture time, pose, dwell, cache age and calibration
remain unchanged, and receipt associations cannot satisfy strict point
metrics. Raw references are either closed by retained artifacts or explicitly
marked NotRetained. A partial result is diagnosable and retryable, while chunk,
snapshot and capture-manifest commits can have distinct manifest revisions when
artifacts interleave.

## Reversibility

The crate and its one additive capture-publication table are backward
compatible: old bundles gain the table only on a writable open, while
read-only opens remain inspectable. Removing this composition adapter leaves
the versioned normalizer, survey association and project-store contracts
intact; already committed chunks and snapshots remain readable by their owning
adapters. Replacing it with a co-transactional adapter requires
preserving the V1 batch semantics, publication order or an equivalent stronger
atomicity guarantee, receipt-only association meaning and exact-idempotent
retry behavior.

## Validation plan

Run the focused pipeline tests, project-store tests, workspace format and
clippy gates, architecture dependency validation and source inventory check.
Review the test evidence for malformed/future input, source/time isolation,
unknown preservation, cancellation, partial publication, duplicate retry and
reopen/replay. Before product completion, add a project-store API that can
co-commit the chunk and survey snapshot metadata or document an equivalent
recovery protocol, then exercise it with crash/failure injection at each
publication boundary. Separately validate actual CoreWLAN transport,
authorization, raw retention and UI command wiring on supported hardware.
