# Executable dependency boundaries

`tools/architecture.json` assigns every workspace crate a reviewed layer and an explicit production dependency allowlist. `tools/architecture.py` inspects locked Cargo metadata and rejects unregistered crates, outward internal dependencies, undeclared foreign packages, and mismatched local crate paths. Optional, conditional-platform and build dependencies are checked; test-only dependencies remain outside the production direction check. Renaming a dependency does not hide its original package identity.

The canonical domain admits Serde only. Numerical/application crates may depend inward. Adapters translate between approved external packages and inward contracts. Executable composition roots may assemble adapters. Adding a package requires updating this policy as part of the same independent architectural review, not disabling the check.

This is a declared dependency gate, not a Rust semantic proof: it cannot identify a foreign schema manually copied into an admitted type, standard-library side effects, FFI inside an approved package or transitive changes within allowed upstream dependencies. Code review, numerical/adapter tests, lockfile/source inventory and supply-chain audit remain required. The checker does not substitute for licensing review or certify a process runtime merely because its types are isolated.

The CI matrix declares Linux, macOS and Windows validation using the same local command runner. Until actual hosted jobs execute, those entries are planned execution targets, not evidence of cross-platform success. Current E2E tests launch the real project CLI. Desktop accessibility, rendering, capture permissions, packaging and optional-worker installation remain separate gates.
