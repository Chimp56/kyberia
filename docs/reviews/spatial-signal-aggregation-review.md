# Spatial signal aggregation review

Date: 2026-09-07

Reviewer: `/root/spatial_review_luna`

Disposition: **APPROVED**

## Findings and resolutions

The first review rejected the increment because ADR 0012 collided with the
receipt-association ADR on `main`, and callers could pair an arbitrary metric
artifact with a different aggregation selection. The ADR is now 0013. A
private `MetricDefinitionBinding` can be created only by validating canonical,
bounded signal metric-definition bytes against exact artifact hash, length,
media type, version, method, parameters, and algorithm version.

The final NIT concerned a duplicate public tile field that hand-written Rust
could forge. Tile construction and its aggregation field are now private;
model output derives it from the verified binding, and serialization tests
require nested and top-level provenance to agree exactly.

The final review reports no BLOCKER, MAJOR, MINOR, or NIT findings.

## Validation reviewed

- spatial analysis: 23 focused tests passed;
- Wi-Fi semantics: 13 focused tests passed;
- complete locked/offline workspace tests: passed;
- workspace and focused Clippy with `-D warnings`: passed;
- formatting, architecture, source inventory, fixture, and diff checks: passed;
- source inventory: 97 locked packages; and
- 10k, 100k, and dense 100k-cell release benchmarks: passed.

## Boundary

The approved increment covers typed, deterministic static aggregation for
coincident measured signal samples and exact tile-v2 provenance. Temporal
spatial aggregation remains rejected until timestamped input evidence exists.
Manifest/storage binding, validated tile import, UI inspection, advanced
interpolation, uncertainty, and calibration remain open.
