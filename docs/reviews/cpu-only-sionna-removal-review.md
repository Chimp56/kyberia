# CPU-only Sionna capability correction review

Date: 2026-09-23  
Implementation: `84a5bcb`  
Follow-up: `d0a88eb`

## Scope and verdict

Independent runtime and documentation/traceability reviews of this bounded
change both pass with no remaining findings. The change removes Sionna
accelerator selection/capability from the application and Kyberia Lab MCP,
while retaining generic browser `wgpu`/WebGL rendering. It does not complete a
roadmap phase or validate Sionna product quality.

The runtime review confirmed that retired accelerator aliases, including
`wgpu-cuda`, are rejected and plain `wgpu` remains accepted. Follow-up `d0a88eb`
uses a fresh valid configuration for the positive `wgpu` assertion, avoiding
state leaked from the test that intentionally mutates a coordinator private
key into an invalid configuration.

The documentation review initially requested changes for stale integration
status and validation claims without revision scope. Follow-up `d0a88eb`
corrects those claims and pins historical evidence separately from current
checks. The final re-review found no new documentation or ledger issues; the
3,336 total obligations are consistently recorded as 66 VALIDATED, 88
IN_PROGRESS, and 3,182 NOT_STARTED.

## Current evidence

- The direct commands comprising the Lab MCP `check` script pass with the
  installed pinned dependencies: Prettier, TypeScript no-emit/build, runner
  bundle build, package inventory, and SBOM/supply-chain validation.
- Lab MCP Node tests: 33 total, 32 passed, one Windows-only skip.
- Focused Sionna/research/ledger Python tests: 117 run, 115 passed, two
  platform skips.
- Ledger, architecture-direction and 522-package source-inventory checks pass.
- TypeScript schema syntax and `git diff --check` pass.

The pnpm wrapper itself was not run: Corepack attempted to fetch pnpm from the
npm registry and DNS/network access failed. The current constituent commands
were therefore run directly. No current full offline Rust workspace run or
pinned worker-0.2.0 Sionna engine execution is claimed. The older full Rust
result is author-reported and was not independently repeated. Numerical
convergence, cancellation, memory isolation, remote artifact integrity,
measured holdouts, authenticated Windows/Kismet hosts, and physical spectrum
validation remain open.

## Discovery limitations

The codebase-memory MCP tools were unavailable during this review, so the
review used the exact source, test, plan, ledger, status, and validation files.
No graph generation or graph-coverage claim is made.
