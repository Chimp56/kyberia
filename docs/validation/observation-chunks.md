# Immutable observation chunk validation

Status: implementation increment independently approved; Gate C remains Proposed pending second-platform portability and production workload evidence.

The project-store implements the narrow `ObservationChunkStore` port with SQLite as metadata authority and the official Apache Arrow Rust `parquet` 59.3.0 crate as the canonical artifact writer/reader. Its default features are disabled; only `parquet/arrow` is enabled, with uncompressed Parquet pages and bounded row groups. A committed artifact begins and ends with the Parquet `PAR1` marker. Every V2 chunk uses the same fixed nullable superset of the closed `ObservationEnvelope` shape, identified by the `kyberia-envelope-v2-fixed-superset-1` fingerprint; absent payload/evidence values are null columns. Union paths use explicitly typed physical columns, so disjoint payload chunks expose identical analytical schemas. JSON is used transiently to bridge the domain's serde shape while constructing and reconstructing typed columns; no JSON payload is persisted.

Publication sorts rows by `ObservationId`, rejects duplicate IDs within a batch, finalizes and fsyncs the content-addressed artifact before one SQLite transaction records its hash, exact bytes, schema/codec versions, row count, ranges, provenance and unique observation members. Read and verify paths enumerate committed SQLite rows only, require manifest/descriptor/member/artifact bijection, rehash the complete artifact, validate the complete nested Arrow `Field` contract and Parquet footer metadata before page or batch materialization, and reconstruct every `ObservationEnvelope` through domain validation. The predecode budget rejects unsupported codecs, excessive row groups/values, declared compressed/uncompressed totals, out-of-file page offsets, and aggregate decoded Arrow allocations; repeated dictionary/RLE string expansion is bounded by the maximum string width multiplied by encoded value counts. Unsupported/future metadata, malformed schemas, row/column limits, truncated/corrupt bytes and noncanonical ordering fail closed. Cancellation is checked before encoding, after durable artifact publication and before metadata commit; published but unreferenced bytes remain invisible for explicit future retention work.

The native framed format has a distinct `application/vnd.kyberia.observation-chunk; format=native; version=1` media type and is not accepted by the committed analytical reader. No fallback is silently selected.

Focused validation:

```text
cargo test -p kyberia-project-store --locked --test observation_chunks
cargo test -p kyberia-project-store --locked
cargo clippy -p kyberia-project-store --all-targets --locked -- -D warnings
cargo fmt --all -- --check
export KYBERIA_PYARROW_PYTHON=/path/to/python
"$KYBERIA_PYARROW_PYTHON" tools/observation_chunks_pyarrow_oracle.py --expected-rows 1 <scan.parquet> <health.parquet>
```

The focused suite covers complete V2 envelope roundtrip including unknown reasons and signed/unsigned clock extremes, deterministic row order, disjoint payload schema equality, nonempty chain lists, duplicate rollback and idempotent retry, cancellation after durable publication with an invisible orphan, corruption, missing member indexes, unsupported media, generic-artifact poisoning prevention, SQLite-authoritative projection recovery, schema migration and read-only reopen behavior. Unit fixtures reject altered list child names, altered child nullability, compressed pages, oversized declared column bytes, excessive declared values, and repeated dictionary/RLE string expansion before reader or batch construction. The independent PyArrow oracle (supported range `>=15`; local evidence used PyArrow 25.0.1) reads both disjoint chunks with the standard reader and checks equal schemas, row counts, metadata fingerprint and uncompressed codecs; the integration test runs only when `KYBERIA_PYARROW_PYTHON` names an available interpreter and otherwise prints an explicit skip. Remaining production gates include process/disk-failure injection beyond deterministic cancellation, adversarial Parquet fuzzing and decompression-bomb testing, sustained backpressure/throughput measurements, and portability validation on a second operating system.

Independent review reproduced the fixed-schema, nested-field, decoded-allocation,
scalar/list count, cancellation, generic-publication, rollback and reopen gates.
The integrated SQLite merge additionally keeps survey, operation and observation
table groups independently exact and migrates all missing groups in one
transaction. See `docs/reviews/observation-chunks-review.md`.

The direct Rust dependency and all resolved transitive packages are recorded in `docs/licenses/cargo-sources.json`; regenerate that inventory after any lockfile change. `tools/architecture.json` records Arrow, Arrow schema, bytes and Parquet as adapter-only dependencies. Gate C remains open until the independent review accepts the implementation and the portability/performance evidence is complete.
