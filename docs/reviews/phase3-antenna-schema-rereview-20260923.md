# Phase 3 antenna schema fix re-review

Date: 2026-09-23

Reviewer: `/root/phase3_schema_review`

Disposition: **APPROVE WITH FOLLOW-UP** for the three-field trailing-line-feed
fix. One related MINOR mismatch remains in `source_checksum_sha256`; resolve it
and obtain another focused review before integration.

Reviewed commit: `b91e193794f4acf6971f6d220006c9b32ac89111`, directly on
`0f1bfe26c0a58207ec06772ec90cf96cfa559ad3`. This worktree was not rebased or
merged onto the newer mainline.

## Finding

**MINOR — checksum pattern still accepts a final line feed.** Under the pinned
`jsonschema==4.25.1` Draft 2020-12 validator, append `"\n"` to the valid
fixture's `source_checksum_sha256`. The candidate schema still reports the
document valid because this pattern remains `^[0-9a-f]{64}$`. The Rust
`validate_sha256` check requires exactly 64 bytes and rejects the resulting
65-byte value. Add the same end-of-input guard to the checksum pattern and add
checksum trailing-line-feed cases to both schema and Rust tests.

## Re-review evidence

The fix addresses the three originally reported fields. The schema patterns
for `model_id`, `source.license_spdx`, and `source.source_uri` now append
`$(?![\s\S])`; with the pinned Draft 2020-12 validator, each corresponding
fixture mutation with a final LF returns invalid. The validator script adds
negative cases for all three, and the Rust integration test mutates each of the
same three JSON pointers and confirms the importer rejects them. The new Rust
test passes.

The schema change only tightens those three patterns. Other schema constraints
and the valid shared fixture remain accepted; inspection found no unrelated
acceptance broadening in this commit. The remaining checksum case shows that
the schema and Rust importer still differ on one field's final-LF handling.

The validation note and `STATUS.md` report 11 synthetic Rust tests, seven
schema rejection cases, and keep clean-bootstrap evidence, independent review,
and Phase 3 acceptance open. The PRE-006 catalog, PREB-001, PREB-004, and
MAP-011 ledger records remain `IN_PROGRESS`; the separate PRE-006 execution
adapter record remains `NOT_STARTED`. The changed schema, Rust test, validator,
and validation-document hashes match the ledger entries. The ledger check
passes; generated traceability continues to show the requirements as open.

## Checks run

- `KYBERIA_TOOL_PYTHON=/Users/vincent/code/kyberia/.tools/supply-chain-schema-venv/bin/python python3 tools/dev.py validate-antenna-schema` — PASS; Draft 2020-12 schema check with seven rejection cases, including the three patched text fields.
- Pinned-validator LF comparison — the three patched fields are rejected; `source_checksum_sha256` with final LF remains accepted.
- `cargo test -p kyberia-antenna-model --locked --offline` — PASS; 11 integration tests, no unit tests or doctests.
- `python3 tools/ledger.py check` — PASS; 5,396 source blocks, 438 explicit ID occurrences, and 447 headings.
- `python3 tools/architecture.py check` — PASS.
- `python3 -m unittest tests.test_dev_commands` — PASS; 12 tests.
- `git diff --check 0f1bfe26c0a58207ec06772ec90cf96cfa559ad3 b91e193794f4acf6971f6d220006c9b32ac89111` — PASS.

## Ten-field handoff

1. **Scope:** Focused independent re-review of the trailing-LF schema fix; no product edits or integration.
2. **Plan and records:** PRE-006, PREB-001, PREB-004, and MAP-011 remain open; the PRE-006 execution-adapter audit record is also `NOT_STARTED`.
3. **Base/head:** Base `0f1bfe26c0a58207ec06772ec90cf96cfa559ad3`; reviewed fix `b91e193794f4acf6971f6d220006c9b32ac89111`.
4. **Inspected paths:** Changed schema, Rust integration tests, validator, validation document, `STATUS.md`, ledger; Rust text/checksum validators; full fix diff.
5. **Verdict/invariants:** APPROVE WITH FOLLOW-UP. All three originally identified fields now reject LF in schema and Rust tests; a fourth analogous checksum case remains mismatched. No unrelated acceptance was broadened.
6. **Checks:** Pinned schema command, explicit LF mutations, Rust tests, ledger, architecture, focused developer-command tests, and diff check are recorded above.
7. **Tracking:** Changed source hashes match ledger references; all relevant completion statuses remain open. Docs continue to distinguish candidate evidence from integrated evidence and clean bootstrap.
8. **Report commit/worktree:** This report is committed separately on `review/phase3-antenna-schema-rereview-current`; only the report is in that commit. The worktree is clean after commit.
9. **Findings:** One MINOR follow-up: JSON Schema accepts an LF after the 64-hex source checksum, while Rust's exact byte-length check rejects it.
10. **Blockers/limits:** No blocker to the bounded re-review. The checksum mismatch should be fixed and re-reviewed before integration. Clean bootstrap, manufacturer rights/checksum correspondence, visual normalization, adapter behavior, and full Phase 3 acceptance remain unverified.
