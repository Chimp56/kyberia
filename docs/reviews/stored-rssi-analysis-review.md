# Stored RSSI composition review

Candidate: `778cc5260da712f9e44e913fd93614e29e3f4bb4`.
Author: /root/operation_log_review_luna.
Independent reviewer: /root/channel_coupling_review_luna; root separately
inspected composition and reran focused tests.
Disposition: REQUEST_CHANGES. Candidate is not integrated.

## MAJOR: synthetic evidence promoted to measured output

The upstream `ValidatedObservedRssiSet::build` accepts
`SourceKind::SyntheticFixture`; its unusable-quality filter does not reject
`QualityFlag::SyntheticFixture`. Both its selection manifest and spatial
inputs unconditionally declare measured evidence. Existing selection tests
use synthetic sources with a synthetic-enabled survey. Thus a structurally
valid stored synthetic survey can produce a tile labeled measured, violating
plan §7.1. This defect exists in the integrated selection prerequisite and
reopens that review as well as blocking this composition candidate.

Require explicit rejection in the measured selector or faithful synthetic
evidence classification throughout. Add tests for both source identity and
quality-flag routes, an explicit output evidence-plane assertion, and an
independently calculated expected numerical result. The author is correcting
this before adding registered nearest/IDW definitions.

## MINOR findings

- The public stored document decoder checks envelope integrity, geometry,
  source artifact, grid dimensions and cell count, not complete numerical
  tile invariants. No current production importer promotes it to a validated
  tile; retain an explicit untrusted-output boundary until a numerical decoder
  exists.
- Snapshot provenance retains hash/revision but omits media type, length and
  richer session/point metadata. The run verifies these through project-store;
  replay-document completeness remains follow-up work.
- Strengthen the integration assertion beyond deterministic bytes and one
  cell to include the expected RSSI and evidence plane.

## Evidence

Revision binding, exact selected-chunk receipts, strict/receipt comparison,
snapshot hash validation, aggregate limits and cancellation otherwise passed
inspection. Reviewer ran focused stored-analysis tests (1 unit, 4 integration),
20 survey-snapshot tests, workspace tests, workspace Clippy, formatting,
architecture and 134-package candidate inventory successfully. Root separately
reran the five stored-analysis tests successfully. Those green tests did not
detect the evidence-plane defect and do not override this disposition.
