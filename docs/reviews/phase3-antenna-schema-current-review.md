# Phase 3 antenna schema-validation candidate review

Date: 2026-09-23

Reviewer: `/root/phase3_schema_review`

Disposition: **APPROVE WITH FOLLOW-UP** for the bounded Draft 2020-12
schema-validation increment, subject to resolving the MINOR schema/runtime
text-acceptance mismatch below and obtaining a focused re-review before
integration.

Candidate: `0f1bfe26c0a58207ec06772ec90cf96cfa559ad3`, based on
`bc51e80b14e30f927628f4ba9f2e92a4773423fe`.

## Findings

**MINOR — trailing line terminators pass the JSON Schema but fail Rust import.**
With the pinned `jsonschema==4.25.1` Draft 2020-12 validator, start with
`crates/antenna-model/tests/fixtures/antenna-pattern-v1-valid.json` and append
`"\n"` to each of `model_id`, `source.license_spdx`, and
`source.source_uri` in separate copies. `Draft202012Validator.is_valid` returns
`True` for all three. The schema patterns for these fields use `$` as the end
anchor; the validator accepts the match immediately before a final line feed.
Rust's `validate_text` in `crates/antenna-model/src/contract.rs` accepts only
bytes `0x20` through `0x7e`, so the importer rejects those same documents.
This leaves external schema consumers with a wider text contract than the Rust
importer. Tighten the patterns to require the actual end of input and add
trailing-line-feed rejection cases for these fields, then rerun the schema and
Rust checks.

No BLOCKER or MAJOR finding was found in the scoped schema-validation change.

## Review and evidence

The authoritative plan anchors are PRE-006 in §6.7, PREB-001 and PREB-004 in
§18.7, and MAP-011 in Appendix I. These call for a versioned open antenna
representation with source and uncertainty metadata, antenna transform tests,
and a canonical antenna pattern library/import boundary. The schema candidate
adds structural Draft 2020-12 execution; it does not complete those broader
requirements.

The schema script imports `Draft202012Validator`, calls
`Draft202012Validator.check_schema`, validates the fixture used by the Rust
integration test, accepts an all-absent cross-polar plane, and rejects four
mutations for closed properties, a missing coordinate field, a missing
elevation endpoint, and a nonnumeric gain. The schema advertises the 2020-12
metaschema and the command verifies the installed package is exactly
`jsonschema==4.25.1`. The `validate-antenna-schema` developer route uses the
configured Python helper.

The shared fixture is included directly by the Rust test and has the same
versioned wire fields, source metadata, axes, frequencies, polarization
samples, and ordering expected by the importer. The requested command passed
in this review worktree. A no-site-packages run returned exit code 2 when
`jsonschema` was unavailable; a simulated installed-version mismatch
(`4.24.0`) also returned exit code 2. The tests exercise genuine schema
constraints, although they do not cover the trailing-line-feed mismatch above.

The validation note correctly distinguishes the local candidate command run
from integrated-main evidence and clean-bootstrap evidence. It states that
clean bootstrap was not tested and keeps Phase 3 open. It also explicitly
limits the evidence to synthetic patterns: no manufacturer licensing audit,
source checksum correspondence check, visual normalization review,
cut/harmonic import, adapter execution, or full product integration is
claimed.

The changed implementation and validation file SHA-256 entries match the
candidate ledger. `tools/ledger.py check` passes with 5,396 source blocks, 438
explicit ID occurrences, and 447 headings. PRE-006, PREB-001, PREB-004, and
MAP-011 remain `IN_PROGRESS`; related unimplemented plan obligations remain
open. `STATUS.md` also labels this as a separate candidate, not integrated
mainline evidence. No codebase graph or graph-coverage claim is made.

## Checks run

- `KYBERIA_TOOL_PYTHON=/Users/vincent/code/kyberia/.tools/supply-chain-schema-venv/bin/python python3 tools/dev.py validate-antenna-schema` — PASS; pinned Draft 2020-12 check and four structural rejections.
- `cargo test -p kyberia-antenna-model --locked --offline` — PASS; 10 integration tests, no unit tests or doctests.
- `python3 tools/ledger.py check` — PASS; 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `python3 tools/architecture.py check` — PASS.
- `python3 -m unittest tests.test_dev_commands` — PASS; 12 tests.
- `git diff --check bc51e80b14e30f927628f4ba9f2e92a4773423fe 0f1bfe26c0a58207ec06772ec90cf96cfa559ad3` — PASS.
- Missing-validator and mismatched-validator failure paths — PASS; each returned exit code 2.
- Trailing-line-feed comparison — finding reproduced; schema accepts the three mutated fields while Rust source validation rejects ASCII controls.

## Ten-field handoff

1. **Scope:** Independent review of the Phase 3 Draft 2020-12 schema-validation candidate; no product edits or integration.
2. **Plan and records:** PRE-006, PREB-001, PREB-004, and MAP-011; every corresponding ledger record remains `IN_PROGRESS`.
3. **Base/head:** Base `bc51e80b14e30f927628f4ba9f2e92a4773423fe`; candidate `0f1bfe26c0a58207ec06772ec90cf96cfa559ad3`.
4. **Inspected paths:** `STATUS.md`; antenna Rust test and shared fixture; `tools/validate_antenna_schema.py`, `tools/dev.py`, and `tests/test_dev_commands.py`; antenna schema and contract; validation document; ledger and generated traceability.
5. **Verdict/invariants:** APPROVE WITH FOLLOW-UP. Draft 2020-12 execution, pinned-version enforcement, shared-fixture acceptance, optional cross-plane omission, and the four structural rejection cases work. Resolve the trailing-line-feed text mismatch and re-review before integration.
6. **Checks:** Requested schema command, focused Rust test, ledger, architecture, focused developer-command tests, diff check, absent/mismatched validator paths, and the mismatch reproduction are recorded above.
7. **Tracking:** Changed file hashes match ledger references; source IDs and statuses remain consistent and open. `STATUS.md` separates candidate-local evidence from integrated evidence. Clean bootstrap and Phase 3 acceptance remain unproven.
8. **Report commit/worktree:** This artifact is committed separately on `review/phase3-antenna-schema-current`; only this report is in that commit. The worktree is clean after the report commit.
9. **Findings:** One MINOR interoperability mismatch: final line feeds in three text fields validate under JSON Schema but are rejected by the Rust importer.
10. **Blockers/limits:** No blocker to this bounded review. This is synthetic structural schema evidence; no clean bootstrap, licensed manufacturer data, external source/checksum verification, visual normalization, cuts/harmonics, adapter, or full Phase 3 acceptance was established. No graph evidence or coverage claim is made.
