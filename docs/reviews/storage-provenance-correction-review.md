# Independent review: required storage provenance correction

Date: 2026-09-07. Reviewer: `rf_numerical_review`, independent of the root correction author. **APPROVED for the bounded STO-001 correction**. No unresolved BLOCKER, MAJOR, MINOR or NIT findings in this correction. The broader storage experiment, Gate C ADR and benchmark conclusions remain under separate review.

## Scope and corrected finding

Root found MAJOR STO-001: the research row validator allowed `None` in required `assignment_version` and `adapter_version` text. Engine behavior then diverged: root reported SQLite/DuckDB accepting null provenance while Parquet rejected it. The correction rejects null for every nonnullable text field before engine-specific work begins. The independently reviewed relevant plan requirements are §3 evidence/provenance, §7.1/7.20 assignment semantics, §11 schema/storage contracts, §14 error boundaries, §16 migrations/recovery and Gate C. This is a research projection, not a claim to implement the complete canonical observation schema.

Reviewed the new `Row.validate` null guard, both affected test methods, generated field lists, Arrow nullability and each `Store.append` path. The required text set is exactly assignment version, adapter version, calibration state, pose-covariance state and quality. Nullable text is exactly source version, its unknown reason and the RSSI/noise unknown reasons. The existing mutually exclusive known/unknown checks still enforce the evidence pairs. The nullable set matches the Arrow schema.

`Store.append` calls `batch_validate` before a transaction, registration, serialization or chunk creation. The correction therefore yields a uniform `ValueError` before any row in the supplied batch can become visible. No fallback value or invented provenance is introduced. The persisted regression places a valid row before the invalid row and asserts that neither is added, preventing a weak test that only checks an exception after partial work.

## Independent evidence

Executed on the actual optional SQLite/PyArrow/DuckDB environment, with no skipped engine tests:

```sh
research/storage/.venv/bin/python -m unittest discover -s tests -p test_storage_research.py -v
research/storage/.venv/bin/python -m ruff check research/storage/model.py tests/test_storage_research.py
research/storage/.venv/bin/python -m ruff format --check research/storage/model.py tests/test_storage_research.py
```

All 15 tests passed in 1.068 seconds; Ruff passed and both files were already formatted. The suite includes whole-batch rejection, reopen, Parquet roundtrip, source unknowns, extreme timestamps, migration rollback, injected failures, corruption and abrupt process-exit recovery. Those broader checks provide regression evidence; they are not a separate acceptance of all storage design claims.

Independent support script `.tools/storage-null-probe.py` in the review worktree used the storage worktree's interpreter:

```sh
../storage-proof/research/storage/.venv/bin/python .tools/storage-null-probe.py
```

The script passed 90 cases: all five required text fields × `None`, empty string, boolean, integer, 130-byte UTF-8 string and bytes × all three engines. Each attempt uses `[valid_new_row, invalid_row]` after a committed baseline row. Every invalid batch raised `ValueError`, left the complete row set unchanged and created no new artifact pathname. Each engine then accepted a valid append and preserved the exact rows through close/reopen. Explicit optional known/unknown source provenance still validated.

An in-memory mutation removed only the newly added null guard from the inspected source, without editing the implementation. Both `assignment_version=None` and `adapter_version=None` then validated successfully, reproducing the defect the new tests guard against. This is a mutation of the corrected source, not a claim to execute an independently preserved earlier Git revision. Temporary databases and probe artifacts were retained in accordance with the deletion policy.

## Exact reviewed snapshot

The model and tests were uncommitted in the isolated storage-proof worktree. Supporting engine/schema source was inspected to establish the rejection boundary; its full implementation is outside this correction approval.

| File | SHA-256 |
|---|---|
| `research/storage/model.py` | `9e7152089ec72c0fd98ced9ce90da3fe9eb37938e9814ab51160caa129e7f7d2` |
| `tests/test_storage_research.py` | `b915112100a0e0362ef02166ad489c67adcc13fd3d1c0a0298b2377ca1bba6ea` |
| `research/storage/engines.py` | `e19c72fc1fae92710fb0c7f979da207e390834d71a9865e3e502bef015963359` |
| `research/storage/requirements.lock` | `59175e90afe0f04915fd44d8573c9dc739182a88dcaaf2171da74b0ee2431824` |

