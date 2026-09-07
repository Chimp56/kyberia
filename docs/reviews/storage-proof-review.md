# Independent analytical storage research review

Disposition: **APPROVED for the scoped Phase 0 research comparison**, with MAJOR STO-001 corrected and independently verified. This is not acceptance of aggregate Gate C, a production Parquet store or release dependency clearance.

Reviewer: `/root`, independent of implementation author `/root/storage_data`. Root authored the small provenance correction after reproducing the defect; `/root/rf_numerical_review` independently reviewed that correction in `b83174a65c1bed830900e32648a652099cd59ab4`. The correction author is not its sole reviewer.

## Findings

- **MAJOR STO-001 — resolved:** the common row validator accepted `None` for required adapter and position-assignment versions. Real SQLite and DuckDB stored those missing values while Parquet failed in its native writer. The root reviewer reproduced both fields across all three engines, added failing regressions, then added required-text validation before any transaction. Fifteen tests now pass; the [separate correction review](storage-provenance-correction-review.md) confirms 90 malformed-batch probes and unchanged committed rows/artifacts after rejection.
- No unresolved BLOCKER or MAJOR finding remains in the bounded research scope.

## Scope and architecture

All three actual engines use the same explicit 26-column synthetic query projection and preserve identifiers, exact signed nanoseconds, positional units, known/unknown signal and provenance reasons. The projection is deliberately a subset of canonical observations. No Python, Arrow or DuckDB type enters the Rust domain or the production project store. The proposed ADR retains SQLite authority and leaves the production split decision gated on Rust integration and deployment evidence.

Review inspected the complete model, adapters, benchmark runner and tests, including batch validation before transactions, duplicate rollback, Parquet file publication before metadata commit, retained orphan handling, bounded native import, additive migration rollback and abrupt-process-exit recovery. Imported native files remain explicitly trusted local research inputs; the code does not claim a hostile-file process sandbox.

## Validation performed independently

- `research/storage/.venv/bin/python -m unittest discover -s tests -p test_storage_research.py -v`: initial 14 tests passed. New null-version regressions then failed as expected. Corrected suite: 15 passed in 1.063 seconds.
- Ruff lint and formatting checks passed on all research Python and tests.
- The complete nine-case benchmark was rerun after the model correction: SQLite, SQLite + Parquet and DuckDB at 10k, 100k and 1m rows. All exact row, query, export/import and migration comparisons passed. The final report hashes match the corrected source.
- Thirty independent checks used 5003 records, generator seed 271828 and shuffle seed 314159, with 127-row batches in shuffled order. All engines passed order-normalized exact row parity, empty storage, signed timestamp extremes, empty/reversed time windows, rectangular predicates, known zero/extreme finite dBm, Parquet export/import and reopen parity. Artifacts are retained under ignored `.tools/storage-independent-czt40byc`; the standalone probe is retained below for reproduction.

At one million rows the final run measured 438.239 MB/35.518 s writes for SQLite, 119.697 MB/14.204 s for Parquet + SQLite and 232.534 MB/16.746 s for DuckDB. Second warm report trials were 1686.096, 543.380 and 10.985 ms respectively. Peak process RSS was 164.561, 197.427 and 751.387 MB. These are whole adapter/interpreter measurements on one shared host; warm caches, native SQL versus Python reduction, synthetic compressibility and whole-process memory are disclosed. The DuckDB 512 MB engine setting is not a process quota.

## Acceptance and remaining requirements

The actual storage comparison and original-fixture methodology support plan §11, §16 reliability/performance, Phase 0 and §20 Gate C. FND-006 remains in progress. Full-envelope production integration, sustained live capture/backpressure, actual project migration/backups, hostile native-import isolation/fuzzing, disk-full/low-memory tests and second-platform execution remain open. The sequence digest in the benchmark assumes ordered fixture ingestion; arbitrary datasets require an explicitly canonical ordering, as the independent shuffled probe does. This is not a dataset hash specification.

## Handoff

1. Scope completed: independent review and rerun of the three-engine research proof.
2. Files changed: this review; root's two-file STO-001 correction, benchmark evidence and corresponding documentation updates are covered above.
3. Decisions: no new accepted storage ADR; proposed SQLite authority/Parquet direction remains evidence-gated.
4. Tests added: required null-version and whole-batch engine regression; separate independent shuffled/boundary probe.
5. Tests executed/results: 15 suite tests, 30 root probes, nine full cases, lint/format all pass; correction reviewer separately passed 90 malformed batches.
6. Limitations: trusted-local synthetic Python research projection on macOS, not production Rust or field evidence.
7. Requirements supported: Gate C comparison, semantic parity, recovery/export/migration methodology.
8. Open requirements: full production storage and remaining Gate C matrix.
9. Risks/follow-up: resource-isolated native imports, live spool/finalization design, cross-platform durability, large-project query memory and distributable notices.
10. Suggested implementation commit: `research(storage): compare transactional and columnar observation stores`.

