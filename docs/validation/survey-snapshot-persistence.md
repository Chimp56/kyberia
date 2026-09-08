# Survey snapshot persistence validation

The focused suite is `cargo test -p kyberia-project-store --test
survey_snapshot --locked --offline`. It currently covers:

* current V2 save, reopen, checksum verification and receipt-associated state;
* legacy untagged V1 import, byte preservation and explicit migration receipt;
* duplicate idempotency, two revisions and append-only history;
* malformed, future-schema and over-budget input;
* wrong-session lookup, checksum corruption and missing artifact;
* optimistic stale-handle rejection and readback after the winning write; and
* projection failure rollback with no manifest revision, index or history row;
* equal timestamp acceptance, timestamp regression rejection and side-effect-free
  future manifest-only opening;
* future manifest preflight with a valid WAL, plus separate rejection of an
  unreadable rollback journal, including the documented volatile `-shm`
  lock-state behavior;
* an oversized future SQLite manifest value constructed with `zeroblob`, which
  stays below the 64 MiB database envelope but is rejected by the 4 MiB value
  bound before a writable open;
* missing, extra and field-mismatched history rows;
* source and collector identity round trips and tamper rejection;
* bounded history listing with an adversarial 4097-row database; and
* history listing rejection for tampered media type, provenance and artifact
  bytes.

The surrounding project-store suites cover the existing manifest/artifact
transaction boundary, SQLite schema authorizer and database/sidecar resource
limits. Survey decoder and association suites cover state semantics and
association provenance independently of storage.

The test fixture uses only the repository's independently authored point-survey
fixture and constructs receipt association evidence through the public survey
API. No external application object model is persisted. Runtime power-loss,
disk-full and multi-platform migration tests remain release validation work;
they are separate from the deterministic contract tests here.
