# ADR-0004: SQLite authority with immutable analytical observation chunks

Status: Proposed. Gate C remains open; research review is complete; production-port validation is required.

## Context

Plan §11 and §20 Gate C recommend SQLite metadata plus Parquet observations, subject to comparison with SQLite-only and embedded analytical engines. The current Rust project-store makes SQLite the committed manifest authority and treats external artifacts as immutable. High-volume observation persistence must preserve that ownership, unknown evidence, exact clocks and reproducible export.

## Proposed decision

Retain SQLite as project metadata and operation authority. Continue implementing a Rust port for immutable typed Parquet observation chunks, with transactional content references and a defined live append/finalization policy. Keep DuckDB as a research comparison and potential optional analytical query adapter; do not make its database format canonical or its runtime a mandatory product dependency on this proof alone.

## Alternatives

SQLite-only offers one mature transactional file and straightforward indexed point/time queries. It remains a viable small-project/live-spool implementation and a correctness reference. SQLite plus Parquet separates transaction-intensive metadata from scan-heavy immutable evidence, with more publication/recovery complexity. Embedded DuckDB offers a strong analytical engine and Arrow interoperability but adds a runtime, database compatibility policy and different memory/deployment costs.

## Evidence

The [Gate C proof](../../validation/storage-proof.md) compares actual SQLite 3.50.4, PyArrow/Arrow 25.0.1 and DuckDB 1.5.5 on original 10k/100k/1m row projections with real writes, time/spatial/report queries, typed Parquet export/import and additive metadata migration. Its [machine-readable evidence](../../../research/storage/evidence/macos-arm64.json) records full parity and measured cost. Independent semantic tests include unknown-only groups, exact nanoseconds, duplicate rollback and abrupt process exit.

At one million rows the finalized frozen-source run measured 119.70 MB for SQLite + Parquet versus 438.24 MB for SQLite-only, with batch write time 14.20 versus 35.52 seconds. Selective indexed SQLite reads are faster than the Parquet scan path. DuckDB's warm report scan is 10.99 ms versus 543.38 ms for the implemented Parquet/Python reduction, but whole-process peak RSS reaches 751.39 MB versus 197.43 MB for the Parquet case. Query execution/reduction paths differ and peak RSS includes export/verification; these are implementation costs, not universal engine rankings.

The proof is one machine, one workload family and a partial canonical-compatible query projection; it does not prove the final Rust port or complete Gate C. All nine cases pass complete row/query/export/migration parity, and the evidence source hashes match the frozen research implementation. The measured footprint supports the default storage split while the scan advantage merits keeping an optional analytical engine behind a query port for further evaluation.

## Consequences

Canonical observations and schema semantics remain Kyberia-owned. Chunk finalization must publish durable immutable bytes before committing their reference; recovery retains unreferenced artifacts until an authorized garbage-collection policy applies. A live ingestion spool and bounded finalization are required to avoid treating incomplete Parquet files as usable evidence. IDs and unknown reasons must survive export, not merely the visible signal scalar. Analytical engine dependencies remain outside the pure domain.

## Reversibility

The production port remains replaceable behind inward contracts. Immutable normalized exports allow reconstruction without DuckDB or a specific query engine. A future benchmark can select a different spool/query adapter without changing canonical observation semantics or rewriting raw evidence.

## Validation plan

The experiment and required-provenance correction have independent review. Implement the production Rust writer/reader and compare against these fixtures; verify complete envelope parity and source provenance; exercise process crash, disk full, cancellation and migration backups; benchmark sustained live workloads and production queries; run at least Windows and macOS portability tests. Assess dependency notices and distributable footprint before accepting a new mandatory runtime. Gate C is not passed by this proposed ADR.
