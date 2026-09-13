# Application project-session validation

This focused validation covers the first reusable application boundary for
canonical project lifecycle and single-snapshot queries. The tests exercise the
real `kyberia-project-store::Bundle` and causal materializer through
`kyberia-application`; production code contains no mock or UI shadow state.

`cargo test -p kyberia-application --locked --offline` covers:

- typed create then read-only reopen of the registered canonical baseline;
- duplicate-create rejection without replacing the original bundle;
- missing project rejection;
- corrupt canonical artifact rejection;
- unsupported project-schema rejection at application open, even though the
  storage adapter can inspect compatible future metadata read-only;
- immutable view and revision consistency when a second real store handle
  publishes a canonical materialized snapshot;
- explicit cancellation before a bounded synchronous query;
- explicit legacy absence when a bundle has no registered baseline; and
- invalid timestamp admission before the application reserves a directory.

Fixtures are retained under `.trash/test-runs` for diagnosis and manual cleanup.
The application maps storage failures to `ApplicationError` categories without
exposing SQLite, rusqlite, `Bundle`, or materialization-publication types.

This validation does not claim product UI completion, mutation command
orchestration, authorization, or cancellation during a synchronous storage
read. Those require later application/job work.
