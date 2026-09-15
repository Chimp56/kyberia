# ADR 0043: CPU/LLVM-only Sionna worker

Status: Accepted product-scope decision; implementation candidate requires independent review

## Context

The original plan and worker capability response carried a future accelerator
path alongside the proven LLVM path. That path added a hardware-specific suite,
device selector, external blocker and backend-variance obligation without being
required for the local-first product. The desktop renderer's portable
WebGL/`wgpu` acceleration is a separate subsystem and remains in scope.

## Decision

RF Atlas supports Sionna only through the pinned
`llvm_ad_mono_polarized` CPU backend, locally or on authenticated remote/HPC
CPU workers. The application, Lab MCP, acceptance catalog and active
documentation do not select, advertise, require or gate an accelerator backend.
Former `cuda`, `gpu`, `sionna-gpu`, and device-selector inputs fail closed; they
are never silently translated to CPU.

The worker transport remains schema version 1 because request framing, job
semantics and result arrays are unchanged. Its capability document is bumped to
version 2 and worker version 0.2.0 because removing a capability field is an
incompatible capability-contract change. Active validation requires the exact
CPU-only field set. An explicit `allow_legacy` path exists solely to validate
immutable worker-0.1 evidence; it is not used for new execution.

Lab coordinator configuration is bumped to schema version 2, its server version
to 0.2.0, and `run_sionna_gate` now accepts only `host` and `scene_set`.
Configuration containing the retired suite and calls containing the retired
device selector are rejected by strict schemas.

## Consequences

Sionna installation and runtime validation need only CPU/LLVM artifacts. Missing
accelerator hardware is no longer an external dependency or roadmap blocker.
CPU convergence, representative performance, cancellation, memory isolation,
remote artifact integrity and measured holdouts remain required. Generic
WebGL/`wgpu` renderer work and CPU/reference differential testing are unchanged.

Historical reviews and evidence retain their original fields and wording so
their hashes and truthfulness are preserved. This ADR and plan version 0.3
supersede their product-scope implications.

## Reversibility and validation

Reintroducing another Sionna backend requires a new accepted ADR, a new
capability-schema version, explicit user-facing value, dependency/SBOM review,
resource-isolation analysis and independent numerical/runtime evidence. The
current decision is validated by exact capability-shape tests, former-selector
rejection tests, catalog absence checks, Lab MCP schema/E2E tests, CPU worker
tests and repository-wide literal audits that exempt only immutable historical
artifacts and explicit rejection fixtures.
