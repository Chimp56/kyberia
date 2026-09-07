# Architecture decisions

Each accepted ADR records context, decision, alternatives, evidence, consequences, reversibility and validation. Proposed decisions stay proposed until their evidence gate passes.

| ADR | Status | Decision and specification gate |
|---|---|---|
| [0001](0001-canonical-boundaries.md) | Accepted specification constraint | Local-first modular monolith, canonical Rust domain and versioned outer adapters (§10/§14, Appendix I) |
| [0002](0002-toolchain-and-delivery-gates.md) | Accepted toolchain baseline; product gates pending | Rust/Cargo and TypeScript/pnpm; retain gates A–I and isolate platform validation |
| [0003](0003-versioned-observation-migration.md) | Accepted | Independently version observations and survey snapshots; preserve immutable V1 migration fixtures |
| [0004](0004-storage-split.md) | Proposed; research comparison reviewed | SQLite authority with immutable analytical chunks; production Gate C remains open |
| [0005](0005-source-response-timing.md) | Accepted; native normalization integrated | Preserve source result/API timing separately from RF capture, cache age and channel dwell; storage/survey composition remains open |
| [0006](0006-sionna-worker-boundary.md) | Accepted boundary; full Gate I open | Adopt exact-source Sionna RT behind versioned, validating process contracts; bounded CPU proof independently reviewed |
| [0009](0009-analysis-manifest.md) | Accepted; execution integration open | Immutable spatial inputs and typed canonical manifest identity with pinned JSON encoding and independently verified hashes |

The plan repeats ADR-012 through ADR-016 for distinct subjects. Repository ADR numbers are unique; source proposal aliases will be mapped with their section and occurrence. No repeated proposal is silently discarded.

Pending durable decisions: native capture viability (A), renderer (B), storage split (C), active integration (D), geometry kernel (E), optimizer (F), licensing/distribution (G), Kismet runtime boundary (H), Sionna adopted worker runtime (I), WASM plugin runtime, predictive tiers, requirements, and deterministic diagnosis.