## Ten-field handoff

1. **Scope completed:** independent review and verification of required text/provenance null rejection and cross-engine batch atomicity for STO-001.
2. **Files changed:** only `docs/reviews/storage-provenance-correction-review.md`; ignored independent probe retained locally. No storage implementation was edited by this reviewer.
3. **Architecture decisions:** none added. Confirmed validation remains before all storage side effects and respects explicit unknown pairs.
4. **Tests added:** independent 90-case null/type/UTF-8-length probe, Arrow-nullability comparison, reopen checks and guard-removal mutation, reproduced below.
5. **Tests executed/results:** all 15 existing/corrective tests, Ruff lint/format and all independent probes passed on the real three-engine environment.
6. **Known limitations:** this is a trusted local research projection; it does not prove production hostile-file isolation, schema migrations for previously stored invalid data, or all future field additions.
7. **Requirements advanced:** uniform schema/provenance validation and storage experiment parity/reliability supporting Gate C and FND storage evidence.
8. **Requirements still open:** broader experiment/ADR acceptance belongs to the separate storage review; production transactional observation storage, complete provenance schemas, platform parity and import isolation remain independent requirements.
9. **Risks/follow-up:** keep Arrow and validator nullability synchronized when adding fields. Existing externally modified research databases are not repaired by the admission guard; production read/migration validation must address that separately. Root must regenerate benchmark provenance hashes after source correction.
10. **Suggested commit:** `docs(review): verify required storage provenance validation`.

## Independent probe

```python
from pathlib import Path
from dataclasses import replace
import tempfile, sys

sys.path.insert(
    0, str(Path(__file__).resolve().parents[2] / "storage-proof/research/storage")
)
from model import generate, TEXT_FIELDS, batch_validate
from engines import Store, arrow_schema

nullable = {"source_version", "source_version_unknown", "rssi_unknown", "noise_unknown"}
required = set(TEXT_FIELDS) - nullable
assert required == {
    "assignment_version",
    "adapter_version",
    "calibration_state",
    "pose_covariance_state",
    "quality",
}
assert {
    f.name for f in arrow_schema() if f.name in TEXT_FIELDS and f.nullable
} == nullable
rows = next(generate(5))
base = Path(tempfile.mkdtemp(prefix="kyberia-storage-independent-null-"))
count = 0
for engine in ("sqlite", "parquet", "duckdb"):
    path = base / engine
    store = Store(path, engine, create=True)
    store.append([rows[0]])
    for field in sorted(required):
        for value in (None, "", False, 0, "é" * 65, b"version/1"):
            bad = replace(rows[2], **{field: value})
            try:
                batch_validate([bad])
            except ValueError:
                pass
            else:
                raise AssertionError((field, value, "validator accepted"))
            before = set(path.iterdir())
            try:
                store.append([rows[1], bad])
            except ValueError:
                pass
            else:
                raise AssertionError((engine, field, value, "store accepted"))
            assert list(store.rows()) == [rows[0]]
            assert set(path.iterdir()) == before, (engine, field, "new artifact")
            count += 1
    store.append(rows[1:])
    store.close()
    store = Store(path, engine)
    assert list(store.rows()) == rows
    store.close()
# Optional known/unknown provenance remains representable, including nullable reason.
for source, reason in ((None, "not_reported"), ("source/é", None)):
    row = replace(rows[1], source_version=source, source_version_unknown=reason)
    batch_validate([row])
print(
    f"PASS: {count} invalid required-text batches rejected without rows/artifacts on all three engines; nullable contracts and reopen passed; retained {base}"
)

# Mutation check: delete only the newly added required-text null guard in memory.
import types

source = Path(sys.path[0], "model.py").read_text()
start = source.index("            if value is None and name not in (")
end = source.index("            if value is not None and (", start)
mutant = types.ModuleType("review_nullable_mutant")
sys.modules[mutant.__name__] = mutant
exec(compile(source[:start] + source[end:], "<review mutant>", "exec"), mutant.__dict__)
row = next(mutant.generate(1))[0]
for field in ("assignment_version", "adapter_version"):
    replace(row, **{field: None}).validate()
print(
    "PASS: removing only the new guard reproduces acceptance of both required provenance nulls"
)
```
