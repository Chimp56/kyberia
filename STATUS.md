# Kyberia implementation status

The authoritative specification is [plan.md](plan.md). This is an active implementation, not a completed product.

- Current phase: **Phase 0 — Research harness and architecture proof**.
- Current iteration: **1 — Canonical contracts, traceability, and reproducible toolchain**.
- Completed foundation increments: independently reviewed lossless source ledger and execution DAG; unit-safe IDs/physical values, explicit unknowns, timing, covariance and observation contracts; 24 original scientific scenes, 25-point deterministic survey, clean-room TIN oracle and runtime evidence checker. These do not constitute a usable survey application or completion of Phase 0.
- In progress: canonical project hierarchy/calibration/commands; real macOS collector process; reproducible automation and remaining Phase 0 proofs. Initial project-store/CLI and source-inventory reviews are approved. Every agent has an isolated worktree within `.worktrees/`.
- Blocked capabilities: no whole capability is classified blocked. Specific unavailable hardware/OS/credential validation gates will be recorded separately in [BLOCKERS.md](docs/implementation/BLOCKERS.md).
- Validation status: 18 domain tests, 13 project-store tests, two executable CLI workflows and seven compile-fail doctests pass on macOS ARM64; formatting and Clippy pass. All 70 Python tests pass (31 ledger, 32 research, seven source inventory), and the source checker verifies 86 locked external packages. Native/runtime/field gates remain separate and open.
- Latest reviewed commit: `5700ec6` research fixtures and evidence gates, with independent acceptance recorded in `39d670a`. Canonical domain `de50f5f` was independently approved after correcting covariance admission.
- Known technical debt: all product implementation remains open; renderer, storage, capture, geometry, optimizer and integration runtime gates are unresolved. Source IDs collide between sections and must never be used unqualified.
- Next executable work: commit reviewed transactional storage/CLI and tooling baseline; complete macOS capability probe and project/calibration graph; implement offline Kismet normalization, analytical-storage proof and Sionna CPU worker gates.

Primary integration branch: `main`. Agents edit isolated worktrees. Meaningful changes require a separate review before integration; no feature may be promoted on author tests alone.
