# CPU-only Sionna removal validation

Date: 2026-09-15
Decision: [ADR-0043](../architecture/ADR/0043-cpu-only-sionna-worker.md)

## Active contract

- The Sionna worker selects only `llvm_ad_mono_polarized`. Its worker version is
  `0.2.0`, transport schema remains version 1, and capability schema version 2
  publishes only `cpu_llvm`, operations, scene kinds, and product-tier status.
- The worker does not enumerate installed Mitsuba variants. Former accelerator
  backend values fail request validation, and former accelerator capability
  fields fail result validation.
- Kyberia Lab MCP coordinator configuration schema version 2 removes the former
  accelerator suite. The server exposes no device selector. Coordinator and
  runner host-capability arrays reject the retired accelerator identifiers, and
  the manager rejects unexpected probe parameters even when called directly.
- Generic browser `wgpu`/WebGL rendering remains in scope. It is not a Sionna
  execution backend and is unaffected by this decision.

Historical evidence and completed review records remain immutable. Their old
capability fields describe the artifacts that were actually reviewed; active
validation accepts them only through the explicit `allow_legacy=True` evidence
path. They do not establish current capability.

## Dependency and container audit

A case-insensitive manifest scan covered Rust and Node manifests and locks,
Python requirements and project metadata, Docker/Containerfiles, compose files,
environment examples, and YAML. It found no NVIDIA runtime, accelerator image,
driver environment, cuDNN package, or CUDA-specific dependency. The Sionna
worker has no container manifest. Its pinned `sionna-rt`, Dr.Jit, and Mitsuba
packages remain because the active LLVM implementation uses them.

## Regression evidence

The candidate passed:

- 54 Sionna contract/lifecycle tests (2 Windows-only skips), including CPU-only
  capability shape, retired selector rejection, and absence of runtime variant
  enumeration;
- 32 Phase 0 research-harness tests and 31 implementation-ledger tests;
- source-qualified ledger reconstruction and generated gate-catalog checks;
- the complete Lab MCP `pnpm run check` gate, including TypeScript, format,
  signed coordinator/runner integration, schema rejection, package inventory,
  and supply-chain verification;
- `cargo fmt --all -- --check` and the complete offline Rust workspace test
  suite (the loopback HTTP fixtures were run outside the filesystem sandbox so
  they could bind localhost);
- architecture-direction, source-inventory, generated-fixture, and ledger
  checks. Source inventory used Python 3.9 with the isolated
  `/private/tmp/kyberia-security-preflight-python` compatibility module
  (`tomli` 2.4.1);
- repository diff whitespace validation.

The checked-in CPU proof remains historical worker-0.1.0 evidence. A new real
engine proof is not promoted until this candidate is independently reviewed and
the worker-0.2.0 artifact is executed in the pinned CPU/LLVM environment.
