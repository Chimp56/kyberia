# Test directory retention audit

The user's AGENTS.md requires explicit target-scoped permission before
recursive deletion. Test directory destructors are included: do not assume
general test authorization approves their automatic cleanup.

Review found direct `tempfile::tempdir()` values retained as `TempDir` in the
observation-pipeline tests and two Kismet database unit tests. These values
automatically removed their directories when earlier test invocations ended.
That behavior did not comply with the user's rule; root reported it and
paused affected test execution while the helpers are corrected.

Current remediation ownership:

- Native session author: observation-pipeline unit/external tests and
  executable-fixture helpers, including failure and unwind paths.
- Kismet author: the two database source-open unit-test directories.
- Stored-analysis author: new bundle workflow fixtures in the unintegrated
  stored-analysis crate.

Use immediately retained paths (`TempDir::keep`) or a wrapper owning only the
retained path. A wrapper must not recursively clean up in `Drop`. Retention
must happen before assertions or other fallible test work, so test failure
does not restore automatic deletion. Keep generated directories outside the
tracked source tree or under ignored build-output directories. Do not remove
old directories while performing this correction.

Root's first-party source audit used `git grep` for temporary-directory and
recursive-cleanup APIs. The inspected CLI/project-store Rust tests already
use retained paths; inspected Python test helpers use `mkdtemp` without
automatic recursive teardown. Kismet's integration-test directory helper is
also retained. This is a source audit of first-party test helpers, not a claim
about every internal operation of compilers or third-party dependencies.

Affected suites may resume after the corrections are reviewed. A later cleanup
requires permission for the concrete retained paths; neither this document
nor a passing test grants that permission.
