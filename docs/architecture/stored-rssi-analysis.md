# Stored RSSI analysis workflow

`kyberia-stored-analysis` is the outward application composition for a
revision-bound measured RSSI tile. It opens no files itself and does not decode
external formats. `Bundle` first verifies the selected observation chunks and
returns canonical envelopes plus an `ObservationQueryReceipt`; the workflow
maps that receipt's exact chunk hashes into the storage-independent
`SelectionSourceBinding`. Survey snapshots are loaded through the project-store
replay boundary and are checked against the request project and revision.

The request owns the spatial floor/frame binding. A snapshot has a historical
commit revision because snapshots are immutable records; that revision must be
nonzero and no later than the query's committed bundle revision. The workflow
pins one requested project revision, checks it before loading, consumes the
query receipt at that revision, and checks the manifest again before publishing
the result. A concurrent write therefore fails closed instead of producing an
output with mixed evidence revisions.

The inward boundary remains `kyberia-observation-analysis`: it receives only
canonical observations, `PointSurvey` values, a receipt-derived source binding,
and explicit metric/spatial configuration. It does not import project-store,
SQLite, Parquet, capture adapters, or UI types. The spatial model retains
unknown cells and support metadata according to its configured method; this
workflow never synthesizes positions, signal values, calibration, or spectrum
evidence.

The result has schema `kyberia.stored-rssi-analysis/1`. Its canonical JSON
retains the exact selection manifest bytes, selection artifact reference,
selected source chunk hashes, snapshot artifact hashes/revisions, and the
serialized numerical tile. The result artifact hash is over those complete
canonical bytes. The tile is returned separately for numerical consumers, but
the bytes and provenance document remain available for replay and independent
verification.

The store's `Bundle::load_survey_snapshot_with_cancel` is a deliberately
narrow adapter extension required by this composition: snapshot replay is
bounded and checks cancellation before and around the artifact read and
decoder. It preserves the existing non-cancellable convenience methods and
does not expose SQLite, artifact paths, or decoder internals to the application
or numerical crates.

Cancellation is checked before storage work, around every snapshot load and
query, before and after selection, during tile computation, before encoding,
and before the final revision check. No partial tile or partially published
result is returned. Storage may retain an unreferenced immutable artifact only
where its own documented publication behavior allows it; this workflow itself
does not write a project bundle.

The current API is intentionally a callable composition function. A CLI
subcommand will be added with the native capture/session command ownership so
that argument parsing and project lifecycle do not diverge between two agents.
