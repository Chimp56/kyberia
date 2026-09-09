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
| [0007](0007-active-process.md) | Accepted bounded process boundary; full Gate D open | Pin iperf3 behind explicit topology, strict result semantics and bounded lifecycle; eight macOS loopback checks passed |
| [0008](0008-renderer.md) | Accepted bounded research evidence; final Gate B decision open | Compare pinned OpenLayers and custom WebGL2 paths on one source-bound synthetic workload without promoting either to the product renderer |
| [0009](0009-analysis-manifest.md) | Accepted; execution integration open | Immutable spatial inputs and typed canonical manifest identity with pinned JSON encoding and independently verified hashes |
| [0010](0010-portable-2d-geometry-boundary.md) | Accepted provisional proposal; production Gate E open | Portable 2-D Rust geometry boundary with bounded import, floor filtering, validation, and explicit repair provenance |
| [0011](0011-explicit-wifi-signal-aggregation.md) | Accepted bounded numerical contract; PAS-001 integration open | Explicit versioned aggregation, linear-power arithmetic, evidence-strict SNR/SIR/SINR, and deterministic provenance |
| [0012](0012-receipt-point-association.md) | Accepted bounded contract; integration open | Keep receipt-based point association separate from strict capture admission; preserve a separate event-ordering watermark |
| [0013](0013-spatial-signal-aggregation-boundary.md) | Accepted bounded numerical boundary; integration open | Spatial coincident groups consume typed Wi-Fi aggregation through verified metric-definition bytes; temporal methods require monotonic evidence |
| [0014](0014-transactional-survey-snapshots.md) | Accepted bounded snapshot boundary; broader storage gates open | Persist immutable, checksummed survey snapshots with transactional history and verified replay |
| [0015](0015-wifi-channel-coupling.md) | Accepted bounded numerical contract; full PHY/MAC integration open | Versioned channel geometry, spectral coupling, explicit utilization evidence, canonical-radio deduplication, and same-BSS policy |
| [0016](0016-operation-log.md) | Accepted for Phase 0 contract work; integration open | Canonical immutable typed operation DAG with deterministic replay, bounded offline merge, explicit conflicts, and auditable undo/redo |
| [0017](0017-metric-registry.md) | Accepted bounded Phase 0 contract; broader analysis integration open | One canonical bounded metric definition for computation, units, UI help, exports, unknown policy, and dimensional checks |
| [0018](0018-native-observation-pipeline.md) | Accepted bounded composition increment; native runtime and co-transaction gates open | Connect native normalized observations to receipt association and durable observation chunks through a versioned composition port |
| [0019](0019-kismet-live-status.md) | Accepted bounded adapter contract; Kismet runtime gate open | Authenticated read-only status/capability projection with bounded transport, deterministic receipts, and no observation/control leakage |
| [0020](0020-operation-inverse-v2.md) | Accepted bounded contract; materializer integration open | Versioned inverse metadata preserves typed calibration priors and explicitly marks irreversible floor-evidence binding while retaining V1 bytes and hashes |
| [0021](0021-materialization-input-identity.md) | Accepted bounded prerequisite; materializer integration open | Domain-separated canonical identities for validated project baselines and exact operation sets |
| [0022](0022-causal-project-materialization.md) | Proposed bounded application contract; independent review and durable project publication remain open | Causal inverse validation and typed operation effects applied through canonical Project invariants |
| [0023](0023-materialized-project-publication.md) | Accepted bounded adapter; product and cross-platform runtime gates open | Explicit baseline registration and transactional, replay-validated project publication |
| [0024](0024-shared-materialization-budget.md) | Accepted bounded contract; storage transaction adoption reviewed | Caller-owned cumulative work and copy accounting across operation admission, replay, identities and materialization |

The plan repeats ADR-012 through ADR-016 for distinct subjects. Repository ADR numbers are unique; source proposal aliases will be mapped with their section and occurrence. No repeated proposal is silently discarded.

Pending durable decisions: native capture viability (A), renderer (B), storage split (C), active integration (D), geometry kernel (E), optimizer (F), licensing/distribution (G), Kismet runtime boundary (H), Sionna adopted worker runtime (I), WASM plugin runtime, predictive tiers, requirements, and deterministic diagnosis.
