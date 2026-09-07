# Gate C analytical storage proof

Status: independently reviewed research proof after correcting required provenance validation; Gate C remains open. This is research code, not a replacement for `crates/project-store` or a product observation store.

Plan references: §3 raw evidence/unknowns/reproducibility, §7.1–7.3 evidence planes and clocks, §10.2/10.7 storage recommendation, §11.1/11.5/11.9/11.11, §16.14–16.16, Phase 0, §20 Gate C, Appendix I canonical boundary and open exports. Backlog FND-006 remains incomplete until the reviewed production writer/reader and application integration exist.

## Question and candidates

The experiment compares identical observation projections in three real engines:

| Candidate | Live batch commit | Query path | Durable metadata |
|---|---|---|---|
| SQLite-only | Prepared inserts in a FULL-synchronous DELETE-journal transaction | SQL with UTC and `(floor, x, y)` B-tree indexes | SQLite |
| SQLite + Parquet | Zstd Parquet chunk written and fsynced; content address hard-linked and directory fsynced; SQLite commits chunk reference and observation-ID uniqueness index together | PyArrow Dataset predicate/projection scan, bounded Python reduction for four-floor count/min/max | SQLite |
| Embedded DuckDB | Arrow batch registered and inserted in a transaction; same logical time/spatial indexes | Embedded SQL, one thread, 512 MB engine memory setting | DuckDB research database |

The Parquet candidate retains one SQLite identity row per observation to reject duplicates transactionally. This overhead is included in disk and ingestion measurements. Arrow conversion, validation, indexes, fsync and metadata commit are included in batch write time. This deliberately measures the adapters as implemented; it is not a universal ranking of engine internals. In particular, native SQL aggregation and the Parquet adapter's bounded Python reduction have different CPU overhead. An optimized Rust/Arrow reduction needs its own measurements before projection results become a product performance promise.

SQLite remains the existing project's authority. DuckDB runs only in an isolated research interpreter, with extension auto-install/loading and external access disabled. No third-party objects enter canonical Rust types and no production dependency is added.

## Original input and semantic contract

`research/storage/model.py` generates original synthetic records using LCG constants 1664525 and 1013904223 modulo 2³², explicit seed 42, ordered nonnil 16-byte observation identities, 8 sources/sensors/adapters/clock epochs, four floor/frame pairs, and three frequencies. Each next observation advances UTC and source-local monotonic time by 25,000,001 ns. A million records represent about 6.94 hours of timestamped evidence replayed at maximum speed; the experiment is not a seven-hour live-radio soak.

The 26-column projection includes observation/session/source/sensor/adapter/epoch/floor/frame identity, UTC nanoseconds, monotonic nanoseconds, assigned x/y/z meters, assignment version, frequency Hz, raw RSSI/noise dBm with separate unknown reasons, producer version with unknown reason, adapter version, calibration/pose-covariance state, original-source hash and synthetic quality label. Every seventh RSSI, third noise and fifth producer version is unknown. Reasons survive as strings rather than numerical zero. Negative positions, Unicode source versions, fractional dBm and signed 64-bit nanosecond extremes have separate edge tests. The signed monotonic projection is a deliberate subset of the domain's unsigned range; values beyond 2⁶³−1 fail instead of narrowing silently.

Position is an assignment projection linked to a version, not a rewrite of raw capture. Calibration is explicitly uncalibrated and covariance not measured. Raw hash reuse every 8192 records represents repeated independent observations of the same original generated source text; only repeated observation IDs are duplicate identities. There are no real MACs, SSIDs, vendor datasets or competitor fixtures.

This projection does not implement full `ObservationEnvelope` serialization: channel dwell, clock covariance, privacy details, frame/scan payloads, optional pose and the complete source descriptor need the production adapter. It supports the storage-choice question without inventing absent evidence. Synthetic coordinate/signal distributions and repeated metadata can compress differently from real captures; field workloads remain necessary.

The strict local projection validator rejects nonfinite values, invalid identities, wrong types, missing value/reason pairs, contradictory known/unknown pairs, unsupported reason/provenance states, oversized text and duplicate IDs within a batch. Both transactional engines enforce primary keys; the Parquet metadata transaction enforces identity uniqueness. A mixed batch containing a new ID and a previously committed ID rolls back entirely rather than silently skipping either row.

## Workload and measurement protocol

