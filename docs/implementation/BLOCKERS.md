# External dependency register

No implemented capability is yet blocked. Toolchain installation, missing code, unrun tests, and roadmap size are executable work, not external blockers.

The current host is macOS 26.6.2 ARM64. The following are **anticipated validation dependencies**, not a declaration that the surrounding capabilities are implemented or blocked:

| Requirement/gate | External dependency to establish | Surrounding executable work | Validation once available |
|---|---|---|---|
| §16.13 Kismet live local/remote capture | Supported Linux radios and authorized sensor access | Offline importers, authentication/version contracts, replay, malformed-input and reconnect tests | Run pinned Kismet; compare the same controlled beacons through API, KismetDB and PCAPNG with source/dwell/drop provenance |
| §16.13 Sionna CUDA | Compatible NVIDIA GPU/backend | CPU installation/runtime, process contracts, deterministic scenes, cancellation and artifact tests | Run the same canonical jobs on CUDA; record versions and compare within established tolerances |
| §10.10/§17 platform capability proof | Representative Windows/Linux/Android/iOS devices and permissions | Native collectors, fixture contracts, capability probes and honest unsupported states | Execute platform probe suite on each actual OS/device/driver and retain evidence |
| §16.6–16.12/Phase 7 measured holdouts | Calibrated RF equipment, controlled physical sites and legitimate competitor licenses | Synthetic fixtures, experiment protocol, field ingestion and holdout analysis | Execute documented lab/route protocols with calibration and software provenance |
| §15.10 release signing and licensed catalogs/SDKs | Signing credentials and reviewed redistribution rights | Reproducible unsigned packaging, SBOM, source ledger, isolated optional adapters | Audit licenses and sign artifacts using authorized release credentials |

Promotion to `BLOCKED_EXTERNAL` requires an exact unresolved subrequirement, recorded probe/evidence, implemented surrounding interfaces and contract tests, and an executable resumption procedure. A blocked runtime test never implies product validation.
