# ADR 0011: Explicit Wi-Fi signal aggregation and power arithmetic

- Status: Accepted
- Date: 2026-09-07

## Context

Plan §§7.9–7.10 require multiple visible RSSI aggregation choices, linear-domain power arithmetic, independently measured noise for SNR, and reproducible provenance. A hidden global average would change metric meaning across live inspection, spatial analysis, and reporting. Adding dBm values would be physically invalid.

## Decision

Kyberia owns these semantics in the pure `kyberia-wifi-semantics` numerical crate. Every aggregate carries a closed algorithm version, the complete method configuration, sample count, and contributing observation identities. Static methods canonicalize identity order. Streaming methods preserve acquisition order and reject mixed clock epochs or non-monotonic timestamps.

Power sums, linear-power means, and SINR denominators use scaled linear-milliwatt arithmetic. Empty or missing evidence remains explicitly unknown. SNR, SIR, and SINR accept evidence-bearing denominators and never insert a default noise floor.

## Alternatives

- A single configurable average in survey or UI code was rejected because method identity and reproducibility would depend on caller behavior.
- Direct arithmetic on dBm was rejected because logarithmic powers cannot be added that way.
- A fixed noise floor was rejected because it would present an assumption as measurement.
- A statistics dependency was unnecessary for these bounded one-dimensional algorithms and would add a wider dependency surface.

## Evidence

Independent-oracle unit tests cover known dBm/mW conversions, power sums, R-7 percentiles, each aggregate, SNR/SIR/SINR, unknown propagation, time ordering, duplicate observations, resource bounds, serialization, and property-based permutation/round-trip invariants.

## Consequences

Callers must choose a method explicitly and persist its result or configuration in the analysis manifest. Calibration, spatial interpolation, channel coupling, and physical fading remain separate operations. A zero-power denominator cannot be represented as finite dBm; absence of measured power remains unknown.

## Reversibility

The process boundary is not affected. New algorithms or changed numerical rules can be introduced with a new closed algorithm-version variant while retaining V1 replay.

## Validation plan

Run focused unit/property tests and workspace formatting, lint, architecture, and regression checks. Add canonical capture fixtures when source normalization feeds these aggregators, then validate end-to-end replay through analysis manifests and spatial layers.
