# Phase 3 antenna-pattern candidate review

Date: 2026-09-23

Reviewer: `/root/phase3_antenna_independent_review`

Disposition: **APPROVED for the bounded synthetic v1 contract/evaluator increment**

Candidate: `442860030e7228773fcf85a3c2612a0298e6e4bd`, based on
`05953134d24666e8483cbfdb7d9aacd0ce4e6e48`. The implementation source commit
is `b86bd130dc77443fe53a6840f28cdc9d5e98b4a6`; later candidate commits add
validation and source-qualified ledger records.

## Findings

No BLOCKER or MAJOR correctness finding was found in the reviewed bounded
contract/evaluator. Two MINOR follow-ups remain before treating the schema as a
portable external interchange contract:

1. `crates/antenna-model/tests/antenna_pattern.rs:395-417` parses the published
   schema as JSON and inspects selected fields, but does not run a Draft 2020-12
   validator against accepted and rejected fixture documents. Runtime/schema
   agreement was checked by source inspection and the runtime tests, not by an
   executable schema-conformance test. Add that conformance test before other
   consumers rely on this schema independently of the Rust importer.
2. `crates/antenna-model/tests/antenna_pattern.rs:164-216` exercises linear-power
   interpolation along azimuth, the periodic seam, and frequency, but not an
   interior elevation interpolation. The vertical rows in the fixture only
   distinguish the equator from the poles, and no non-pole elevation lookup is
   asserted. Add an elevation midpoint (ideally also an irregular-axis) golden
   case to establish the second bilinear dimension and sample flattening.

The following boundaries are explicit in the candidate documentation and are
not approval blockers for this partial slice:

- The source digest is syntax-checked only; the importer does not fetch the URI
  or compare the digest with external source bytes. Likewise, the SPDX parser
  checks expression shape, not license rights or the authoritative SPDX
  identifier/exception registry. The README correctly assigns those checks to
  the caller. Do not treat these fields as a licensing audit.
- Normalization currently means that each frequency's nominal gain agrees
  with the maximum co-polar grid sample within uncertainty plus tolerance. It
  does not integrate the pattern over the sphere to establish consistency
  between gain and efficiency, and no visual pattern review was run.
- Uncertainty is stored for nominal gain and efficiency, but not per direction
  or as an uncertainty returned by `evaluate_gain`. Downstream planning must not
  present the returned point gain as uncertainty-aware until an explicit
  pattern-sample/interpolation uncertainty contract is added or justified.
- V1 supports one full-sphere grid only. Manufacturer-format import, azimuth/
  elevation-cut reconstruction, harmonics, a visualizer, polarization mismatch
  loss, and a Sionna adapter remain open. The contract is not field-validated
  against licensed antenna data and must not be promoted as a complete antenna
  catalog or predictive-planning feature.

## Design and invariant review

The v1 wire contract is closed and versioned. The JSON Schema and Rust wire
types agree on required fields, discriminator values, printable text bounds,
grid sample shape, and optional-or-null cross-polar values; Rust adds the
documented relational rules. Unknown fields and unsupported enum values fail
closed. Frequency and angular axes are strictly increasing; the grid has
explicit flatten order, starts at azimuth zero, excludes a duplicate 360-degree
seam, and includes both poles. Pole rows are constrained to azimuth-invariant
gain, avoiding direction-dependent lookup at a singular coordinate.

The evaluator normalizes finite world directions with scaling before the norm
calculation, rejects zero/non-finite directions, applies the conjugate of the
validated local-to-world WXYZ quaternion, and uses the stated local XYZ/degree
convention. The quaternion is norm-checked, normalized within tolerance, and
canonicalized across antipodes and signed zero. Frequency lookup rejects
out-of-range values. Spatial azimuth/elevation and frequency interpolation are
performed in linear power and converted back to dBi; the azimuth bracket wraps
across the seam. Missing cross-polar data returns an explicit unsupported error
rather than inventing a value. The selected co/cross planes are not a
polarization-mismatch calculation, consistent with the declared boundary.

Numeric validation bounds gain, uncertainty, efficiency intervals, dimensions,
frequency count, bytes, JSON depth, and deterministic validation work. Checked
dimension multiplication prevents overflow before sample traversal. JSON
deserialization is bounded by the input-byte ceiling, and canonical identity is
computed over the validated/canonicalized contract with a domain separator.
The generic error enum separates invalid input, unsupported requests, resource
limits, and JSON decoding failures. The model remains isolated as a numerical
crate with only `serde`, `serde_json`, and `sha2`; no UI, persistence, vendor
catalog, or Sionna objects cross into it.

## Validation reviewed

