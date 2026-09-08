# Kismet offline observation import validation

This increment extends the read-only KismetDB 5–10 metadata boundary into a
bounded canonical V2 normalizer. Kismet remains an external GPL process/file
producer; no Kismet implementation or object schema crosses into Kyberia
domain code.

`KismetDb::normalize_batch` requires explicit application-owned session,
canonical datasource mappings and privacy policy. It returns canonical
`ReceivedObservation` frames, a versioned source/hash/row-count receipt, and a
per-row receipt retaining the SQLite row ID and raw PHY signal integer. The
integer has no established dBm meaning in this database boundary, so canonical
RSSI is `Unknown(UnsupportedCapability)`. Packet timestamps retain the
source-reported UTC microsecond field only. Monotonic time, synchronization,
receipt time, pose, dwell, complete channel geometry, noise, radio/BSSID/ESS/
MLD identity and information elements remain explicit unknown/not-retained
states. Version 7 PHY rate and version 9 original length remain typed when
their columns are present.

The normalizer derives observation IDs from the immutable database SHA-256 and
SQLite row ID. It preserves row order and pagination boundaries, rejects
incomplete datasource mappings and payload-retaining policy, fails without a
partial batch on malformed canonical conversion, and maps an explicit Kismet
source-error flag to unusable `Malformed` quality. A complete import still
requires consuming every batch from the initial cursor and calling `finish()`;
the batch `complete` flag alone does not certify earlier cursors.

## Validation commands

```text
cargo test -p kyberia-kismet-adapter --locked --offline
cargo clippy -p kyberia-kismet-adapter --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
```

The focused suite passes 16 tests with one explicit metadata benchmark ignored.
It covers all six supported schema versions, deterministic pagination and
normalization, source/hash/row provenance, source-version distinction,
timestamp/channel unknown semantics, typed version-gated values, required
mapping/privacy checks, source-error quality, malformed and oversized values,
schema drift, duplicate handling, cancellation/deadline behavior, source
mutation, stripped payloads and corruption. Fixtures are original synthetic
SQLite data; no Kismet runtime or upstream implementation is bundled.

## Remaining gates

The adapter does not persist into a project bundle, atomically publish chunks,
associate observations with pose or survey points, parse PCAPNG, consume
authenticated REST/WebSocket streams, infer physical radio identity, or
establish channel-hop/dwell/drop telemetry absent from the imported rows.
Producer software versions and actual Kismet release artifacts require pinned
runtime fixtures. The Kismet integration gate and Linux hardware validation
remain open.

## Source-open correction

Independent review found a race between path metadata preflight and ordinary open.
The source now opens atomically without following a final-component symlink and
validates the opened handle before copying bytes. Unix uses `O_NOFOLLOW` and
`O_NONBLOCK`; Windows opens the reparse point itself, rejects reparse attributes,
and denies write/delete sharing. The selected handle defines the snapshot,
so replacing the path after open cannot redirect the read. Parent directories
remain operator-selected; this is not a sandbox for arbitrary directory traversal.
A closed source file remains required; no transactional snapshot of a live writer
is promised. Windows execution remains an explicit native validation gate.

Regression tests replace a preflighted file with a symlink, replace the path after
handle acquisition, and reject directory input. The original symlink rejection
error category is preserved. Platform behavior follows the official
[Rust Unix OpenOptionsExt](https://doc.rust-lang.org/std/os/unix/fs/trait.OpenOptionsExt.html),
[Rust Windows OpenOptionsExt](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html),
and [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
contracts. No unsafe implementation code was introduced; pinned existing libc
and windows-sys packages supply platform constants.
