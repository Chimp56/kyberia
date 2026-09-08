# ADR-0012: Keep receipt-based point association separate from capture admission

Status: Accepted bounded contract; native transport, storage, and UI integration remain open.

## Context

Plan §§7.1–7.4 and §10.16 require raw observations, normalized spatial facts
and derived point metrics to remain distinct. The native macOS managed-mode
collector can expose a source result receipt and an API request window, but it
does not expose the RF capture instant, scan cache age, channel dwell or
receiver pose. Passing the receipt timestamp to the strict point-survey
admission path would manufacture capture evidence and could make a stale or
uncertain result satisfy a quality gate.

## Decision

Add `PointSurvey::associate_received` and the V1
`PointObservationAssociation` fact in `kyberia-survey`. It assigns the
observation to the selected point anchor using either a known source receipt
time or a non-ambiguous API request window. The fact records its policy and
method versions, source response, point anchor, copied unknown/known envelope
fields and explicit temporal uncertainty. It is deduplicated and serialized
with the point receipt snapshot, while strict `admit` records and progress
remain unchanged.

Strict capture records and receipt associations share one observation-ID
namespace within a point snapshot. Admission rejects an ID already present in
the other plane, and snapshot validation rejects a collision. This prevents a
single canonical observation from being counted twice or being interpreted as
both strict capture evidence and receipt-only evidence during later replay.

The strict active-time clock (`last`) is separate from the event-ordering
watermark (`event_last`). An association may advance the ordering watermark so
that replay rejects an older response, but it cannot advance active duration,
dwell, metric counts or any strict readiness gate. A strict transition absorbs
the watermark by advancing its active-time clock and clearing the separate
event watermark. V1 association temporal uncertainty is exactly
`Unknown(NotMeasured)`; a known value or another unknown reason is malformed.

Receipt association can accept `ClockUncertain` because the flag is preserved
and prevents strict quality completion; it does not infer a clock
synchronization model.

## Alternatives

* Reinterpret `SourceResponseTiming.returned_at` as `CaptureTime`: rejected
  because source result emission can follow caching and API transport.
* Weaken `PointSurvey::admit` to allow unknown capture/dwell/pose: rejected
  because it would invalidate scan freshness and channel-completeness gates.
* Keep the association only in the macOS adapter: rejected because point
  spatial frames, active-window policy, deduplication and snapshot provenance
  belong to the survey boundary and must survive adapter replacement.
* Use host ingestion time: rejected because it is neither source capture nor
  source result time and is outside the canonical envelope contract.
* Allow one observation ID in both planes: rejected because cross-plane replay
  could double-count the same canonical evidence or change its meaning based
  on load order.
* Use `last` for association ordering: rejected because receipt-only timing
  would then silently become strict active-time evidence and could satisfy
  readiness.

## Evidence

`crates/domain/tests/reception.rs` proves that source response timing remains
separate from actual capture and dwell. `crates/survey/tests/association.rs`
proves in-window/outside/ambiguous boundaries, pause/resume, source/session/
frame validation, duplicate rejection in both observation planes, malformed
and future wire rejection, association bounds, exact uncertainty shape,
strict-gate isolation, resource-bound validation and deterministic replay.
The macOS adapter documentation records the source-specific fact that actual
capture time, cache age, dwell and pose are unknown.

## Consequences and reversibility

Point snapshots gain optional association and event-watermark fields under
their existing outer V2 receipt schema; empty legacy snapshots serialize
byte-for-byte as before. Existing strict survey records and downstream quality
accounting are unchanged. A future source with measured capture timing can
continue through `admit`, or add a more precise association basis without
changing existing facts. The association type and method are versioned so its
temporal policy can evolve without changing the observation envelope.

## Validation plan

Independent survey and architecture reviewers must inspect the code and tests.
Before this ADR becomes accepted, run the package and workspace format,
clippy, unit, migration, architecture and source-ledger checks. After native
transport and project storage exist, replay the same macOS stream through the
association policy, verify no unknown capture/dwell field is rewritten, and
verify a strict point cannot complete from associations alone or from their
event-ordering watermark.
