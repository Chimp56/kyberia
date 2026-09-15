# External dependency register

No implemented capability is yet blocked. Toolchain installation, missing code, unrun tests, and roadmap size are executable work, not external blockers.

The current host is macOS 26.6.2 ARM64. The following are **anticipated validation dependencies**, not a declaration that the surrounding capabilities are implemented or blocked:

| Requirement/gate | External dependency to establish | Surrounding executable work | Validation once available |
|---|---|---|---|
| §16.13 Kismet live local/remote capture | Supported Linux radios and authorized sensor access | Offline importers, authentication/version contracts, replay, malformed-input and reconnect tests | Run pinned Kismet; compare the same controlled beacons through API, KismetDB and PCAPNG with source/dwell/drop provenance |
| §10.10/§17 platform capability proof | Representative Windows/Linux/Android/iOS devices and permissions | Native collectors, fixture contracts, capability probes and honest unsupported states | Execute platform probe suite on each actual OS/device/driver and retain evidence |
| Appendix I OPS-004 Lab MCP Windows containment | Windows host capable of executing and observing Job Objects | Strict signed coordinator/runner contracts, fixed Windows operation catalog, Job Object helper, injection tests and exact hosted procedure | Build the Lab MCP on hosted Windows, invoke the fixed helper through a pinned runner, cancel a descendant-producing fixture, prove the descendant exits, and verify the signed cancelled manifest |
| §16.6–16.12/Phase 7 measured holdouts | Calibrated RF equipment, controlled physical sites and legitimate competitor licenses | Synthetic fixtures, experiment protocol, field ingestion and holdout analysis | Execute documented lab/route protocols with calibration and software provenance |
| §15.10 release signing and licensed catalogs/SDKs | Signing credentials and reviewed redistribution rights | Reproducible unsigned packaging, SBOM, source ledger, isolated optional adapters | Audit licenses and sign artifacts using authorized release credentials |

Promotion to `BLOCKED_EXTERNAL` requires an exact unresolved subrequirement, recorded probe/evidence, implemented surrounding interfaces and contract tests, and an executable resumption procedure. A blocked runtime test never implies product validation.

During local implementation, automatic command review rejected wiring/executing the fixed Python Windows containment helper because it conservatively classified any helper-mediated process execution as an arbitrary-command path. The checked-in helper itself accepts only a closed suite/selector catalog and no executable, argv, path or environment input. The coordinator remains fail-closed on Windows until independent review approves integration; the exact Windows execution gate above requires a real Windows host in any case.