Each candidate and row count uses a fresh subprocess and newly reserved directory. The default sizes are 10,000, 100,000 and 1,000,000 rows, with batches of at most 4096. Input generation and expected-digest preparation occur outside write timing. The runner records wall and process CPU seconds, batch p95/max latency, ingestion rate, create/close/reopen, full typed verification, portable export/import, additive metadata migration and peak process RSS. Storage bytes sum unique file inodes, counting retained hard-linked temporary names once, and exclude the separately reported portable export. File hashes/sizes are recorded individually.

Every row is reconstructed and validated after reopen and after migration, and the complete sequence hash must match the generated sequence. The ordered fixture makes append order and observation-ID order identical; this is a sequence checksum for this workload, not a general order-independent dataset hash. Export writes real Parquet 2.6 with explicit Arrow field types and metadata, then imports and compares every value. Repeated exports of the edge fixture produce identical file hashes.

Queries return per-floor total count, known RSSI count, minimum and maximum known dBm. They never infer signal from an empty/unknown group. Three trials cover a 1%-duration time window, a fixed floor/rectangle and a full report scan. Independent Python-oracle tests validate all predicates and grouping. Cross-engine query results and row digests must match before an aggregate evidence report is written.

OS caches are not flushed; queries run after full verification and are therefore warm. No cold-open, uncontended-system or power-loss claim is made. Measurements run inside the workspace sandbox. PyArrow may report that optional host cache-size sysctls are unavailable there; this does not invalidate semantic tests, but host capability/performance retesting outside the sandbox is part of a production performance gate. Process RSS includes Python/native libraries and export verification; DuckDB's 512 MB setting is not a whole-process memory cap. The parent enforces a 900-second limit per case, retaining interrupted artifacts for inspection.

## Failure and migration experiments

Tests inject cancellation and I/O failure before commit and, for Parquet, after writing and after publishing a complete content-addressed chunk. Previously committed evidence remains readable, and retrying the next batch succeeds. A separate child process exits abruptly during the transaction using `os._exit(79)`, bypassing Python unwinding; reopen recovers only the committed prefix on every candidate. These tests exercise real transaction rollback/recovery, not simulated database responses.

The Parquet publication order ensures that a committed chunk reference points to a complete, synced file. A crash can leave pending or published-but-unreferenced files. Queries enumerate committed metadata only. Temporary files and hard links are retained under ignored directories per deletion policy. This is not a garbage collector or approved production cleanup design.

The additive metadata migration creates an annotation table and advances research format 1 to 2 transactionally. Injection between DDL and version update rolls back; reopening version 2 preserves all rows and existing Parquet hashes. This does not implement project migration history, backups, every historical production schema, read-only handling of unknown required features or encrypted migration.

Read tests reject missing/corrupted chunk bytes, mismatched row counts, traversal-like metadata names, unknown format versions and unexpected Parquet schemas. Parquet import checks the file hash, a 64 MiB file limit, schema metadata, row limit and bounded batches; native decoder Thrift string/container limits are supplied. This is a trusted-local experiment and is not an adversarial native-decoder sandbox: compressed allocation bombs, malicious concurrent filesystem replacement, disk-full hardware behavior, low-memory termination and hostile database schemas still require production security/fuzz testing.

## Reproduce

The checked lock contains tested CPython 3.12 macOS ARM64 wheel hashes only. Extend the lock with independently verified platform wheels before running another platform. A Python 3.12.12 interpreter must already be installed; the proof used a separate venv created from the pinned development interpreter available to the Sionna proof, without importing Sionna.

```sh
python3.12 -m venv research/storage/.venv
research/storage/.venv/bin/python -m pip install --require-hashes --only-binary=:all: --no-cache-dir -r research/storage/requirements.lock
research/storage/.venv/bin/python -m ruff format --check research/storage/*.py tests/test_storage_research.py
research/storage/.venv/bin/python -m ruff check research/storage/*.py tests/test_storage_research.py
research/storage/.venv/bin/python -m unittest discover -s tests -p test_storage_research.py -v
research/storage/.venv/bin/python research/storage/benchmark.py --output research/storage/evidence/new-run.json --data-root research/storage/.venv/new-run-data
```

Use fresh output/data paths for every run; existing evidence is never overwritten. Research Python sources are hashed before execution and checked unchanged before publication. Root Python 3.9 can run the two dependency-free model tests; thirteen real-engine tests explicitly skip if optional engines are absent. Those skips are not Gate C validation.

