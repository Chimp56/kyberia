# Cross-platform CI Python runtime

## Evidence

Foundation validation run `34735001744` attempted CPython 3.12.12 on the
current GitHub-hosted Linux, macOS and Windows matrix. `actions/setup-python`
could not resolve that patch release on either the macOS ARM64 or Windows x64
runner, so both jobs failed before repository bootstrap or tests began.

The official `actions/python-versions` release manifest retrieved on
2026-09-12 lists CPython 3.12.10 packages for Linux ARM64/x64, macOS ARM64/x64,
and Windows ARM64/x64/x86. It lists 3.12.12 only for Linux ARM64/x64.

## Decision

The general cross-platform workflow uses CPython 3.12.10. This is the newest
3.12 patch release present for every architecture in the current hosted matrix.
The separately audited Sionna worker environment remains pinned to CPython
3.12.12; its lock, inventory, and numerical evidence are unchanged.

## Validation gate

A replacement hosted run must complete bootstrap, build, lint, typecheck,
tests, source inventory, and evidence checks on Linux, macOS, and Windows before
the second-platform gate can be marked validated.
