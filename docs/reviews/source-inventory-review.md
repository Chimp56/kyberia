# Independent Cargo source inventory review

Reviewer: `/root/qa_spec_audit`. Author: `/root`.
Decision: **APPROVED for the selected locked source inventory tooling**.
No unresolved BLOCKER or MAJOR finding remains in this increment.

The primary HEAD during review was
`39d670a1c45ef5ba82794e7f31c0e3e0af5ea9ef`. Reviewed files were working-tree
changes; the hashes below identify their actual content independently of that
baseline. Review was read-only against primary source. Relevant requirements
are plan source/license provenance, dependency locking, reproducibility, and
release validation in §§14, 18–20 and 23 and the source audit appendices.

| File | SHA-256 |
|---|---|
| `tools/source_inventory.py` | `ccd023ea08f53eaa8fd48cef9d519faac90509d8de2789f9545bc1885189d54f` |
| `tests/test_source_inventory.py` | `73979ffa7b014181fb87e1a7c84325a603a40d9ecb53af6bcfa0480cf6a8e3c5` |
| `tools/requirements.txt` | `d4ead4c623d74f7cb35a46aba550ca25bc88a09147e6e0d46aece4651ea98447` |
| `docs/licenses/cargo-sources.json` | `f0b4e3e85481bff67db09d6df5129f6194e3d537288b0da468b0a7e72584be81` |
| `docs/licenses/SOURCE_LEDGER.md` | `1e7277dffecc54f38e26f08ac5ecaa81e20188c3b0b576cf04097f0771b636f8` |
| `README.md` | `fd0bd2e6791cf8cb18843a7619af57bf8beef77bec97d2fb4bafb07544aa71d3` |
| `Cargo.lock` | `8a48ed6da401c68de3b3629cb94d0893303685964704743547997118a9cc52ed` |

## Findings and corrections

**MAJOR SI-001 — Resolved.** The initial generator hashed a cached archive
without comparing it to its package checksum in Cargo.lock. A separate reviewer
fixture containing altered archive bytes and a different lock checksum was
accepted. The correction parses TOML with a mature parser, matches exact
name/version/source identities, and requires the archive digest to equal the
locked checksum. It also rejects duplicate identities and incomplete metadata
coverage. The original reviewer reproduction now raises the checksum error.

**MINOR SI-002 — Resolved.** The initial generator silently skipped all packages
whose source was null, including hypothetical external path dependencies. The
correction skips only actual workspace members and rejects other path packages
pending explicit provenance review. The original independent nonworkspace
fixture now raises the provenance error. No existing package was alleged to be
missing: all current path packages are workspace members.

The minimum supported Python uses the hash-pinned Tomli 2.4.1 universal wheel;
Python 3.11 and later use standard-library tomllib. Setup and source ledger
document this development dependency. No hand-written TOML parser was added.

## Independent execution

```text
.tools/venv/bin/python -m unittest discover -s tests -p test_source_inventory.py -v
PASS: 7 tests

.tools/venv/bin/python tools/source_inventory.py check
PASS: 86 locked external packages

Separate retained reviewer fixtures:
PASS: altered archive rejected against exact locked checksum
PASS: nonworkspace path dependency rejected
```

Tests cover matching archives, tampering, missing/different identities,
incomplete metadata, duplicate metadata, workspace/path distinction and missing
license declarations. The actual check independently reproduced the committed
inventory for all resolved target and development dependencies. No real Cargo
cache entries were modified by adversarial tests; temporary fixtures were
retained without recursive cleanup.

## Acceptance limits

This is a source inventory, explicitly not a target-specific shipped-binary
SBOM. It records upstream declared licenses and binds downloaded archive bytes
to the reviewed lockfile; this does not independently establish upstream
authenticity, redistribution clearance, or integrity of a maliciously edited
extracted build tree. Native bundled component notices still need packaging
review. Target-specific release SBOM generation, dependency vulnerability
audits, signing, reproducible distributions and non-Cargo dependency inventories
remain independent delivery work. Approval does not complete those requirements
or the complete source-audit runtime gates.
