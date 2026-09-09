# Planar geometry candidate validation

The corrected candidate `f247510` is now integrated through `b647d6f` after
[independent combined review](../reviews/planar-geometry-adapter-review.md).
Root reran all nine geometry tests and workspace lint successfully on main.
The combined CLI/geometry inventory verifies 241 locked external packages;
pinned cargo-deny 0.20.2 passes advisories, bans, licenses and sources on this
combined closure using the unchanged repository policy. Its retained log is
`.tools/cli-geometry-dependency-audit.log`.

The original candidate evidence below is historical. Its initial numerical
defects were corrected before integration; it is not the accepted code state.

Candidate `e891002` is isolated in `feat/planar-geometry`; not integrated.
Root author checks passed:

- `cargo test -p kyberia-geometry-adapter --offline --locked`: three tests,
  including 99 analytic symmetric cases under endpoint/operand permutations.
- Focused all-target Clippy with warnings denied and formatting.
- `cargo check -p kyberia-geometry-adapter --target wasm32-unknown-unknown --locked --offline`.
- Architecture dependency checks and 239-package source inventory generation.

WASM compilation is not runtime validation. Independent adversarial numerical
review is running; no acceptance or Gate E completion is claimed here.

Root also passed the pinned cargo-deny 0.20.2 workspace audit against this
candidate: advisories, bans, licenses and sources all passed. The first run
failed to locate the relative advisory database from the isolated worktree;
the successful run used an ignored copy of the main deny.toml changing only
`advisories.db-path` to the existing main absolute database path. No allowlist,
advisory severity, source policy or dependency version was relaxed. The local
log is retained at the candidate's `target/geometry-dependency-review.log`.

The initial combined regression attempt was denied local socket binding by
the sandbox in five Kismet HTTP fixture tests (`PermissionDenied` at bind).
That run is retained in `.tools/cli-geometry-regression.log`; it is not passing
evidence. The authorized loopback-capable rerun is recorded separately.

The loopback-capable full workspace rerun passed: 451 tests passed,
0 failed, 9 explicitly ignored, including doctests. Command:
`cargo test --workspace --locked --offline --quiet`. Retained log:
`.tools/cli-geometry-regression-unsandboxed.log`.
