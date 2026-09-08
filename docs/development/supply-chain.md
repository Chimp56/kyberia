# Rust supply-chain checks

The developer commands cover the current executable Rust CLI dependency
closure rooted at `apps/cli/Cargo.toml`. They do not certify the desktop shell,
Python/Sionna environments, Kismet installation, or distributable artifacts;
those remain separate release work.

Run the pinned tools and retain all generated evidence locally:

```text
python3 tools/dev.py bootstrap
python3 tools/dev.py supply-chain-bootstrap
python3 tools/dev.py sbom
python3 tools/dev.py audit
```

The generic `bootstrap` command remains portable across CI hosts. The explicit
`supply-chain-bootstrap` command validates or downloads the currently pinned
`aarch64-apple-darwin` cargo-cyclonedx 0.5.9
and cargo-deny 0.20.2 archives using the SHA-256 pins in
`docs/licenses/supply-chain-tools.json`. It refuses to overwrite an existing
archive or install directory, verifies the installed executable bytes against
the archive member, checks the pinned tool version, and verifies the declared
license files. Tar extraction has explicit archive, member-count,
uncompressed-size, network-timeout, traversal, link, special-file, and unsafe-
permission bounds. Add a separately reviewed tool pin before using another
host target; a requested unsupported target fails explicitly.

Supply-chain bootstrap also installs six exact Python validator versions from
`tools/supply-chain/requirements.txt`, using hash-checked binary wheels. The
generic bootstrap does not require these optional tools. The vendored official
CycloneDX 1.5 Draft 7 schemas are pinned to specification commit
`595d98f16159bdf7463adc140509ded479130b8b`, including SPDX and JSF references.
Validation checks their bytes and resolves references entirely offline; no
network retrieval is allowed. This applies JSON Schema assertions, not optional
format extras, cryptographic signature validation or artifact authenticity.

The RustSec advisory database is a local Git checkout configured by `deny.toml`;
it is fetched only when absent. When its HEAD is older than P7D, run the
explicit network operation `python3 tools/dev.py supply-chain-refresh`; refresh
records and verifies the new Git revision and freshness without deleting or
rewriting the old checkout.

`sbom` runs the real CycloneDX plugin with `SOURCE_DATE_EPOCH` set to the Git
HEAD timestamp, `CARGO_NET_OFFLINE=true`, the requested SBOM target (defaulting
to the rustc host), JSON CycloneDX 1.5, all dependencies, and binary-only
description. The host tool target and requested SBOM target are resolved
separately. It asserts that Git HEAD, Cargo.lock, and every workspace
`Cargo.toml` hash remain unchanged across generation. It validates the tool
identity, non-empty target metadata, dependency references, and required BOM
structure, rewrites cargo-cyclonedx absolute workspace `path+file:` references
to the stable `path+file://workspace/...` namespace, and adds the Git revision,
all-workspace-manifest hash, CLI manifest hash, Cargo.lock hash, target, and
generation arguments as CycloneDX properties. The resulting evidence is
`.tools/supply-chain/evidence/kyberia_bin.cdx.json`. A rerun with unchanged
source and lockfile must produce the same canonical JSON bytes.

`audit` runs cargo-deny advisories, licenses, bans, and sources checks against
the CLI manifest with `--locked --offline`. It records the exact cargo-deny
version, RustSec Git revision and commit timestamp, lock/config hashes, and
each command's real stdout/stderr in
`.tools/supply-chain/evidence/cargo-deny-audit.json`. A missing or older-than-
P7D advisory checkout fails before the checks run. Evidence validation requires
all four unique checks with zero exit codes, the pinned cargo-deny version,
current Git/manifest/lock/config hashes, a bounded timestamp, and the current
RustSec revision. Dirty checkouts and implausible future timestamps fail. Git
HEAD, lockfile, workspace manifests, deny configuration and the advisory revision
are checked again after all four commands; changes make the audit fail. Audit
covers all target platforms under the pinned tool's default feature/dependency
selection; a supplied `--target` is rejected instead of silently ignored. Any check failure makes the command fail and evidence records
`result: failed`.

The strict bans policy rejects public path dependencies without explicit
versions. Internal workspace manifests therefore carry exact `0.1.0` versions;
adding broad wildcard exemptions would conceal packaging defects.

The pinned tool source and license records are in
`docs/licenses/supply-chain-tools.json`. It records each archive's actual
license files, redistribution boundary, transformation, provenance, and update
procedure. This is a developer-tool inventory, not the complete release SBOM
promised by Phase 0.

The schema/validator inventory is `docs/licenses/schema-validator-sources.json`.
RustSec's general CC0 dedication has an explicit CC-BY-4.0 exception for marked
GHSA-derived content; preserve those per-advisory licenses and attribution URLs
if exporting advisory text. Neither developer tools nor the database are
bundled into the current CLI.

Ordinary unit tests do not launch cargo-cyclonedx based on cache presence.
Set `KYBERIA_TEST_SUPPLY_CHAIN_RUNTIME=1` explicitly to run its real-tool test.
Optional schema tests run when the pinned validator is installed; other
provenance/parser tests run without it. Tests retain temporary artifacts.
