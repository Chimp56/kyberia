# Iteration 3: bounded packet ingestion and spatial numerics

This records local software validation, not completion of Phase 0 or a usable survey application.

`python3 tools/dev.py check` passed on macOS 26.6.2 ARM64 with Rust 1.98.1 after integrating spatial numerical code and correcting PCAPNG EOF cancellation. It runs formatting, Clippy with warnings denied, workspace typechecking, architecture checks, 124 Rust integration/property tests, seven compile-fail doctests, and Python discovery (96 tests: 91 passed and five opt-in native tests skipped). It also checks all 90 locked external packages, all 5392 plan source blocks and generated original scientific fixtures. Four explicit Rust benchmarks are excluded from the default suite and have separate execution evidence.

## Numerical spatial scope

`cargo test --locked -p kyberia-spatial-analysis` passes 16 tests including translation/permutation and convex-bound properties. Pure nearest/IDW computation preserves value, mask, support and unknown uncertainty separately. Inputs retain evidence plane, floor/frame, observation IDs and deterministic policy. Sample gaps remain unknown outside the explicit support radius. Radius support is not a convex-hull or barrier model. Unknown uncertainty is not a numerical confidence estimate.

The [independent review](../reviews/spatial-baseline-review.md) includes six additional probes and source hashes. A fully occupied 100-million-distance-evaluation job with 64 retained neighbors took 5040.383 ms; UI integration must run jobs away from the event loop and use smaller viewport budgets. The backend nearest/IDW synthetic-baseline requirement is validated. Tile storage/import, spatial indexing, TIN, barriers, estimated uncertainty and frontend display remain open.

## Packet container scope

`cargo test --locked -p kyberia-packet-import` passes 17 tests after reproducing and correcting the independent review's final-EOF cancellation/deadline defect. Both new regressions failed before the fix by receiving an incorrect successful receipt. Read completion and final receipt publication now recheck cancellation and deadline. This remains cooperative interruption; a blocked kernel read needs a supervising process.

The adapter uses pinned pcap-parser 0.17.0 for block parsing and adds complete bounded framing/options validation. No raw packet bytes are persisted automatically, and a consumer must stage a delivered prefix until the final receipt succeeds. Legacy PCAP, Radiotap/802.11 and canonical observation normalization remain open. This increment does not validate aggregate Kismet replay or live capture gates.

Separate release benchmarks include parsing and SHA-256 over original generated packet streams; the independent final run measured 10,000 packets in 10.378 ms and 100,000 in 103.334 ms. The spatial and packet baselines are local measurements, not portable CI performance promises.
