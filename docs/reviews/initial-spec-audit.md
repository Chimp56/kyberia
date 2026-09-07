# Independent initial specification audit

Reviewer: `qa_spec_audit`, initially read-only. Scope: all `plan.md`, empty repository and Git history, initial environment. No production implementation existed to review. The same agent subsequently authored ledger tooling; this document is **not independent review of that tooling**. A different reviewer must inspect the implementation before integration.

## Findings

| Severity | Finding | Required resolution |
|---|---|---|
| BLOCKER for ledger correctness | 97 original IDs recur across 438 definitions; many refer to distinct requirements | Namespace source occurrences and retain all definitions; never deduplicate by bare IDs |
| MAJOR | Section 20 assigns ADR-012 through ADR-016 twice | Allocate unique durable ADR files while retaining original aliases |
| MAJOR | Catalog/backlog-only tracking omits normative methodology, security, validation and audit obligations | Losslessly cover all 446 headings and subordinate blocks; retain all 80/172 Appendix I rows |
| MAJOR | Mock/replay success cannot validate hardware or external runtime adoption | Track contract versus runtime/hardware/measured gates explicitly |
| MAJOR | A point-only MVP cannot satisfy Phase 1 | Include manual continuous paths, snapshots, barrier-aware IDW, native desktop workflows and source-inspectable maps |
| MINOR | Sionna job names differ between §8.5 and Appendix I | Select one versioned canonical vocabulary and explicit aliases |
| MINOR | §19 and Appendix I sequence native and Kismet offline work differently | Canonical contracts first; keep independent native/offline proofs parallel and live Kismet after offline mapping |
| MINOR | Appendix I suggests maintainer outreach before interchange, while external messages need user authorization | Prepare local RFC/fixtures; obtain authorization for actual outreach; do not block independent bridge work on acceptance |

Examples of semantic collisions: backlog `FND-001` means units, audit `FND-001` means desktop shell; catalog `PAS-002` means measured noise, audit `PAS-002` means SIR/SINR; catalog `PRE-007` means calibrated hybrid modeling, audit `PRE-007` means worker lifecycle; catalog `OPT-005` means robustness, audit `OPT-005` means installation constraints. All 13 `UX` identifiers are reused with different meanings.

## Earliest implementation acceptance

Strong units should reject dBm+dBm, Hz+meters, pixels-as-meters and cross-clock duration mistakes. Unit constructors and deserialization must reject nonfinite numbers and invalid ranges. Unknown/noise/unsupported/failure states must survive round trips distinctly. Position covariance needs positive-semidefinite validation, and monotonic clocks need source/session identity.

Observation schemas must preserve adapter/source/version, timing uncertainty, channel/dwell, calibration, quality and raw provenance. Compatibility tests should accept permitted additive fields and reject unsupported required semantics. Architecture tests must detect transitive forbidden imports and production references to research-only dependencies.

The first bundle/CLI acceptance should create actual SQLite projects, round-trip manifests/assets, and detect corruption, unsupported schemas, absent chunks, hash mismatches, and interrupted creation. Seeded synthetic fixtures must remain explicitly synthetic. Analysis hashes must change for semantic inputs and remain stable under canonical map-key reordering.

Durable Gates A–I remain unresolved until their stated experiments pass. A technology recommendation or ADR draft alone does not validate the renderer, storage split, native capture, geometry kernel, optimizer, active integration, licensing, Kismet, or Sionna decision.

## Open scope

Every product phase was unimplemented at this audit. Hardware/runtime evidence, actual user workflows, numerical quality, storage safety, performance, and security review remain open. Recommended first bounded implementation is canonical units/IDs/unknowns/provenance plus transactional project/CLI invariants, independently reviewed before collectors and UI depend on them.
