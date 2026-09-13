# ADR-0029: Canonical application project sessions

- Status: Accepted bounded application increment; mutation and job surfaces remain open
- Date: 2026-09-12
- Related: plan §§10.1, 10.4, 10.6, 11.3, Phase 1; FND-001, FND-004, FND-006; ADR-0001, ADR-0023

## Context

The repository has reviewed domain, operation/materialization, and project-store
contracts, but callers still assemble project creation and canonical reads
directly from storage adapter APIs. That leaves no inward-facing application
boundary for a desktop or future API session, and makes it easy to combine
independent reads or leak SQLite/publication types into a caller.

## Decision

Add `kyberia-application` with two explicit surfaces:

1. `Command` admits typed `CreateProject` and `OpenProject` lifecycle requests.
   Creation generates a nonzero domain `ProjectId`, creates a real bundle, and
   registers its immutable canonical baseline. Opening selects a typed
   read-only or read-write session and rejects an unsupported logical schema at
   the application boundary, including when the lower store can inspect future
   metadata read-only.
2. `ProjectQuery::CurrentSnapshot` reads one canonical store snapshot and maps
   it into an immutable `CurrentProjectView`. The view carries the project
   state, canonical domain project, and distinct bundle, project, logical,
   operation, and publication revision fields. A true read-only legacy bundle
   with its additive materialization table group entirely absent remains
   explicit absence; a partial group is corrupt and fails schema validation.
   A publication may be older than the
   current manifest when a later metadata/artifact commit does not replace the
   published project; the view therefore preserves both counters and accepts
   `publication_bundle_revision <= bundle_revision`.

`ProjectStorePort` is an application-owned inward port returning only
application-owned values and takes a caller-owned cumulative resource budget.
The private production implementation wraps the reviewed `Bundle` APIs; no
`Bundle`, `StoreError`, SQLite/rusqlite value, or publication receipt is part
of the public API. A private rusqlite dependency is used only to classify
typed SQLite error codes at this adapter boundary; it is not re-exported or
present in any public signature. The crate is classified as a composition
layer in the dependency policy because its production adapter must call the
existing storage crate; domain and numerical crates remain unaware of it.

Opening first admits the requested root path. Only an absent root maps to
`MissingProject`; missing `project.sqlite`, artifact directories, or declared
artifact files inside an existing root map to `CorruptProject`. Store failures
are mapped privately by operation context. Typed publication budget failures,
resource-budget exhaustion, and SQLite operation/resource exhaustion map to
`ResourceLimit`; typed SQLite busy/locked/I/O failures map to `Storage`; only
typed SQLite corruption/not-a-database failures map to `CorruptProject`.
Caller admission failures remain `InvalidRequest`; persisted invalid-content
messages are never parsed to infer an error category.

Every snapshot mapping revalidates the logical schema version and required
feature set. This protects an already-open session from a concurrent logical
format advance. The application passes one cumulative budget through the
store's publication verification of the baseline and all materialized current
history, and polls the same caller-owned cancellation hook before, during, and
after that bounded synchronous work. Legacy snapshots bypass the verifier only
when their optional materialization table group is entirely absent. A partial
group is corrupt and is rejected by schema validation. The final canonical
snapshot read has no
budget-taking store API, so its fixed SQLite, manifest, artifact, and schema
limits remain an additional authoritative bound.

## Alternatives

- Keep CLI/UI code opening `Bundle` directly: rejected because each caller
  would reimplement lifecycle and could merge independently committed reads.
- Expose `Bundle` or `CanonicalProjectSnapshot`: rejected because storage and
  publication adapter types would become application API contracts.
- Add a new store implementation in the domain: rejected because domain
  dependency direction and side-effect-free canonical contracts must remain
  intact.
- Treat a future-schema read-only bundle as a supported current project:
  rejected because current application views cannot safely interpret unknown
  project semantics.

## Consequences and limits

Create/open and a single canonical current query are now reusable by UI, CLI,
and service composition code. Exact baseline registration and artifact
integrity remain delegated to the real project-store behavior, and duplicate
create is an explicit conflict because create reserves a new path. Command
application for project mutations, operation-log append/materialization
publication orchestration, survey persistence, authorization, and asynchronous
job cancellation remain follow-on increments.

The application view copies the bounded canonical project returned by the store
and exposes only shared immutable domain methods. Revision counters retain
their distinct semantics so callers cannot mistake a bundle metadata commit for
an operation or aggregate revision.

The application budget regression builds a real multi-publication history and
shows that a limit below the cumulative replay copy charge fails after work
has begun. A single publication would not prove that the budget is shared
across history rows. Private unit tests construct typed SQLite failures and a
resource-looking publication corruption message to protect the error mapping
contract from message-based classification.

## Validation

`crates/application/tests/project_session.rs` uses real bundles and real causal
materialization to prove create/reopen, duplicate-create retry behavior,
missing roots and internal files, declared-artifact and malformed-manifest
corruption, unsupported project rejection both at open and after a concurrent
logical advance, immutable view/revision behavior across publication followed
by an ordinary `MapSource` commit, cancellation, cumulative budget exhaustion,
legacy optional-table absence, typed SQL error mapping, and canonical data
sourcing. It uses retained fixtures under
`.trash/test-runs/`; tests never recursively clean those directories.
