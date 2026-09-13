# Application project-session validation

This focused validation covers the first reusable application boundary for
canonical project lifecycle and single-snapshot queries. The tests exercise
the real `kyberia-project-store::Bundle` and causal materializer through
`kyberia-application`; production code contains no mock or UI shadow state.

`cargo test -p kyberia-application --locked --offline` covers:

- typed create then read-only reopen of the registered canonical baseline;
- duplicate-create rejection without replacing the original bundle;
- missing project-root rejection;
- missing `project.sqlite` and declared artifact rejection as corruption;
- corrupt canonical artifact and malformed manifest rejection;
- unsupported project-schema rejection at application open, even though the
  storage adapter can inspect compatible future metadata read-only;
- revalidation of unsupported schema and required features after a session is
  already open;
- immutable view and revision consistency when a second real store handle
  publishes a canonical materialized snapshot and then commits an ordinary
  `MapSource` artifact at the next bundle revision;
- explicit cancellation before and during a bounded synchronous query;
- a real two-operation, three-publication history proving one cumulative
  application resource budget rejects replay after its shared copy quota is
  exhausted;
- explicit legacy absence when a bundle has no registered baseline; and
- invalid timestamp admission before the application reserves a directory.

Fixtures are retained under `.trash/test-runs` for diagnosis and manual
cleanup. The application maps storage failures to `ApplicationError` categories
without exposing `StoreError`, SQLite, rusqlite, `Bundle`, or
materialization-publication types. Internal operation context reserves
`MissingProject` for the requested root and maps resource/quota failures to
`ResourceLimit`.

This validation does not claim product UI completion, mutation command
orchestration, authorization, or preemptive cancellation during filesystem or
SQLite calls. Those require later application/job work; the existing budget
hook is polled at the store's deterministic verification boundaries.
