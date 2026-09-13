# ADR-0029: Canonical application project sessions

- Status: Proposed bounded application increment
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
   state, canonical domain project, and distinct bundle, aggregate, logical,
   operation, and publication revision fields. A legacy bundle without a
   baseline remains explicit absence.

`ProjectStorePort` is an application-owned inward port returning only
application-owned values. The private production implementation wraps the
reviewed `Bundle` APIs; no `Bundle`, SQLite/rusqlite value, or publication
receipt is part of the public API. The crate is classified as a composition
layer in the dependency policy because its production adapter must call the
existing storage crate; domain and numerical crates remain unaware of it.

Cancellation is checked before and after the synchronous canonical query using
the existing pure `CancellationHook`. The store's own bounded SQLite,
manifest, artifact, materialization, and schema checks remain authoritative;
the application does not claim mid-read preemption.

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

## Validation

`crates/application/tests/project_session.rs` uses real bundles and real causal
materialization to prove create/reopen, duplicate-create retry behavior,
missing and legacy states, corrupt and unsupported project rejection,
immutable view/revision behavior across a canonical publication, cancellation,
and canonical data sourcing. It uses retained fixtures under
`.trash/test-runs/`; tests never recursively clean those directories.
