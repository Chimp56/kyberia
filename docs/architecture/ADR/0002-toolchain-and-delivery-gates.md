# ADR-0002: Toolchain baseline and evidence-gated technology decisions

Status: Accepted for initial engineering toolchain; renderer/storage/solver product decisions remain proposed.

## Context

Plan §10.2 recommends Rust, Tauri 2, React/TypeScript and Cargo/pnpm. §20 requires benchmark proofs before selecting renderer, storage, geometry and optimizer implementations. Baseline contained only plan.md and zero Git commits.

## Decision

Pin the available Rust 1.98.1 toolchain (edition 2024) and Node 24.20.0 LTS for initial engineering. Use Python 3.9-compatible standard-library tooling for source traceability and portable research fixtures. Proceed toward the recommended Cargo/pnpm/Tauri architecture without declaring gates B/C/E/F passed from scaffold code. Keep every native/runtime/field validation gate distinct from contract and synthetic validation.

## Alternatives

An all-Python product would abandon the recommended core performance and unit-safety strategy without evidence. Unpinned toolchains impede reproduction. Prematurely accepting all recommended libraries would bypass required prototypes.

## Evidence

Local probes on macOS 26.6.2 ARM64 report rustc/cargo 1.98.1, rustfmt/clippy, Swift 6.3.3 and Python 3.9.6. Node's official distribution index lists 24.20.0 as LTS; [official release guidance](https://nodejs.org/en/about/previous-releases) recommends supported LTS for production. Installation is verified separately from application behavior.

## Consequences

Builds pin toolchains and dependency lockfiles. Optional external engines have their own environments. Baseline has no existing tests to execute; new tests must contain executable assertions before claiming validation. Generated schemas, placeholder UI or fixture-only adapters do not complete product requirements.

## Reversibility

Toolchain upgrades are small commits with full affected checks. Renderer, storage and optimizer choices remain reversible ports until benchmark ADRs are accepted.

## Validation

Bootstrap/version checks, formatting, compiler/type checks, independent review, architecture tests and lockfile audits. Gate A–I decisions require the exact evidence in §20. Two-platform Phase 0 parity requires a second platform execution record.
