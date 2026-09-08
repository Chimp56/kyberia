# Independent supply-chain/platform review

Review scope: the frozen `supply-final-freeze.json` in
`/Users/vincent/code/kyberia/.worktrees/supply-chain-fix`, focused on SC-R001
through SC-R005. The author worktree was not modified. The reviewed source
worktree was at `72a39d859e25141dab45c9813d5e0ea0def62f9c`; the older author
worktree was at `20b309e91b2a049dac8c89e0ab980e994425639e`; main was at
`cddd37b8238f512f45b5aec241ff7aa22ea09a64`.

The 15 frozen-file hashes were verified against the freeze manifest and are
retained in [supply-final-freeze-hashes.json](supply-final-freeze-hashes.json).
The copied review tree used for independent tests is
`/Users/vincent/code/kyberia/.worktrees/manifest-luna-review/.review/supply-final-freeze`.

## Findings

**MINOR SC-R005-001 — the source/license ledger uses a stale “current” RustSec revision.**

`docs/licenses/SOURCE_LEDGER.md:46` says the current 97-package graph was
audited against `faedffd5118c1835e13cca3babb6059afb1eb8d0`. The frozen runtime
evidence, the clean advisory checkout, and the validated audit evidence use
`8a1eb4f933fb5821add5b4e98601ebd90b8b3538`. The runtime is internally
consistent: all four checks return zero and `validate_audit_evidence` accepts
the current revision. Clarify the ledger sentence as historical or update it
to `8a1eb4f...` before treating the ledger as the current audit record.

**NIT SC-R003-001 — optional validator metadata is package-level rather than wheel-level.**

`docs/licenses/schema-validator-sources.json` records all six optional
validator packages, versions, licenses, redistribution boundaries and PyPI
provenance. `tools/supply-chain/requirements.txt` supplies the required hashes,
and the local selected-wheel metadata was used for the offline test. The
checked-in JSON does not enumerate each selected wheel filename, platform tag
and hash. This is acceptable for the explicitly unbundled developer-tool
scope, but retain it as metadata coverage debt if a developer bundle or
release notice package is added.

**NIT SC-PLAT-001 — standalone supply-chain bootstrap relies on the generic bootstrap for Python 3.9 Tomli.**

`tools/dev.py:64-66` installs the six schema-validator packages, while
`tools/supply_chain.py:28-34` needs `tomli` when the interpreter is before
3.11. `tomli` is pinned by the generic `tools/requirements.txt`, and the
documented command sequence runs generic `bootstrap` first, so the documented
path is portable. A manually pre-created `.tools/venv` on Python 3.9 that runs
only `supply-chain-bootstrap` lacks that parser. Either keep the documented
ordering explicit or add the conditional Tomli pin to the supply-chain
bootstrap requirements.

The 15-file freeze is not a complete standalone source tree for one existing
test: `test_audit_rejects_changes_during_tool_execution` still hashes the
baseline `apps/cli/Cargo.toml`. The independent copy included that unchanged
baseline file for execution. This is a review-fixture limitation, not a
runtime finding.

There are no BLOCKER or MAJOR findings. The NITs are tooling/documentation
coverage debt. The MINOR ledger correction should be made before publishing
the audit record; it does not invalidate the code guards or the fresh runtime
evidence.

## Verified licensing and provenance

The actual pre-existing archives matched the manifest pins:

- cargo-cyclonedx 0.5.9, Apache-2.0, archive SHA-256
  `4c53dfa21e70b65bf7f8d2592aadde3bcb02c1a40b6ec63b877e5ca65a29e180`, with
  `cargo-cyclonedx-aarch64-apple-darwin/LICENSE` present and nonempty.
- cargo-deny 0.20.2, Apache-2.0 OR MIT, archive SHA-256
  `fe67d82a10d8597a3549364cb733a3f9cc1bfff9031b7ae46384a9f2a72090c3`, with
  both `LICENSE-APACHE` and `LICENSE-MIT` present and nonempty.

The three vendored CycloneDX schemas are pinned to official commit
`595d98f16159bdf7463adc140509ded479130b8b`; the schema and Apache notice
hashes are recorded in the frozen source manifest and match the copied files.
The six optional validator pins are jsonschema 4.25.1 (MIT), attrs 25.3.0
(MIT), jsonschema-specifications 2025.9.1 (MIT), referencing 0.36.2 (MIT),
rpds-py 0.27.1 (MIT), and typing-extensions 4.15.0 (PSF-2.0). The docs mark
all six as tooling-only and unbundled.

The RustSec checkout was clean, fresh and at
`8a1eb4f933fb5821add5b4e98601ebd90b8b3538`; the ledger retains the CC0-1.0
license and CC-BY-4.0 attribution exception for marked GHSA records.

## Commands and results

All commands used explicit worktrees. No network download, live RF test,
recursive deletion, destructive Git operation, source edit or dependency
installation into the source worktrees was performed.

1. Freeze verification: the programmatic SHA-256 comparison against
   `.tools/supply-final-freeze.json` returned `freeze PASS`; the copied
   15-file tree also returned `copied freeze PASS`.
2. Offline validator setup used the retained local wheels only:
   `pip install --no-index --only-binary=:all: --require-hashes --target
   .../.review/validator-site --find-links .../.tools/supply-chain/wheels
   -r tools/supply-chain/requirements.txt` — all six packages installed.
3. Copied-tree focused suite:
   `PYTHONPATH=.../.review/validator-site /Users/vincent/code/kyberia/.tools/venv/bin/python -m unittest -v tests/test_supply_chain.py`
   — **20 tests, 19 passed, 1 explicit runtime skip**.
4. `python3 tools/source_inventory.py check` — **PASS: 97 locked external
   packages**.
5. The three frozen Python files compiled with a review-local
   `PYTHONPYCACHEPREFIX` — pass.
6. `ensure_tool` independently verified both installed executables against
   their pinned archive member bytes, license files and version output — pass.
7. `validate_cyclonedx_schema` with `urllib.request.urlopen` patched to fail
   — pass offline; the positive, invalid-component, local-SPDX-reference and
   tampered-schema tests all passed.
8. `validate_sbom` on the retained evidence — **PASS**, 45 components and 46
   dependency nodes. `validate_audit_evidence` — **PASS**, RustSec
   `8a1eb4f...`, advisories/licenses/bans/sources all return code 0.
9. The real pinned cargo-deny `check bans` command returned summary errors 0
   (four duplicate-version warnings); the retained evidence records all four
   audit groups as zero.

The runtime evidence reports official Draft 7 schema validation and identical
repeat SBOM bytes. It explicitly limits the claim to the ARM64 macOS Rust CLI
tooling closure and does not claim Python/Sionna/Kismet/controller coverage or
a complete distributable release SBOM.

## Recommendation

**Conditional approval of the frozen supply-chain code and tooling boundary;
no unresolved major security or licensing finding.** Correct or explicitly
reframe the single stale RustSec sentence in `SOURCE_LEDGER.md` before using
the ledger as release evidence. The optional wheel metadata and standalone
Python 3.9 bootstrap behavior can remain documented debt under the current
developer-only, unbundled scope.

Integration resolution: the stale source-ledger sentence is explicitly historical. The later runtime evidence and inventory retain the actual 8a1eb4f… advisory revision. Generic bootstrap ordering and wheel-level metadata limits remain documented.
