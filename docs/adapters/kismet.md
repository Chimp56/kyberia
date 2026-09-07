# Kismet adapter

Kismet remains an external GPL integration. Kyberia owns canonical observations and Wi-Fi semantics. This initial increment implements a read-only KismetDB packet-metadata reader in `crates/kismet-adapter`; it does not yet normalize canonical envelopes, decode PCAPNG, consume the authenticated API, or validate the full Kismet runtime gate.

## Database evidence boundary

The reader accepts closed KismetDB schema versions 5 through 10, with exactly one version row, ordinary rowid tables and unique datasource UUIDs. It rejects unsupported versions, views, virtual tables, shadowed row identities (including generated/hidden columns inspected with `table_xinfo`), malformed typed values, unresolved datasource references and active WAL/journal files. SQLite is the existing pinned rusqlite/bundled SQLite implementation; no Kismet GPL implementation has been copied or linked.

`KismetDb::open(path, budget)` streams the input into a private temporary file while computing SHA-256. All queries read that copy with a read-only SQLite connection and transaction. Source-path replacement after opening cannot alter the queried evidence or its hash. Provenance fields and the connection are private and exposed only through read-only getters. This binds results to the copied bytes; it does not certify that an actively changing input ever represented a complete capture. Close capture and checkpoint it before import. A malicious same-user process capable of modifying Kyberia's private temporary files is outside this in-process boundary; dedicated importer process isolation remains open.

The temporary copy may contain raw private captures even though query results exclude them. The owning single-file guard unlinks that one file when the reader closes, including on ordinary error unwinding; it performs no recursive cleanup. Crash leftovers and forensic secure erasure are not addressed by unlinking. Application privacy controls, encrypted projects and crash-recovery policy remain necessary before user-facing ingestion.

`read_batch(after, limit)` uses stable row order and a direct rowid range predicate. Limit is 1–4096 records, source count at most 1024, metadata text at most 256 UTF-8 bytes and input size at most 8 GiB. SQLite column/row values are limited to 1 MiB, SQL to 16 KiB, columns to 128, expression depth to 32 and compiled instructions to 100,000. A cumulative 20-million SQLite VM operation ceiling and explicit 1–120-second positive deadline bound work; cancellation is checked while copying and reading and every 1000 SQLite instructions. Filesystem calls can block in the kernel; these are cooperative bounds, not a process-kill deadline. Authorizer rules permit only selected reads and necessary metadata/length functions; extensions, writes and arbitrary SQL are not exposed.

`Batch.complete` describes the requested tail of the database. Callers must retain the first cursor, accumulate each batch, and commit results transactionally only after the complete sequence and `finish()` succeed. Repeating a cursor deliberately repeats the same rows; `(database SHA-256, rowid)` is the ingestion idempotency key. Packet IDs and CRCs are retained as correlation evidence and never used alone to drop matching receptions. No storage transaction or duplicate-import registry is implemented in this reader itself.

## Measurement interpretation

The [official database field definitions](https://kismetwireless.net/docs/dev/kismetdb/) specify microsecond `ts_usec`, kHz frequencies and Mbps PHY rates. The introductory timestamp prose says milliseconds despite referring to `tv_usec`; this adapter follows the explicit per-packet microsecond field definition. Signed seconds plus microseconds convert with checked arithmetic into source-reported UTC nanoseconds. Clock origin, synchronization, receipt time, dwell, position covariance and calibration are not invented. Frequency zero and absent/zero PHY rate remain unknown.

Signal remains a **PHY-specific raw integer**, with no dBm type or conversion until a validating normalization policy establishes the source semantics. Packet source/transmitter addresses are not assumed to be BSSIDs. Per-device strongest signals, lifetime GPS averages, datasource definitions, GPS coordinates, network identifiers and device JSON are never loaded into packet records. This reader therefore cannot paint a survey heatmap on its own.

Version 7 introduces PHY rate, version 8 correlation ID/CRC and version 9 original packet length. Earlier schemas retain explicit unknowns. Stored payload bytes may be absent while metadata remains useful, as in Kismet's [packet-stripping workflow](https://kismetwireless.net/docs/readme/kismetdb/kismetdb_strip_packets/). NULL or empty blobs with nonzero reported capture length become `NotRetained`; nonempty length contradictions fail. Capture length, original length and stored-payload availability remain separate. Actual packet bytes are not read into returned records.

## Validation and remaining gates

```sh
cargo test -p kyberia-kismet-adapter --locked --offline
cargo test -p kyberia-kismet-adapter --release --locked --offline metadata_batch_benchmark -- --ignored --nocapture
```

Twelve initial behavioral tests create independently constructed SQLite fixtures for versions 5–10, distinct tied-time receptions, malformed fields/schema, missing/duplicate sources, absent values, negative/extreme rowids, stripped payloads, cancellation/deadline limits, random corrupt headers, source mutation and exact snapshot hashes. No upstream implementation or real capture is used to generate fixtures. The explicit benchmark imports 10,000 and 100,000 original synthetic records in batches of 512, including snapshot/hash time.

On macOS 26.6.2 ARM64 with Rust 1.98.1 release optimization, 10,000 rows took 3.393 ms to snapshot/open and 9.338 ms total; 100,000 rows took 28.284 ms to snapshot/open and 79.069 ms total. These are local baseline measurements, not cross-platform performance claims or stable CI thresholds. Fixture creation is outside the timed region. Both runs verified every row was admitted exactly once.

These tests validate a decoding contract. A real supported Kismet installation must still produce controlled DB artifacts for field parity and version evidence. Canonical mapping, authenticated API/event consumption, reconnect/backpressure, remote sensor clocks/hopping/dwell, KismetDB-to-PCAPNG parity, packet parsing, privacy-aware storage and the complete runtime gate remain independent executable work. Missing producer binary version must become canonical unknown evidence; database schema version is not its replacement.