I independently ran these commands in the assigned review worktree:

- `cargo test -p kyberia-antenna-model --locked --offline` — PASS, 10
  integration tests; no crate unit tests or doctests are defined.
- `cargo clippy -p kyberia-antenna-model --all-targets --locked --offline -- -D warnings`
  — PASS.
- `cargo fmt --package kyberia-antenna-model -- --check` — PASS.
- `python3 tools/architecture.py` — PASS, reviewed dependency directions and
  external package boundaries.
- `python3 tools/source_inventory.py check` — PASS, 522 locked external
  packages.
- `python3 tools/ledger.py check` — PASS, 5,396 source blocks, 438 explicit ID
  occurrences, and 447 headings.
- `git diff --check` — PASS.

The tests cover canonical axes and a Z-axis mount rotation, poles and azimuth
seam, linear-power azimuth/frequency interpolation, out-of-range frequencies,
co/cross availability, signed-zero and antipodal identity canonicalization,
invalid metadata/grid cases, byte/depth/custom-work limits, and zero/extreme
direction vectors. The minor test follow-ups above are not covered by those
checks. No broad workspace build, manufacturer dataset, visual comparison,
hardware test, or Sionna execution was performed or is claimed.

## Traceability and acceptance boundary

The candidate correctly records `catalog:PRE-006:1`, its implemented leaves,
`backlog:PREB-001:1`, `backlog:PREB-004:1`, and `audit:MAP-011:1` as
`IN_PROGRESS`, not complete. Cut/harmonic support, visual normalization, and
3D visualization remain open; section 8.8's polarization-mismatch obligation
also remains `NOT_STARTED`. `STATUS.md` labels the work as a candidate pending
independent review and integration and lists several omissions.

The implementation path hashes in `docs/implementation/ledger.json` match the
reviewed files. The source revision points to the implementation commit; the
validation record points to its validation-document commit. The Cargo source
inventory hash matches `Cargo.lock`. Architecture, inventory, and ledger checks
all pass. I found no claim that Phase 3 or PRE-006 is complete.

This approval is only for integrating the bounded importer/evaluator as
`IN_PROGRESS` work. It is not approval to claim that the plan's complete
antenna library, uncertainty-aware prediction, visual validation, or Phase 3
acceptance gates are met.

## Ten-field handoff

1. **Objective/scope:** Independently review the Phase 3 antenna-pattern
   contract/evaluator against plan §§6.7, 8.8, 18.7, Appendix I MAP-011, and
   current source-qualified evidence; no candidate-code edits.
2. **Requirements/anchors:** `catalog:PRE-006:1` and leaves,
   `backlog:PREB-001:1`, `backlog:PREB-004:1`, `audit:MAP-011:1`; plan §§6.7,
   8.8, 18.7, and Appendix I.
3. **Base/head:** Base `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`; reviewed candidate
   head `442860030e7228773fcf85a3c2612a0298e6e4bd`.
4. **Changed paths reviewed:** `Cargo.lock`, `STATUS.md`,
   `crates/antenna-model/{Cargo.toml,README.md,schema/antenna-pattern-v1.schema.json,src/contract.rs,src/evaluate.rs,src/lib.rs,tests/antenna_pattern.rs}`,
   `docs/implementation/{TRACEABILITY.md,ledger.json}`,
   `docs/licenses/cargo-sources.json`,
   `docs/validation/phase-three-antenna-current.md`, and
   `tools/architecture.json`.
5. **Design/invariants assessed:** Schema/runtime shape, units/axes, quaternion
   direction mapping, interpolation and seam behavior, pole constraints,
   numeric/provenance validation, resource limits, errors, and architecture
   boundary; substantive findings and limits are above.
6. **Tests/checks:** Focused test, Clippy, fmt, architecture, source inventory,
   ledger, and diff checks all pass in this review worktree; exact scope and
   test coverage are recorded above.
7. **Traceability/status:** Hashes verify and all relevant tracked records stay
   `IN_PROGRESS`; no Phase 3/PRE-006 completion claim is justified.
8. **Report commit/worktree:** This report is committed separately on
   `review/phase3-antenna-current-review-20260923`; only the report is in the
   commit. The review worktree is clean after the commit.
9. **Findings/residual risks:** No BLOCKER/MAJOR; two MINOR test follow-ups.
   Source-license/checksum verification, full pattern normalization,
   direction-dependent uncertainty, polarization mismatch, formats, visual
   review, and external/product integration remain explicitly out of scope.
10. **Blockers/limitations:** None blocking this bounded review. No codebase
    graph tools were available in the parent environment; source and plan
    fallback was used. No broad workspace or real antenna-data validation was
    run.