## Frozen artifacts

| Path | SHA-256 |
|---|---|
| `research/storage/model.py` | `9e7152089ec72c0fd98ced9ce90da3fe9eb37938e9814ab51160caa129e7f7d2` |
| `research/storage/engines.py` | `e19c72fc1fae92710fb0c7f979da207e390834d71a9865e3e502bef015963359` |
| `research/storage/benchmark.py` | `6167129ddcc335e018313eff98267d21492cd9df97a15fe17f312e5180e711c4` |
| `research/storage/requirements.lock` | `59175e90afe0f04915fd44d8573c9dc739182a88dcaaf2171da74b0ee2431824` |
| `research/storage/evidence/macos-arm64.json` | `fb5e5b764996221e8ea441e9abd6c0d1d725fb8cdb791f6bf4ada66d60a1fa24` |
| `tests/test_storage_research.py` | `b915112100a0e0362ef02166ad489c67adcc13fd3d1c0a0298b2377ca1bba6ea` |
| `docs/validation/storage-proof.md` | `265ce56f9852e4aa14a0b1f656d0e02ccb1321101af424cb5cdd11e6faf4418f` |
| `docs/architecture/ADR/0004-storage-split.md` | `20156a781124781a1f9befbf3a1258c4a9f9526d2576ac2797add024fb37e1e9` |
| `docs/licenses/storage-research-sources.json` | `28a4c0bd9060525e3e0e816a098658c176e0ff808a334972042b9f637c3aa849` |

## Independent probe

```python
from pathlib import Path
from dataclasses import replace
import sys, tempfile, random, json
root=Path('/Users/vincent/code/kyberia/.worktrees/storage-proof')
sys.path.insert(0,str(root/'research/storage'))
from model import generate,digest_rows
from engines import Store,export_parquet,import_parquet
rows=[r for b in generate(5003,seed=271828) for r in b]
rows[1]=replace(rows[1],utc_ns=-(2**63),rssi_dbm=0.0)
rows[2]=replace(rows[2],utc_ns=2**63-1,rssi_dbm=-1e200)
random.Random(314159).shuffle(rows)
ordered=sorted(rows,key=lambda r:r.observation_id)
def oracle(selected):
    by={}
    for r in selected: by.setdefault(r.floor_id,[]).append(r.rssi_dbm)
    return [(key,len(values),sum(v is not None for v in values),min((v for v in values if v is not None),default=None),max((v for v in values if v is not None),default=None)) for key,values in sorted(by.items())]
base=Path(tempfile.mkdtemp(prefix='storage-independent-',dir='/Users/vincent/code/kyberia/.tools'))
checks=0
for engine in ('sqlite','parquet','duckdb'):
    store=Store(base/engine,engine,True)
    assert store.aggregate()==[];checks+=1
    for i in range(0,len(rows),127): store.append(rows[i:i+127])
    recovered=sorted(store.rows(),key=lambda r:r.observation_id)
    assert digest_rows(recovered)==digest_rows(ordered);checks+=1
    assert store.aggregate()==oracle(rows);checks+=1
    for start,end in [(-2**63,-2**63+1),(2**63-2,2**63-1),(42,42),(900,100)]:
        assert store.aggregate('time',start,end)==oracle([r for r in rows if start<=r.utc_ns<end]);checks+=1
    assert store.aggregate('spatial')==oracle([r for r in rows if r.floor_id==(601).to_bytes(16,'big') and 20<=r.x_m<40 and 30<=r.y_m<60]);checks+=1
    path=base/(engine+'.parquet');checksum=export_parquet(store.rows(),path)
    assert digest_rows(sorted(import_parquet(path,checksum,maximum_rows=5003),key=lambda r:r.observation_id))==digest_rows(ordered);checks+=1
    store.close()
    reopened=Store(base/engine,engine)
    assert digest_rows(sorted(reopened.rows(),key=lambda r:r.observation_id))==digest_rows(ordered);checks+=1
    reopened.close()
print(json.dumps({'checks':checks,'result':'PASS','rows':len(rows),'seed':271828,'permutation_seed':314159,'engines':3,'retained_artifacts':str(base)}))
```
