# ADR-0025: Narrow Windows disk-handle validation

Status: Proposed; independent review and native Windows execution pending.

## Context

Plan security/import/platform requirements prohibit interpreting devices or pipes
as bounded ordinary request files. Review WR-001 found that Windows
`Metadata::is_file` does not establish a disk handle. A byte limit alone cannot
bound a blocking device read. Workspace core crates forbid unsafe Rust.

## Decision

Add an outward `kyberia-file-adapter` exposing one safe borrowed-File query.
It requires `GetFileType == FILE_TYPE_DISK` and rejects all other return values.
It neither owns nor closes the input handle. The caller also validates regular
file metadata, all reparse attributes, the byte limit and JSON structure.
Windows request acquisition allows read sharing only, preventing ordinary
concurrent write/delete opens during acquisition.

The adapter denies unsafe Rust except for the single documented function
calling already-pinned `windows-sys` 0.61.2. Domain, numerical and all existing
crate policies remain unchanged. This exception is local to an OS boundary.

## Alternatives

Metadata-only checking is insufficient. Rejecting Windows input entirely would
leave required product functionality unavailable. The capability-filesystem
stack offers broader sandbox operations and dependencies than this handle-kind
query needs; no path sandbox is claimed here. A direct FFI call in CLI would
spread the unsafe exception into application orchestration.

## Evidence

Microsoft documents disk, character, pipe and unknown handle types in
[GetFileType](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfiletype).
The adapter compiles with all native Windows test targets using
`cargo check -p kyberia-file-adapter --all-targets --target x86_64-pc-windows-gnu --locked --offline`.
This is compilation evidence only. Original Windows tests exercise an executable
disk file, NUL character device and connected named pipe that sends no data.

## Consequences

This admits disk handles before any read and makes unknown type an error.
It does not provide a disk-I/O deadline, parent-path sandbox or guarantees
against privileged drivers modifying content. Existing reparse and request
validation gates remain required. No foreign handle crosses into core types.

## Reversibility

A reviewed safe upstream wrapper can replace the implementation behind the same
borrowed-File interface. There is no persisted-format change.

## Validation plan

Independent safety review of handle lifetime and exact platform policy; native
Windows disk/device/pipe tests and real CLI workflows; retained reparse fixtures
on a host with link privilege; macOS regression and workspace boundary checks.
A compile pass must not be labeled runtime validation.