The [source inventory](../licenses/storage-research-sources.json) records pins, tested wheel hashes, native engine/compiler identifiers, license/notice hashes, interpreter provenance and update procedure. These are research dependencies, not a distributable SBOM. Rust's existing project-store uses SQLite 3.53.2, whereas this Python build supplies SQLite 3.50.4; port-level confirmation remains necessary.

## Evidence and remaining gate

The machine-readable [macOS report](../../research/storage/evidence/macos-arm64.json) contains all input settings, nine cases, full row/query parity, individual artifact checksums and source hashes. The frozen run completed on September 7, 2026, from 06:41:41 through 06:52:58 UTC. All nine cases passed complete row, query, export/import and migration parity; the recorded research source hashes match the checked-in source.

| Rows | Engine | Batch writes (s) | Time query (ms) | Spatial query (ms) | Report scan (ms) | Stored logical MB | Peak process MB |
|---:|---|---:|---:|---:|---:|---:|---:|
| 10,000 | SQLite | 0.158 | 0.306 | 0.079 | 12.134 | 4.375 | 88.162 |
| 10,000 | SQLite + Parquet | 0.250 | 2.732 | 6.475 | 7.295 | 1.183 | 106.496 |
| 10,000 | DuckDB | 0.290 | 0.669 | 0.695 | 0.350 | 2.896 | 152.142 |
| 100,000 | SQLite | 1.839 | 0.295 | 1.894 | 122.229 | 43.778 | 98.075 |
| 100,000 | SQLite + Parquet | 1.426 | 6.475 | 15.475 | 55.445 | 11.787 | 118.276 |
| 100,000 | DuckDB | 1.466 | 0.562 | 1.436 | 1.494 | 30.945 | 266.158 |
| 1,000,000 | SQLite | 35.518 | 3.822 | 30.773 | 1686.096 | 438.239 | 164.561 |
| 1,000,000 | SQLite + Parquet | 14.204 | 63.479 | 128.970 | 543.380 | 119.697 | 197.427 |
| 1,000,000 | DuckDB | 16.746 | 0.847 | 9.196 | 10.985 | 232.534 | 751.387 |

Query columns show the second of three warm trials; all trials and CPU times remain in JSON. MB means decimal 1,000,000 bytes. Batch p95 is the lower empirical order statistic at `floor((batch_count−1)×0.95)`, not an interpolated percentile. Reopen timings measure database connection/format validation only; full evidence hashing is a separately timed operation. The tiny additive metadata migration takes about 1.1–1.4 ms here and says nothing about a costly future schema transformation.

At one million rows the Parquet split uses about 27% of SQLite-only's logical bytes and has faster batch writes in this implementation. SQLite indexed selective reads remain substantially faster than the Parquet scanner. DuckDB is strongest on report scans but reaches 751 MB whole-process peak RSS despite the 512 MB engine setting, underscoring that the setting is not a process limit. That footprint includes indexes, native/Python allocations and export/verification, so it should not be reported as query-only engine memory.

These results support continuing the planned SQLite authority plus immutable Parquet work, preserving an indexed live/selective query path and leaving DuckDB optional pending production memory/deployment tests. The [proposed ADR](../architecture/ADR/0004-storage-split.md) does not claim Gate C passed.

Validation executed: the documented Ruff formatting check and lint passed; Python compilation passed; `research/storage/.venv/bin/python -m unittest discover -s tests -p test_storage_research.py -v` passed all **15 tests in 1.063 seconds**, including real abrupt-exit recovery. The complete default benchmark command passed all nine cases. Python 3.9 dependency-free test discovery remains usable, with optional engine execution explicitly skipped when absent.

Remaining Gate C work includes production Rust columnar integration, complete envelope roundtrip, longer live capture/backpressure, crash/disk-full behavior under production publication, migration/backups for the actual project schema, compressed hostile imports, file-lock/move behavior on Windows and second-platform portability/performance execution. None of those are marked externally blocked by this proof merely because they are unfinished.

Independent review found missing adapter/assignment versions were accepted by the common validator and SQLite/DuckDB but rejected by Parquet. The validator now enforces the same required text fields on every engine, with whole-batch rejection before writes. Fifteen tests, a separate 90-case correction review, 30 independent permutation/boundary/export checks and all nine benchmark cases pass on the corrected source. The [full review](../reviews/storage-proof-review.md) and [correction review](../reviews/storage-provenance-correction-review.md) document scope and hashes. These research results do not validate the production storage gate.
