# Canonical project CLI

The CLI creates a canonical baseline for every new bundle:

```sh
cargo run -p kyberia-cli -- new /path/to/home.rfatlas "Home"
```

`new` first validates the name, creates a fresh bundle and project identity,
then registers an empty domain `Project` at revision `0` and logical time `0`.
The baseline is immutable and is bound to the bundle project ID and name. The
command refuses an existing destination and does not remove a partially-created
bundle after an I/O failure; retain it for explicit diagnosis or recovery.

Use the read-only query to inspect the verified canonical state:

```sh
cargo run -p kyberia-cli -- query-canonical-project /path/to/home.rfatlas
```

The output schema is `kyberia.canonical-project-query/1`. Its `state` is one of:

- `baseline_only`: a canonical baseline is registered, but no materialized
  operation result has been published;
- `materialized_current`: the current materialized project and its validated
  publication receipt are present;
- `legacy_absent`: the bundle has no registered canonical baseline. The query
  leaves `project`, `baseline`, and `publication` absent and never reconstructs
  a project from manifest metadata.

The `manifest.bundle_revision` is the SQLite-backed bundle metadata revision.
For a materialized state, `publication.operation_project_revision` is the
operation-log revision and `publication.operation_max_causal_depth` is DAG
metadata. These values are intentionally reported separately from the domain
project's `revision` and `logical_time`.

The query opens the bundle read-only. It validates the manifest, immutable
baseline bytes, current pointer, publication row, artifact hashes, and domain
project before emitting a canonical state. Corrupt canonical artifacts produce
an error and no state is emitted. Legacy bundles remain inspectable with
`inspect` and can be migrated only through a future explicit, evidence-bound
workflow; this command does not invent geometry, revisions, or provenance.
