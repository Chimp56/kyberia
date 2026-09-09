# Windows stored request reader review

Initial source: root draft based on `0765787`. Independent reviewer: Beauvoir.
Disposition: REQUEST_CHANGES.

## WR-001 — MAJOR: opened-handle type

Windows `Metadata::is_file` alone did not establish a disk handle. A byte cap
does not stop a blocking device or pipe read. The correction requires
`GetFileType == FILE_TYPE_DISK` through a narrowly audited outer file adapter
before inspecting metadata or reading bytes from that handle. Unknown types
are rejected. [ADR-0025](../architecture/ADR/0025-windows-disk-handle.md) records
the local FFI exception; existing core unsafe-code policies remain intact.

## WR-002 — MINOR: concurrent mutation sharing

The initial draft shared write and delete access. The correction shares read
access only. It does not claim a transactional snapshot against privileged
drivers or a disk I/O deadline.

## Correction validation

- `cargo test -p kyberia-cli -p kyberia-file-adapter --locked --offline`: PASS on macOS.
- `cargo clippy -p kyberia-cli -p kyberia-file-adapter --all-targets --locked --offline -- -D warnings`: PASS.
- `cargo check -p kyberia-file-adapter --all-targets --target x86_64-pc-windows-gnu --locked --offline`: PASS.
- `cargo clippy -p kyberia-file-adapter --all-targets --target x86_64-pc-windows-gnu --locked --offline -- -D warnings`: PASS.
- Formatting, architecture and locked source inventory checks: PASS.

Native Windows tests cover disk files, NUL, and a connected local named pipe
that sends no bytes. They have compiled, not executed on this macOS host.
Windows CLI runtime, final-path reparse tests and race/sharing validation remain
open. Native fixture execution uses a local PowerShell pipe server terminated
and reaped by its test owner; no test directory is deleted.

The correction needs independent review before integration. This packet does
not close WR-001/WR-002 by author assertion.
