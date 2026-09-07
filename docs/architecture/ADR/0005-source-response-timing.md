# ADR-0005: Preserve source response timing separately from capture

Status: Accepted after independent domain review by `/root/sionna_rt`; native normalization is independently reviewed and integrated. Storage and survey composition remain open.

## Context

Plan §7 evidence semantics, §10 inward contracts and the canonical observation audit require honest timestamps and provenance. Native CoreWLAN exposes a result retrieval/emission time and an API-call window, while actual RF capture time, cache age and channel dwell remain unknown. The existing V2 observation envelope correctly represents those capture unknowns. Losing the source response times would prevent later auditing; inserting them into capture or dwell would change their meaning.

## Decision

Add canonical `SourceResponseTiming` and a separately versioned V1 `ReceivedObservation` wrapper in the inward domain. The wrapper pairs a validated observation with known/unknown source response timing. It does not change the V2 observation or its V1 migration. Source response timing contains the source's result timestamp and an optional monotonic API-call window. `returned_at` means the timestamp at which the source exposes the result record, which can follow API completion. It is not necessarily the API-window endpoint or the receiving application's ingestion time.

Adapters return the canonical pair, preserving absent capture time, cache age, dwell and pose. No pure domain operation reads a clock. A source response and API window must use the same explicit monotonic epoch when both are known, and the response cannot precede API completion. A capture timestamp is compared with response time only when their monotonic epochs are explicitly equal. Cached results may predate the API window; different hardware/source clocks are not implicitly synchronized. Wall times from separate clocks are not ordered without a clock model.

## Alternatives

An adapter-owned receipt leaves durable provenance outside Kyberia's shared contract. Adding response fields to the current envelope would require another envelope/snapshot migration for a separable acquisition concern. Reusing capture time or dwell would be scientifically incorrect. Dropping response timing would lose available evidence.

## Evidence

The original [macOS fixture](../../../collectors/macos/fixtures/valid.ndjson) records API window 2900–3500 ns and result timestamp 4000 ns while capture/cache/dwell remain explicitly unknown. Six [domain tests](../../../crates/domain/tests/reception.rs) cover preserving those distinctions, cached results, clock mismatch, impossible ordering, strict wrapper decoding and exact large monotonic values.

## Consequences and reversibility

Storage and survey composition must retain the pair when acquisition timing matters. The current strict point-survey admission policy still requires actual capture evidence and is not relaxed by this wrapper. A later explicit position-assignment policy may use receipt timing with appropriate quality/uncertainty labels; it must not rewrite raw capture. Host ingestion time, transport sequencing and source raw-field preservation remain separate acquisition concerns. The wrapper can evolve independently from nested observation versions, and existing envelopes remain readable without fabricated receipts.

## Validation plan

Run domain and workspace regressions, independently review the temporal invariants, then test actual macOS normalization against the original native fixtures. Validate storage roundtrip and receipt-based assignment policy separately before claiming a usable point survey. Required field and duplicate-key decoding must fail; unknown values must survive serialization.
