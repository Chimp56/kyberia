# Architecture decisions

Each accepted ADR records context, decision, alternatives, evidence, consequences, reversibility and validation. Proposed decisions stay proposed until their evidence gate passes.

| ADR | Status | Decision and specification gate |
|---|---|---|
| [0001](0001-canonical-boundaries.md) | Accepted specification constraint | Local-first modular monolith, canonical Rust domain and versioned outer adapters (§10/§14, Appendix I) |
| [0002](0002-toolchain-and-delivery-gates.md) | Accepted toolchain baseline; product gates pending | Rust/Cargo and TypeScript/pnpm; retain gates A–I and isolate platform validation |

The plan repeats ADR-012 through ADR-016 for distinct subjects. Repository ADR numbers are unique; source proposal aliases will be mapped with their section and occurrence. No repeated proposal is silently discarded.

Pending durable decisions: native capture viability (A), renderer (B), storage split (C), active integration (D), geometry kernel (E), optimizer (F), licensing/distribution (G), Kismet runtime boundary (H), Sionna adopted worker runtime (I), WASM plugin runtime, predictive tiers, requirements, and deterministic diagnosis.
