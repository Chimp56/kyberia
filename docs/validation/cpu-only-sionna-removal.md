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

## Revision-scoped regression evidence

### Earlier candidate `d1c2bb9`

The independent review report on ref `review/remove-cuda-application` records
that the earlier immutable candidate `d1c2bb9` passed the Lab MCP
`pnpm run check` gate: 33 tests, 32 passed and one Windows-only skip. That
report predates the broader accelerator-alias rejection added at `121dab5` and
the `wgpu-cuda` regression case at `84a5bcb`; it is historical evidence, not
validation of the current tree. The same report identifies the full offline
Rust workspace result as author-reported and not independently repeated. The
earlier `cargo fmt`/Rust results are not promoted here as independently
verified evidence.

### Integrated candidate `84a5bcb` and follow-up test correction

The independent runtime review at `84a5bcb` found no code findings. A direct
run of every command in the Lab MCP `check` script, using the already-installed
package binaries (without invoking pnpm), passed after isolating the portable
`wgpu` assertion from the test's intentionally mutated private-key fixture:

- Prettier check, TypeScript no-emit check and build, and runner bundle build;
- 33 Node tests: 32 passed, one Windows-only skip;
- package inventory and supply-chain/SBOM verification.

The `pnpm` wrapper itself was not run: Corepack attempted to resolve pnpm from
the npm registry and the environment's DNS/network access failed. The direct
commands above are the constituent commands specified by the package's
`check` script and used the installed pinned dependencies.

Focused Python regression (`tests.test_sionna_worker`,
`tests.test_research_harness`, and `tests.test_ledger`) ran 117 tests: 115
passed and two were skipped for platform reasons. The ledger check passes
(5,396 source blocks, 438 explicit ID occurrences, 447 headings), as do
architecture-direction validation, source inventory (522 locked packages),
TypeScript schema syntax validation, and `git diff --check`.

The checked-in CPU proof remains historical worker-0.1.0 evidence. The pinned
worker-0.2.0 Sionna engine has not been run here; CPU numerical/runtime proof,
convergence, cancellation, memory isolation, remote artifact integrity, and
measured holdouts remain open.
