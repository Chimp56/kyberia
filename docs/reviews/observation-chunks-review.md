# Observation chunks review

Status: **APPROVED**

Scope: fixed-schema immutable Parquet observation chunks, transactional bundle
publication, cancellation and recovery, bounded decoding, and PyArrow
interoperability.

Independent review required three correction rounds. Initial findings were:

- dynamic schemas made valid V2 chunks incompatible in PyArrow datasets;
- generic artifact publication could label arbitrary bytes as normalized
  observations;
- resource limits were enforced after Arrow allocation.

The fixed contract was then reviewed adversarially. Follow-up findings required
full nested Arrow field comparison (including list child names and nullability),
an aggregate decoded-allocation budget before reader construction, and a
type-aware metadata limit that rejects list element 1,025 while accepting the
declared maximum of 1,024. Each finding was corrected with a regression that
observes the pre-reader rejection stage.

The final reviewer, who did not author the implementation or final correction,
reproduced the 1,024/1,025 list boundary, scalar-over-row rejection, nested child
name/nullability rejection, repeated-string decoded-budget rejection, exact
fixed schema, cancellation visibility, generic publication rejection,
projection rollback/recovery, and legitimate reopen/read round trips. It
reported no BLOCKER, MAJOR, MINOR, or NIT findings.

The same independent reviewer then inspected the integration resolution at
`f976958`. All eight combinations of the three optional SQLite table groups
(survey, operation, observation) passed historical open and writable-migration
probes. Partial groups and unknown tables, indexes, or views were rejected; the
authorizer remained least-authority; and verification checked both operation
and observation integrity. The combined schema regression is committed in
`3d91127`.

Validated commands included:

```text
cargo test -p kyberia-project-store --locked --offline
cargo test --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
python3 tools/architecture.py
python3 tools/source_inventory.py check
git diff --check
```

The PyArrow 25.0.1 oracle was run during the correction review through the
opt-in `KYBERIA_PYARROW_PYTHON` interpreter and combined known/unknown chunks as
one dataset with all 217 fields equal. The final review did not rerun that
optional interpreter gate; this does not replace future second-OS and sustained
production-load validation.
