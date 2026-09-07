# Kyberia implementation status

The authoritative specification is [plan.md](plan.md). This is an active implementation, not a completed product.

- Current phase: **Phase 0 — Research harness and architecture proof**.
- Current iteration: **2 — Project calibration, native capture proof, and automated validation**.
- Completed foundation increments: independently reviewed lossless source ledger and execution DAG; unit-safe IDs/physical values, explicit unknowns, timing, covariance and observation contracts; 24 original scientific scenes, 25-point deterministic survey, clean-room TIN oracle and runtime evidence checker. These do not constitute a usable survey application or completion of Phase 0.
- In progress: real macOS collector process; dependency-direction CI and developer commands; point-survey state machine; remaining Phase 0 proofs. Initial project-store/CLI, source inventory and project/calibration contracts are independently approved. Every agent has an isolated worktree within `.worktrees/`.
- Blocked capabilities: no whole capability is classified blocked. Specific unavailable hardware/OS/credential validation gates will be recorded separately in [BLOCKERS.md](docs/implementation/BLOCKERS.md).
- Validation status: 32 domain tests, 13 project-store tests, two executable CLI workflows and seven compile-fail doctests pass on macOS ARM64; formatting and Clippy pass. All 70 Python baseline tests pass (31 ledger, 32 research, seven source inventory), and the source checker verifies 86 locked external packages. Five new architecture checks are under review. Native/runtime/field gates remain separate and open.
- Latest reviewed commit: `364ff2d` project/calibration contracts, with independent acceptance recorded in `1cf2395`. Storage/source baseline `1d703b0` has independent acceptance in `e854bae`.
- Known technical debt: full editor undo/merge and large-history replay remain open; metadata operation replay currently grows quadratically. Renderer, storage, capture, geometry, optimizer and integration runtime gates are unresolved. Source IDs collide between sections and must never be used unqualified.
- Next executable work: validate native macOS collector and point-survey contracts; complete developer/CI automation, offline Kismet normalization, analytical-storage proof and Sionna CPU worker gates.

Primary integration branch: `main`. Agents edit isolated worktrees. Meaningful changes require a separate review before integration; no feature may be promoted on author tests alone.
