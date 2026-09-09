# Materialized publication correction review

Root independently reviewed Russell's correction `b77e553` atop publication
candidate `3ac3e10`. No BLOCKER or MAJOR finding remains in the bounded correction.
This continues the earlier publication, pointer and shared-budget reviews; it
does not replace their historical findings or promote product collaboration UX.

- STORE-BUDGET-001: closed. Baseline/publication/state and operation row paths
  now inspect SQLite UTF-8 byte lengths as scalars before selecting owned text
  or BLOB values. Fixed schema bounds apply to lookup, retry and inventory paths.
  Historical verification charges metadata into its shared caller budget.
  Queries remain within one transaction, preserving the preflight/read snapshot.
- STORE-BUDGET-002: closed. Artifact reads inspect the same no-follow file handle,
  reject actual size differing from the declared size before allocation, and
  read into a fixed admitted buffer with a separate one-byte overrun guard.
  Growth cannot force the buffer to expand; truncated bytes fail integrity checks.
- Generic `Bundle::verify` still has per-file limits without a cumulative artifact
  byte budget. This remains documented scoped debt, not a whole-bundle memory claim.

Root confirmed exact candidate source equality with `b77e553` before adding
tests. Its `bundle.rs` SHA-256 was
`cdd3aae0ae27c2d3708341814da82b60d22ee46e374de2641592b344862e5e68`.
Current-main integration in `.worktrees/publication-integration` passed 127
project-store tests (one ignored), focused Clippy, and 615 workspace tests
(nine ignored), with zero failures. Logs are retained under that tree's `.trash/`.
Two subsequent root-authored tests pass for growth after metadata at zero, one
and 1,024 admitted bytes, and truncation after metadata. Russell independently approved both tests with no findings and reran the focused
tests, Clippy and diff checks successfully.

Windows evidence is deliberately limited. `x86_64-pc-windows-gnu` was installed.
Full `cargo check -p kyberia-project-store --target x86_64-pc-windows-gnu --locked
--offline` stopped in `libsqlite3-sys` because `x86_64-w64-mingw32-gcc` is absent.
A retained exact file-open-branch harness in
`.worktrees/materialized-project-publication/.trash/windows-compile-b77e553`
passes `cargo check --target x86_64-pc-windows-gnu --locked --offline` with
`windows-sys 0.61.2`. This checks Windows API names/types, including file-system
`SECURITY_IDENTIFICATION`, but does not prove full crate compilation, linking,
filesystem behavior or Windows runtime security. Those remain validation gates.

Subsequent native CI at `d5050f3` passes the complete foundation/CLI build on
Windows, macOS and Linux. This closes the full Windows build-evidence gap;
validation jobs are still running. See the [public CI checkpoint](../validation/ci-publication-checkpoint.md).
The earlier local MinGW failure remains a local-toolchain observation, not a
claim that Windows compilation is externally blocked.
