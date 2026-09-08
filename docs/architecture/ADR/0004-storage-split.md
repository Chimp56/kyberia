# ADR-0004: SQLite authority with immutable analytical observation chunks

Status: Proposed. Gate C remains open; research review is complete; production-port validation is required.

## Context

Plan §11 and §20 Gate C recommend SQLite metadata plus Parquet observations, subject to comparison with SQLite-only and embedded analytical engines. The current Rust project-store makes SQLite the committed manifest authority and treats external artifacts as immutable. High-volume observation persistence must preserve that ownership, unknown evidence, exact clocks and reproducible export.

## Proposed decision

Retain SQLite as project metadata and operation authority. The project-store now has a narrow inward observation-chunk port backed by the pinned Apache Arrow Rust `parquet` 59.3.0 implementation with only its `arrow` feature enabled. Finalized chunks are real Parquet files with a deterministic fixed nullable V2 superset schema (`kyberia-envelope-v2-fixed-superset-1`), explicit schema metadata, complete nested Arrow `Field` validation, uncompressed bounded row groups and complete envelope reconstruction; JSON is an in-memory structural bridge and is not persisted in the artifact. A conservative decoded Arrow allocation budget is checked from footer value counts before a batch reader is built, including bounded expansion for repeated string values encoded with dictionary or RLE pages. Content references and observation-ID uniqueness are committed transactionally after the durable artifact is finalized. A native framed representation remains a separately labeled live-spool option and is not accepted as the analytical chunk format. Keep DuckDB as a research comparison and potential optional analytical query adapter; do not make its database format canonical or its runtime a mandatory product dependency on this proof alone.

## Alternatives

SQLite-only offers one mature transactional file and straightforward indexed point/time queries. It remains a viable small-project/live-spool implementation and a correctness reference. SQLite plus Parquet separates transaction-intensive metadata from scan-heavy immutable evidence, with more publication/recovery complexity. Embedded DuckDB offers a strong analytical engine and Arrow interoperability but adds a runtime, database compatibility policy and different memory/deployment costs.

## Evidence

The [Gate C proof](../../validation/storage-proof.md) compares actual SQLite 3.50.4, PyArrow/Arrow 25.0.1 and DuckDB 1.5.5 on original 10k/100k/1m row projections with real writes, time/spatial/report queries, typed Parquet export/import and additive metadata migration. Its [machine-readable evidence](../../../research/storage/evidence/macos-arm64.json) records full parity and measured cost. Independent semantic tests include unknown-only groups, exact nanoseconds, duplicate rollback and abrupt process exit.

At one million rows the finalized frozen-source run measured 119.70 MB for SQLite + Parquet versus 438.24 MB for SQLite-only, with batch write time 14.20 versus 35.52 seconds. Selective indexed SQLite reads are faster than the Parquet scan path. DuckDB's warm report scan is 10.99 ms versus 543.38 ms for the implemented Parquet/Python reduction, but whole-process peak RSS reaches 751.39 MB versus 197.43 MB for the Parquet case. Query execution/reduction paths differ and peak RSS includes export/verification; these are implementation costs, not universal engine rankings.

The proof is one machine, one workload family and a partial canonical-compatible query projection; it does not prove the final Rust port or complete Gate C. All nine cases pass complete row/query/export/migration parity, and the evidence source hashes match the frozen research implementation. The measured footprint supports the default storage split while the scan advantage merits keeping an optional analytical engine behind a query port for further evaluation.

## Consequences

Canonical observations and schema semantics remain Kyberia-owned. Chunk finalization must publish durable immutable bytes before committing their reference; recovery retains unreferenced artifacts until an authorized garbage-collection policy applies. A live ingestion spool and bounded finalization are required to avoid treating incomplete Parquet files as usable evidence. IDs and unknown reasons must survive export, not merely the visible signal scalar. Analytical engine dependencies remain outside the pure domain.

## Reversibility

The production port remains replaceable behind inward contracts. Immutable normalized exports allow reconstruction without DuckDB or a specific query engine. A future benchmark can select a different spool/query adapter without changing canonical observation semantics or rewriting raw evidence. Arrow and Parquet objects do not cross the adapter boundary, and generic artifact import rejects the normalized-observation kind so the native spool media type cannot become committed analytical evidence by accident.

## Validation plan

The experiment and required-provenance correction have independent review. The production Rust writer/reader now includes deterministic schema, metadata-budget, cancellation-window and SQLite-authority evidence, plus an independent PyArrow 25.0.1 interoperability oracle. Remaining validation must exercise process crash and disk full beyond deterministic rollback, adversarial Parquet fuzzing, sustained live workloads and production queries, and at least Windows and macOS portability. Assess dependency notices and distributable footprint before accepting a new mandatory runtime. Gate C is not passed by this proposed ADR.
