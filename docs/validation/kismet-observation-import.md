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
