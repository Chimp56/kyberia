# Initial transactional project bundle

The `kyberia-project-store` adapter owns filesystem and SQLite side effects. It imports canonical project identity from `kyberia-domain`; the domain does not import storage. This is the initial container proof for plan §§11, 18.1 and Gate C, not the completed observation database or analytical storage decision.

A project is a newly reserved directory containing `project.sqlite`, `manifest.json`, and `artifacts/<lowercase-sha256>`. SQLite stores the committed versioned manifest and revision in one transaction. JSON is a redundant, human-readable projection. Artifact bytes are immutable, bounded to 64 MiB per chunk, and checksummed before use. The manifest is bounded to 4 MiB and 10,000 entries. Formats requiring larger data must chunk through their owning adapter; this interface never silently truncates input.

Artifact imports persist and sync bytes before registration. A write transaction rechecks the current schema and revision, publishes the projection while holding the writer lock, then commits. Failed registration can leave an unreferenced blob. A crash between projection publication and commit can leave a stale projection; `verify` diagnoses it and explicit `recover-manifest` rebuilds it from committed SQLite. Recovery does not repair corrupted evidence and exits unsuccessfully if verification still fails. Creation reserves a fresh directory and retains partial files on failure for diagnosis.

Version 1 uses SQLite's full synchronization and rollback journal. The bundle manifest schema version must match SQLite `user_version`. Unknown compatible versions can be inspected read-only; writable opens and already-open writer handles reject unsupported versions and required features. No fictitious upgrade migration is supplied for a nonexistent prior format. Future migrations require archived fixtures, transactional conversion and backup/recovery tests.

Stored artifact paths are generated from validated hashes. Direct symlinks and nonregular files are rejected, reads are bounded, and a declared artifact must match its content length and checksum. This initial directory API assumes the containing filesystem is not being concurrently replaced by another local process. Descriptor-relative confinement, adversarial filesystem races, Windows durability, encrypted containers, streaming cancellation, multiple semantic references per blob, observation tables, analytical exports and Gate C comparative benchmarks remain open. Imported raw bytes are never executed or interpreted by this adapter.

The caller supplies timestamps and provenance IDs. An update timestamp older than the committed update is rejected; the application must expose clock correction rather than rewrite acquisition timestamps. Reusing identical bytes with a different semantic/provenance entry is explicitly rejected until the reference graph is implemented.

## CLI

```sh
cargo run -p kyberia-cli -- new /private/tmp/home.rfatlas Home
cargo run -p kyberia-cli -- inspect /private/tmp/home.rfatlas
cargo run -p kyberia-cli -- verify /private/tmp/home.rfatlas
cargo run -p kyberia-cli -- recover-manifest /private/tmp/home.rfatlas
```

Commands output JSON. Exit 0 means successful requested operation; exit 1 means completed verification found integrity failures; exit 2 means invalid arguments or an operation error. `new` never overwrites an existing directory. The desktop survey workflow remains under implementation.
