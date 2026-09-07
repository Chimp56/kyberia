# Independent KismetDB packet-metadata review

Reviewer: `/root/qa_spec_audit`. Author: `/root`.
Decision: **APPROVED for the initial bounded KismetDB reader**.
The formal MAJOR generated-row-identity finding and preparatory design concerns
were corrected before approval. No unresolved BLOCKER or MAJOR finding remains.

## 1. Scope completed

Read-only review of the database boundary, schema versions, packet metadata,
row identity, cancellation/resource limits, private snapshot hashing, payload
retention semantics, dependency policy and source inventory. Relevant plan:
§10.12, §§11/15/16, Gate H and Appendix I Kismet runtime requirements.
Review approval is limited to decoding; it does not complete the Kismet gate.

## 2. Files reviewed and changed

Primary HEAD at hash capture was `2d69333a7ef664843cb1e526292d76d395842050`.
The following hashes identify actual working-tree content. New code is not
attributed to that baseline commit. Only this report was written by the reviewer.

| File | SHA-256 |
|---|---|
| `crates/kismet-adapter/Cargo.toml` | `35994416c9d16ea7a79273d52239b68d4d5733a957265010be9ec90eda913ce5` |
| `crates/kismet-adapter/src/database.rs` | `743bc5336340a70b34cbd6f677c17a18ab717be83ed90df251ada395ba18348a` |
| `crates/kismet-adapter/src/lib.rs` | `5594c6429e9735128d1dc8cd1e273bd80637796deaca8c408da536714f84c4f5` |
| `crates/kismet-adapter/tests/database.rs` | `78937c63c97b3029f1a8aeff8aacea2ba1f42a2806bfc5098aba690b8ce577a5` |
| `docs/adapters/kismet.md` | `d44ebee4ace9faa70ce797740b70a5b1fbcdcc8ac141b3263513dff7a20468f7` |
| `tools/architecture.json` | `e4f4792e60561cf11f3af59e6e10a40a580b0283378a3448ebce625a4742a3ee` |
| `Cargo.lock` | `edd3e7d1f04aa069380d06b8f0d9e4852d27eb0570621b5a4ac05a4b51ccc68b` |
| `docs/licenses/cargo-sources.json` | `7aa8b81eeebbbaaf371bc253a852982c83555d6a9e92e0bdf59169a1b64bc879` |

## 3. Architecture and scientific assessment

The adapter is outward and reuses pinned SQLite, hashing and temporary-file
libraries. No Kismet implementation was copied or linked. Returned packet
records are foreign evidence; device aggregates are never promoted to samples.
Provenance fields and the SQLite connection are immutable through the public API.

