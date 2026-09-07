# ADR-0006: Adopt audited Sionna RT behind a versioned process boundary

Status: Accepted after independent architecture review by `/root/sionna_rt`. The source disposition and process boundary are mandatory plan constraints; only the bounded CPU proof has passed runtime review. Full Gate I remains open.

## Context

Plan §§8.15, 16.13, Gate I and Appendix I adopt Sionna RT for high-fidelity propagation while reserving canonical geometry, identity, Wi-Fi semantics, uncertainty and provenance to Kyberia. Native Python/Mitsuba/Dr.Jit objects must not enter the domain or desktop. A failed requested engine must remain a visible failure, and generic multi-transmitter SINR must not be presented as Wi-Fi SINR.

The source audit pins Sionna RT 2.0.1 at `bc0549155c7b782c7614a0ec06a0ac4e32b979ae`. A same-version PyPI distribution differs in four material source files. Version text alone therefore cannot establish the audited implementation's identity.

## Decision

Keep Sionna inside an optional, isolated worker launched by a trusted outer adapter. Use explicit versioned JSON requests/results containing only bounded, unit-labeled values and engine-neutral identifiers. Validate both directions; require request, scene, solver profile and runtime source/build provenance to match. Exact source, dependency locks and numerical tolerances are part of the execution identity.

The current research implementation uses one process per request, a trusted interpreter/worker path, bounded input/result/log streams, cancellation, timeout and explicit crash/absence errors. It accepts an explicit seed. It returns linear path gain, complex path coefficients/delays and radio-map values with masks; Kyberia owns subsequent Wi-Fi channel overlap, association, airtime, contention, interference and capacity. It does not expose or use Sionna's generic SINR.

Preserve the audited source build and its inventory independently from core dependencies. Do not silently substitute P0/P1 after a Sionna failure. Reject unsupported scene/interaction/backend requests with explicit capability information. The current empty-space CPU proof is labeled accordingly; it does not satisfy P2/P3 or material/antenna/diffraction requirements.

## Alternatives

Loading Python/native runtime state inside the domain or desktop violates the mandated boundary and couples UI reliability to native failures. Building a competing full electromagnetic tracer contradicts the adopted disposition without a future evidence-backed ADR. Depending only on a package version fails to distinguish the observed source discrepancy. A persistent worker pool may reduce startup overhead later, but requires measured lifecycle, isolation, cache invalidation and resource-budget evidence before replacing the current request process lifecycle.

## Evidence

The [adapter procedure](../../adapters/sionna.md), [source inventory](../../licenses/sionna-sources.json) and [independent CPU review](../../reviews/sionna-cpu-review.md) preserve the exact source discrepancy, reproducible audited wheel and all 49 package source hashes. The installed proof uses Python 3.12.12, Mitsuba 3.8.0, Dr.Jit 1.3.1 and LLVM 18.1.8 on macOS ARM64. It passes 34 author CPU checks, 17 contract/lifecycle tests and 19 independently rerun upstream subset tests. Independent asymmetric path and area-integrated map checks verify axis attribution and numerical meaning. This evidence concerns the explicitly restricted empty-space solver profile only.

## Consequences

The desktop/core can remain usable when the optional runtime is absent or fails. Native-engine output is evidence requiring canonical normalization, not domain truth. Exact-source and dependency maintenance increase release work but make execution provenance inspectable. One process per request has startup cost; performance evidence must guide any pool. Process separation alone does not provide a hard memory limit, filesystem/network sandbox, distribution clearance or measured RF accuracy.

Full scene compilation, materials, antennas/orientation effects, reflection, transmission, diffuse scattering, diffraction, GPU execution, cache artifacts, sustained-kernel cancellation, OOM containment and measured holdouts remain implementation/validation work. Native GPU hardware execution is a distinct gate from writing the surrounding contracts and CPU implementation.

## Reversibility

Version the worker protocol independently of canonical observations and projects. Adapter/runtime replacements preserve canonical evidence and explicit engine identity. A later worker pool or schema version requires compatibility fixtures and migration strategy; changing the adopted engine disposition requires an evidence-backed ADR that preserves the product requirement.

## Validation plan

Keep malformed-request/result, provenance, deterministic-seed/tolerance, timeout, cancellation and crash tests in ordinary CI without requiring the optional engine. Run fresh-environment source verification and actual CPU acceptance with pinned dependencies. Expand actual solver profiles only alongside independent numerical scenes and supported-interaction tests. Validate GPU on compatible hardware separately, and retain failed convergence evidence. Before declaring Gate I complete, run every normative runtime item, complete measured comparison and release isolation/license/SBOM checks; the current proof is insufficient for that declaration.
