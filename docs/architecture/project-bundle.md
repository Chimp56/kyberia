# Initial transactional project bundle

The `kyberia-project-store` adapter owns filesystem and SQLite side effects. It imports canonical project identity from `kyberia-domain`; the domain does not import storage. This is the initial container proof for plan §§11, 18.1 and Gate C, not the completed observation database or analytical storage decision.

A project is a newly reserved directory containing `project.sqlite`, `manifest.json`, and `artifacts/<lowercase-sha256>`. SQLite stores the committed versioned manifest and revision in one transaction. JSON is a redundant, human-readable projection. Artifact bytes are immutable, bounded to 64 MiB per chunk, and checksummed before use. The manifest is bounded to 4 MiB and 10,000 entries. Formats requiring larger data must chunk through their owning adapter; this interface never silently truncates input.

Artifact imports persist and sync bytes before registration. A write transaction rechecks the current physical schema, singleton record and revision, requires exactly one changed row, and reads back the authoritative manifest and exact serialized bytes before publishing the projection while holding the writer lock and committing. Failed registration can leave an unreferenced blob. A crash between projection publication and commit can leave a stale projection; `verify` diagnoses it and explicit `recover-manifest` rebuilds it from committed SQLite. Recovery does not repair corrupted evidence and exits unsuccessfully if verification still fails. Creation reserves a fresh directory and retains partial files on failure for diagnosis.

Version 1 uses SQLite's full synchronization and rollback journal. The main database and any SQLite `-wal`, `-journal`, or `-shm` recovery sidecars must be nonsymlink regular files and share one 64 MiB read budget, checked before open and before each operation. This permits bounded crash recovery without letting a small main file hide an unbounded sidecar. The bundle manifest schema version must match SQLite `user_version`. Unknown logical versions using the same supported physical manifest-table envelope can be inspected read-only; writable opens and already-open writer handles reject unsupported versions and required features. No fictitious upgrade migration is supplied for a nonexistent prior format. Future migrations require archived fixtures, transactional conversion and backup/recovery tests.

Before reading imported metadata, SQLite views and triggers are disabled and defensive mode is enabled. The adapter reads only SQLite's built-in schema catalog to require the exact manifest-table DDL emitted by the existing V1 writer, with no other schema objects. Manually rewritten SQL, extra indexes/tables, virtual or generated-column tables, views and triggers are rejected. This does not change the bytes written by V1; a future physical-schema migration must explicitly extend the reviewed schema allowlist and supply compatibility fixtures. A compatible future JSON version alone does not authorize foreign executable SQL.

An SQL authorizer restricts the connection to the manifest/catalog reads, manifest-body/revision updates, transaction control and specific required pragmas. Reads and writes revalidate the schema within their transactions, including previously opened handles. A metadata file over 64 MiB is rejected before opening and at subsequent public operation boundaries. Each operation receives a 2,000,000 SQLite VM-operation budget and a five-second elapsed-time check every 1,000 VM operations; interruption returns an error. SQLite lock waits retain their three-second busy timeout. These are cooperative SQL limits, not a hard process-memory or total wall-time sandbox: filesystem calls, native parsing/I/O stalls and a callback interval cannot be forcibly preempted. The 64 MiB cap provides headroom above the 4 MiB manifest and SQLite copy/freelist overhead; it is a format-proof resource policy, not a measured performance SLA.

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
