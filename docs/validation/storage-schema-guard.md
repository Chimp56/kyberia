# Storage schema and write-integrity correction

Status: implementation frozen for independent root review; not self-approved.
Requirement: P0-MID-001 in `docs/reviews/phase0-midpoint-audit.md`, plan §§11, 15 and 16. This change corrects the implemented transactional container; it does not complete analytical storage Gate C or the final untrusted-project security boundary.

## Failure-first evidence

Before implementation, `cargo test -p kyberia-project-store --test schema_guard --offline --locked` produced six failing tests and one passing valid-bundle control. Failures reproduced accepted hostile triggers, stale-handle write suppression, accepted recursive views, unexpected schema objects, an extra singleton-table row hidden by the old predicate, and a metadata file over the new size policy. The generated-column fixture was then made a valid deterministic expression with mandatory successful fixture construction; the final test does not silently skip SQLite-rejected setup.

The original trigger is `CREATE TRIGGER hostile BEFORE UPDATE ON bundle_manifest BEGIN SELECT RAISE(IGNORE); END;`. Before the fix, `put_artifact` returned success while the committed manifest remained unchanged. The fix rejects this schema on open and rechecks it for existing handles before metadata writes/recovery. A test-only authorizer that suppresses column updates independently forces SQLite to report a matched row without persisting the intended values; authoritative readback detects this, rolls back and preserves the projection. This test would fail if the readback check were removed even with schema guards intact.

## Implemented boundary

- Configure SQLite defensive mode, disable views/triggers and set expression, column, SQL length, result length, VM-program and attachment limits before the first SQL statement.
- Inspect the built-in schema catalog only; require the existing canonical V1 manifest DDL and reject all other objects before selecting imported manifest content. No imported view is executed to determine whether it is compatible.
- Authorize only required operations. Reject recursion, functions, DDL, ATTACH, writable-schema changes and unsupported pragmas.
- Limit metadata file size to 64 MiB; reset a cooperative 2M VM-operation/five-second budget at public operation boundaries. A real ten-million-element recursive aggregate is interrupted by the progress backstop in isolation, then a fresh operation succeeds. Production authorization rejects that recursive SQL before execution.
- Validate exactly one singleton record, check one affected write row and exact authoritative readback inside the immediate transaction before projection publication and commit.
- Keep read schema validation and content in one transaction. Verification holds a read transaction through its SQL checks; filesystem race confinement remains separately open.

The physical DDL written to disk is unchanged. Existing V1 bundles reopen. Existing future-version compatibility tests continue to permit read-only inspection only when the same physical envelope is retained; write/recovery still reject future versions or required features. Rejecting extra SQL objects is explicit, not an implicit migration.

## Validation

```text
cargo test -p kyberia-project-store -p kyberia-cli --offline --locked
cargo fmt -p kyberia-project-store -- --check
cargo clippy -p kyberia-project-store -p kyberia-cli --all-targets --offline --locked -- -D warnings
git diff --check
```

Results: **PASS**, 23 project-store tests (3 unit guards, 13 existing bundle cases, 7 hostile-schema/round-trip cases) and 2 executable CLI workflows; formatting, clippy with warnings denied, and diff whitespace checks pass. Existing bundle/CLI workflows, stale-handle and future-version behavior, projection rollback and source checksums are included in the affected suites. Temporary fixtures are retained; no recursive deletion or destructive Git action is used.

## Limits

The SQL budget is cooperative; it is not a hard process, memory, filesystem-call or total wall-time limit. Native parsing/I/O stalls are not preempted. The existing assumption of no concurrent malicious filesystem replacement and the remaining SQLite sidecar/import isolation work are unchanged. Rejected writes can retain an orphaned immutable blob, as documented before this correction. A physical-schema change requires an explicit reviewed migration and fixtures. No extra external library, domain type, canonical format version or analytical backend is introduced; this uses the pinned rusqlite hooks feature already present elsewhere in the workspace.
