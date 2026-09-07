# Independent developer tooling review

Reviewer: `/root/qa_spec_audit`. Author: `/root`.
Decision: **APPROVED for initial Cargo/CLI developer tooling and declared CI**.
No unresolved BLOCKER or MAJOR finding. The MINOR line-ending finding below was
corrected before approval. Relevant plan requirements include OSS-012 dependency
lints, reproducible developer setup, dependency direction, and CI validation.

Review was read-only against primary, whose HEAD was
`1cf23951deaf6e8a342233db8bca33d24e7259df`. The following working-tree hashes
identify the reviewed content; new files are not attributed to that baseline.

| File | SHA-256 |
|---|---|
| `tools/architecture.py` | `9e45fb737c49e824e1785210c41aaeb55747a2a12370aa2799cafd4b9aee9529` |
| `tools/architecture.json` | `6edc027de00fed07615c02a3a7cb361f6c79dd477a2fa1fabc77f1a0bcfbc224` |
| `tests/test_architecture.py` | `eb43e7d4cd22b026527152121a1be3a5a2a9bd6ec3d72ae55a80a0548f496277` |
| `tools/dev.py` | `21a631eaf304fda3a06a91164a6dac26f77c3640d989cf4a38521bf8a5cb37ef` |
| `.github/workflows/ci.yml` | `84429e0292d86d82aadc613cb702664110fe2f81d83f8253dec2e20ec77a6c1a` |
| `docs/architecture/dependency-policy.md` | `9f1a4dfb6123de4013484250d35d85a979cb26215e468ed2e91a2d78825529fd` |
| `README.md` | `95ba04382273946246d07da5d364b9a9646405ac1d239bf9bf33b7343326f57d` |
| `docs/licenses/SOURCE_LEDGER.md` | `1fd8b4eaabce2601c3b35816e76fdc0b8ccf8a867b2b8dad96c5c8072d405929` |
| `.gitattributes` | `9d34089b39381a0c53cc51515d393e9739f3dcdc26b4a4bdec0eb8e76bcf7bf8` |

## Findings and correctness assessment

**MINOR DT-001 — Resolved.** Raw source/checksum evidence could vary on Windows
checkouts with `core.autocrlf=true` because the repository lacked a text EOL
policy. The author added `* text=auto eol=lf`. Independent `git -c
core.autocrlf=true check-attr text eol -- plan.md Cargo.lock
tools/architecture.py .github/workflows/ci.yml` confirms `text: auto` and
`eol: lf` on all four paths. This verifies attribute resolution, not an actual
Windows hosted job. Binary files remain subject to Git's automatic detection.

The architecture check uses Cargo's original dependency package name, so aliases
do not hide outward or unreviewed packages. Local internal dependencies must
resolve to the reviewed workspace manifest directory. Optional, target-specific
and build dependencies are included; dev dependencies are explicitly outside
this production-direction gate. Every workspace package must have a policy
entry. Exact external identity and archive provenance remain the separate
source-inventory check, which the aggregate developer command invokes.

The command runner passes argument lists without a shell, uses the repository
root explicitly, and enables subprocess failure checking. A separate mocked
subprocess probe confirmed aggregate `check` stops on its first failure and
the CLI propagates child exit code 37. Missing dependencies do not silently
convert failed checks into success. Windows chooses the virtual environment's
`Scripts/python.exe`; other hosts choose `bin/python`.

The workflow uses an immutable checkout action commit, read-only contents
permission, disabled credential persistence and `clean: false`. It invokes the
same developer commands for Linux/macOS/Windows, with a bounded job timeout.
The reviewed code contains no recursive cleanup command. Bootstrap creates the
environment, installs hash-locked Python requirements, selects the pinned Rust
toolchain and fetches the locked Cargo graph; no unnecessary reinstall was run
during this review.

## Independent validation

```text
.tools/venv/bin/python -m unittest discover -s tests -p test_architecture.py -v
PASS: 5 adversarial tests

.tools/venv/bin/python tools/architecture.py
PASS

python3 tools/dev.py --help
PASS: current command set is discoverable

python3 tools/dev.py check
PASS: 47 Rust tests, 7 doctests, 75 Python tests;
      formatting, Clippy, typecheck, architecture, ledger,
      86-package source inventory and synthetic fixture checks

Independent mocked subprocess probe
PASS: fail-fast aggregation and exact nonzero child exit propagation

Git attributes under core.autocrlf=true
PASS: reviewed text paths select LF
```

## Scope and remaining validation

Approval covers the implemented foundation and real executable CLI test command.
The reviewer did not execute hosted GitHub Actions, Linux, Windows, fresh-machine
composite bootstrap, packaging or a desktop/browser workflow. CI declarations
are not evidence that those platforms pass. Documentation preserves that limit.
Runner images and their Python/tool availability still require actual hosted
execution; action pinning alone does not freeze the entire build environment.

The checker is a declared direct-dependency gate, not a semantic proof: approved
upstream crates can have transitive dependencies, and Rust standard-library I/O
or manually copied foreign schemas remain code-review concerns. Production
observability, desktop commands, release packaging, distributable SBOM, broader
benchmarks and a user-authorized cleanup mechanism remain delivery work. No
complete Phase 0, full build/release acceptance or OSS runtime claim follows.

Only this report was changed by the reviewer. Suggested review commit:
`docs(review): approve initial developer commands and dependency checks`.
