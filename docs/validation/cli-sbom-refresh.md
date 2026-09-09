# Integrated CLI SBOM refresh

At source revision `93df487a553ec89df1b1ecfbe6b262d7a6b2d9e7`, command
`.tools/venv/bin/python tools/supply_chain.py sbom` passed using pinned
cargo-cyclonedx 0.5.9 and the pinned CycloneDX schema validator for
`aarch64-apple-darwin`. The generated CLI binary closure contains 86 components
and 87 dependency nodes. It includes the integrated stored-analysis CLI changes.

Artifact: `.tools/supply-chain/evidence/kyberia_bin.cdx.json`.
SHA-256: `de8e0256e0ce453fc2cc80d06c53609510a7bc9799b6aac25de0d03bbec44c27`.
The generator binds exact lockfile, workspace manifest, CLI manifest and source
revision metadata and validates the resulting document. This record does not
claim repeat-generation determinism for this particular refresh.

Previous SBOM files were moved to
`.trash/sbom-history/pre-refresh-29id1b35` with original-path metadata, for manual
retention or disposal. No recursive deletion was performed. An initial command
using system Python lacked the schema-validator dependency and failed; the
pinned repository venv run above is the successful evidence.

This is the Rust CLI closure, not a complete application distribution SBOM.
Geometry is currently a separate workspace library and its dependencies remain
covered by the 241-package workspace inventory and dependency audit. Python,
external workers, native collectors and final packaging require their own
distribution coverage before release acceptance.

## Repeat-generation check

At `6f8f21c`, root invoked `supply_chain.generate_sbom()` twice using the pinned
repository Python venv and default current-host target. Both generations passed
schema/source validation and their normalized bytes were identical. Each contains
86 components. SHA-256:
`47674f0989a6b8b80922808ac9457122bfc0fb780594e2ebea7650bfd476f6c9`.

The prior revision's artifact is retained under
`.trash/sbom-history/determinism-ze44uh61`; the first identical run is retained
under `.trash/sbom-history/determinism-awydivhb`. The second is in the standard
ignored evidence location. Original-path manifests accompany the archives.
This proves repeatability for this source revision, dependency closure, pinned
tool and target; it does not establish reproducibility across other targets.
