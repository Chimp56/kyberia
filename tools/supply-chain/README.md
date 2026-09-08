# Kyberia Rust supply-chain tools

`tools/supply_chain.py` owns the pinned developer workflow. Downloaded
archives, the RustSec advisory checkout, and generated SBOM/audit evidence are
kept under the ignored `.tools/supply-chain/` directory at the repository root.
The repository never stores tool binaries or generated release evidence.

The current pin is for `aarch64-apple-darwin`; add a separately reviewed
manifest entry before using another target. Archive extraction rejects absolute,
traversal, link, and special-file members and validates SHA-256 before any
executable is installed.
