# Offline Kismet normalization review

Independent reviewer: /root/operation_log_review_luna.
Initial commit ddde607: REQUEST_CHANGES for a MAJOR source-path replacement race.
Correction 32b5696: APPROVED with no remaining findings.
Integrated commits: 7e05adf and 51bb264. Authors: /root/channel_coupling_review_luna (normalizer), /root (source-open correction).

The review verified bounded KismetDB 5–10 metadata normalization, deterministic IDs, exact source-byte provenance, caller-owned identity mapping, UTC microseconds, and explicit unknown signal units, position, monotonic clock, dwell and radio identity. Raw Kismet signal is not silently converted to dBm. No GPL implementation was copied or linked.

MAJOR finding resolved: ordinary File::open could follow a symlink replacing the preflighted path. Source acquisition now uses atomic no-follow flags and validates the opened handle. Adversarial replacement tests pass. The selected handle defines the snapshot; parent-directory trust and closed-file requirements are documented. Windows code is not compiled or runtime-validated on this macOS host.

Independent correction checks passed: cargo test -p kyberia-kismet-adapter --locked --offline (18 passed, one explicit benchmark ignored), focused Clippy with -D warnings, cargo fmt --all -- --check, architecture, 134-package source inventory, and commit diff check. The original normalizer review also passed workspace tests and the explicit release benchmark. Generated source inventory conflicts during integration were resolved by regeneration from the combined lockfile.

Live authenticated Kismet, remote sources, channel hopping/drop telemetry, real producer-version validation, complete PCAPNG normalization parity and Windows execution remain open. This approval covers the offline increment, not the complete runtime gate.

Integration regression on 51bb264: `python3 tools/dev.py check` passed, including workspace Rust checks, 179 Python tests (19 optional skips), source inventory, ledger and original fixture verification.
