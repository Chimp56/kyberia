# Kismet adapter

Kismet remains an external GPL integration. Kyberia owns canonical observations and Wi-Fi semantics. The adapter contains a read-only KismetDB packet-metadata reader and bounded normalizer plus a bounded live REST status/capability client. It does not decode PCAPNG, consume event streams, invoke capture helpers, change Kismet configuration, or validate the full Kismet runtime gate.

## Live status and capability boundary

`live::KismetLiveClient` uses the pinned `ureq` 2.12.1 blocking HTTP client
with its rustls TLS feature. Its agent has redirects disabled, environment
proxy discovery disabled by the selected feature set, and a bounded overall
attempt timeout; each request also receives the remaining shared poll budget.
Plaintext HTTP endpoints are accepted only for literal loopback addresses used
by local fixtures; remote Kismet endpoints must use HTTPS.
The client requests exactly
`/system/status.json`, `/datasource/types.json`, and
`/datasource/all_sources.json` with an API token in the `KISMET` cookie. It
never calls `/session/check_session` (that endpoint validates browser login
sessions, not API keys), `/datasource/list_interfaces.json`, or a mutating
route. Redirects are reported as an error and cannot forward the cookie.

The token and endpoint are opaque, non-serializable values. Their debug
representations and every public error omit secret and URL details. Response
bytes, JSON depth, source/type/list counts, string lengths, retries and
backoff are bounded. A global poll deadline is shared across all three
requests; the underlying socket is bounded by ureq's attempt timeout, while
cooperative cancellation is checked before each request and during retry
backoff. A syscall already blocked in a socket read can finish at that
timeout before cancellation is observed.

The supported producer version is the inspected acceptance tuple from the
pinned Kismet source (`2026.09.0` with source identity `2d25ad0`, or its full
40-character commit). Kismet's date version is a build date, so a future or
otherwise syntactically valid version is rejected until a new pinned server
review adds it. Missing or malformed versions/source identities, duplicate
JSON object keys, unsupported root shapes and wrong known-field types fail
closed. Unknown additive Kismet fields are not copied into Kyberia; the
result records the known field names actually available. Missing channel,
hop, source-error and server-clock fields remain `null`/unknown. Source error
packet counts are operational counters, not packet observations or
transport-drop measurements. Datasource inventory is sorted by UUID/type
before canonical serialization and is never deduplicated across physical
radios.

`LiveStatusSnapshot::canonical_bytes` and `canonical_sha256` provide a stable,
secret-free status receipt. The receipt is an operational capability snapshot,
not a capture authorization, observation envelope, clock synchronization
proof, channel legality claim, or measurement result. A real Kismet server
parity gate remains open; the retained local TCP fixture proves transport and
decoder behavior only.

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

## Canonical observation normalization

`KismetDb::normalize_batch(after, limit, context)` maps one bounded packet batch
to V2 `ReceivedObservation` values plus an explicit `ImportReceipt` and one
`ObservationReceipt` per row. The caller supplies the canonical session,
source/collector mappings and privacy policy. Foreign datasource UUIDs are
validated evidence and never become Kyberia IDs. Observation IDs are stable
content-derived values from the immutable database hash and SQLite row ID;
replayed batches therefore have deterministic identity without treating a
packet ID or CRC as a deduplication key.

The packet `ts_sec`/`ts_usec` fields become a known source-reported UTC wall
timestamp with microsecond precision. They do not become a monotonic clock,
receipt time or synchronization model. Kismet's schema version is recorded as
`kismetdb/<version>`; the producer/software version remains
`Unknown(SourceDidNotProvide)`. The reported kHz frequency becomes a typed
source-reported primary frequency, while band, channel number, width, second
segment, puncturing, dwell and pose remain unknown because these supported
packet rows do not establish that geometry.

The PHY-specific `signal` integer is retained only in the row receipt and is
explicitly unavailable as dBm in the canonical frame. Noise, frame subtype,
retry, radio/BSSID/ESS/MLD identities, per-chain values and information
elements remain unknown or not retained. Version-gated PHY rate and captured
length remain typed known fields. A Kismet source-error flag becomes
`Malformed` quality, preserving the row while preventing clean-evidence use.
The source database is referenced by its content hash and exact byte length;
the normalizer never persists its payload or invents a source response time.

`complete` describes the requested row tail, not the whole import unless the
caller began with `after=None`, consumed every cursor in order, and called
`finish()`. A failed mapping or canonical conversion returns an error without
returning a partial normalized batch. Payload-retaining privacy policy is
rejected because this metadata-only path does not emit packet bytes.

## Validation and remaining gates

```sh
cargo test -p kyberia-kismet-adapter --locked --offline
cargo test -p kyberia-kismet-adapter --release --locked --offline metadata_batch_benchmark -- --ignored --nocapture
```

The behavioral suite creates independently constructed SQLite fixtures for
versions 5–10, distinct tied-time receptions, malformed fields/schema,
missing/duplicate sources, absent values, negative/extreme rowids, stripped
payloads, cancellation/deadline limits, random corrupt headers, source
mutation and exact snapshot hashes. Normalization tests additionally cover
canonical frame construction, source/hash/row receipts, schema-gated fields,
timestamp and channel unknowns, required mappings, privacy rejection, source
errors, deterministic bytes and paginated completeness. No upstream
implementation or real capture is used to generate fixtures. The explicit
benchmark imports 10,000 and 100,000 original synthetic records in batches of
512, including snapshot/hash time.

On macOS 26.6.2 ARM64 with Rust 1.98.1 release optimization, 10,000 rows took 3.393 ms to snapshot/open and 9.338 ms total; 100,000 rows took 28.284 ms to snapshot/open and 79.069 ms total. These are local baseline measurements, not cross-platform performance claims or stable CI thresholds. Fixture creation is outside the timed region. Both runs verified every row was admitted exactly once.

These tests validate a decoding and metadata-normalization contract. A real
supported Kismet installation must still produce controlled DB artifacts for
field parity and version evidence. Exact source artifact persistence into a
project, transactional import publication/idempotency, authenticated API/event
consumption, reconnect/backpressure, remote sensor clocks/hopping/dwell,
KismetDB-to-PCAPNG parity, packet parsing, privacy-aware storage and the
complete runtime gate remain independent executable work. The metadata-only
normalizer does not claim complete channel/dwell/pose, BSSID or PHY signal
semantics and cannot produce survey samples without application association.
Missing producer binary version must become canonical unknown evidence;
database schema version is not its replacement.