The official schema specifies per-packet microseconds, kHz frequency, Mbps rate
and PHY-specific signal. Version 7 adds rate, version 8 correlation fields and
version 9 original packet length. The implementation follows explicit `ts_usec`
field semantics despite the introductory prose's conflicting milliseconds word.
Signal remains a raw integer, and packet correlation IDs do not erase distinct
receptions. These choices match the [official KismetDB definitions](https://kismetwireless.net/docs/dev/kismetdb/).

Packet stripping can retain metadata while removing payloads. The reader now
represents NULL/empty removed bytes separately instead of declaring preserved
metadata corrupt, consistent with the [documented stripping workflow](https://kismetwireless.net/docs/readme/kismetdb/kismetdb_strip_packets/).
No authorized live-radio accuracy claim follows from those schema references.

## 4. Findings and corrections

**MAJOR KD-005 — Resolved.** `PRAGMA table_info` omits generated columns. An
independent test added `_rowid_ INTEGER GENERATED ALWAYS AS (1) VIRTUAL` to a
three-packet database. The reader opened it, returned two rows with identity 1,
then returned an empty second batch marked complete: one packet was silently
lost. The correction uses `table_xinfo` and rejects shadowing names across hidden
and generated columns. The original independent attack now fails admission.
Author regressions cover `_rowid_`, mixed-case `RoWiD`, `oid`, virtual generation
and a stored generated identity in the version table.

Preparatory review also resulted in four concrete corrections:

- Private getters replace mutable source/hash/schema fields.
- A private temporary copy is hashed as it is written and supplies every SQLite
  query. Original-path replacement cannot substitute different bytes after hashing.
- Direct rowid range predicates replace an optional-parameter OR predicate.
  Independent EXPLAIN QUERY PLAN comparison showed the old form scanning the
  table and the direct form seeking through the integer primary key.
- Missing payloads remain explicit `NotRetained` while packet metadata survives.

These changes were reviewed before final tests; they are not unresolved debt.

## 5. Independent tests executed

```text
cargo test -p kyberia-kismet-adapter --locked --offline
PASS: 12 behavioral tests; explicit benchmark intentionally ignored

cargo test -p kyberia-kismet-adapter --release --locked --offline metadata_batch_benchmark -- --ignored --nocapture
PASS: 10,000 rows / batch 512: snapshot-open 3.263 ms, total 8.936 ms
PASS: 100,000 rows / batch 512: snapshot-open 28.662 ms, total 79.211 ms

cargo clippy -p kyberia-kismet-adapter --all-targets --offline --locked -- -D warnings
PASS

cargo fmt --all -- --check
PASS

.tools/venv/bin/python tools/architecture.py
PASS

.tools/venv/bin/python tools/source_inventory.py check
PASS: 86 locked external packages

Separate rustc-compiled review probes
PASS: generated row identity cannot silently lose packets
PASS: corrupting the original path leaves the hashed private snapshot readable
```

Existing tests cover all six schema versions, tied packet timestamps/correlation
IDs, negative/extreme rowids, invalid units/types, absent fields, source identity,
nonordinary schema, corrupt headers, bounds, cancellation, deadline, stripped
payloads and exact snapshot hashing. Benchmarks include copy/hash time and verify
row counts. Their timings are local macOS ARM64 baselines, not universal limits.
Temporary test inputs were retained; no recursive deletion was performed.

## 6. Known limitations

The source must be closed and checkpointed. Hashing the copied bytes establishes
their identity, not that concurrently modified input ever formed a coherent
capture. Same-user modification of private temporary files and hostile importer
process confinement are outside this in-process boundary. Kernel filesystem calls
can block; deadline and cancellation are cooperative, not hard process deadlines.

The temporary snapshot contains original raw capture content even though packet
records omit it. Single-file unlink on normal drop is not secure erasure and
does not address crash leftovers. Privacy-aware retention and recovery need
application integration before a complete user-facing importer exists.

## 7. Requirements supported

This increment supports version-aware historical packet metadata extraction,
bounded pagination, source/hash provenance, explicit missing metadata, raw signal
honesty, cancellation and malformed-database rejection. Read-only source access
and private snapshot isolation preserve the external integration boundary.

## 8. Requirements still open

Canonical observation normalization, exact source artifact persistence,
transactional import publication/idempotency, authenticated API/event streams,
remote acquisition/reconnect/drop semantics, PCAPNG decoding/parity, real Kismet
producer-version fixtures and Linux hardware runtime gates remain independent
work. Database schema versions are not Kismet executable versions. No Gate H,
complete Appendix I Kismet gate or end-user survey completion claim is made.

## 9. Follow-up risks and acceptance

Callers must begin at the intended cursor, stage every batch, retain its provenance
and publish only after the entire requested sequence and finish succeed. The
reader's complete flag describes the tail after the supplied cursor, not proof
that a caller imported earlier rows. A storage transaction/duplicate registry is
not supplied here. Reopening exact retained bytes plus rowid gives a stable
idempotency key; packet hash alone does not.

Next validate actual Kismet-produced artifacts against controlled radio captures
and API/PCAPNG output from pinned releases. Preserve unresolved clock alignment,
dwell, calibration and PHY signal meaning during normalization. Unknown radio
context must remain unknown rather than becoming a plausible-looking heatmap.

## 10. Suggested commit

`feat(kismet): add bounded private-snapshot database metadata reader`

Review artifact: `docs(review): approve corrected KismetDB reader boundary`.
