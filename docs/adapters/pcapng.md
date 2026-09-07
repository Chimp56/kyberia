# PCAPNG container adapter

`crates/packet-import` adopts `pcap-parser` 0.17.0 behind an outward adapter. Kyberia owns normalized evidence and Wi-Fi interpretation. The current increment parses PCAPNG container packets; it does not yet parse legacy PCAP, Radiotap/802.11, Kismet custom GPS metadata or canonical observation envelopes.

The chosen library supports both byte orders and multiple sections, uses safe Rust and borrowed data, and exposes typed blocks without requiring libpcap or privileged capture. Its package is MIT/Apache-2.0; included license files and the crate archive are pinned through Cargo.lock and the source inventory. Alternatives include libpcap-backed readers (native capture/library footprint) and writing a full parser (unnecessary duplicate maintenance). Kyberia uses the mature parser for block fields and adds defensive framing and semantic validation where its tolerant parsing would otherwise accept ambiguous input.

## Input and output contract

`pcapng::read(reader, limits, cancellation, consumer)` accepts a finite byte stream and delivers borrowed packet views to a callback. It returns a receipt only after valid EOF. The receipt identifies the decoder, SHA-256 of the exact consumed stream, byte/block/packet/section counts and skipped-block count. `(stream hash, block offset)` identifies a packet reception; correlation packet IDs do not remove duplicate receptions across interfaces. Callers must stage observations until a complete receipt is available. A callback can receive a valid prefix before a later error; that prefix is not a successfully imported file.

Both endian orders may occur in different sections. Interface IDs reset at every section, and an enhanced packet must reference an already declared interface. A simple packet uses interface zero and retains unknown time. Captured length excludes alignment padding. Original length is preserved; an original length below capture length is explicitly flagged instead of silently corrected, as allowed by the current format's inherited-length discrepancy rules. Snaplen zero is unlimited. No raw payload or identifier is automatically persisted or logged by this adapter.

The parser follows the [PCAPNG format draft](https://www.ietf.org/archive/id/draft-ietf-opsawg-pcapng-05.html) and [upstream library API](https://docs.rs/pcap-parser/0.17.0/pcap_parser/). Raw timestamp ticks, resolution byte and signed interface offset remain available. Decimal/binary resolution converts through integer arithmetic; subnanosecond values round downward with an explicit remainder flag. Source clock synchronization is not inferred from timestamp resolution. Canonical nanosecond overflow fails explicitly. Missing interface timestamp options select the format-defined microsecond/zero-offset defaults, not guessed hardware precision.

Enhanced packet flags, dropped count since the previous packet on that interface and correlation packet ID retain known/unknown distinction and section byte order. Other block types are counted as skipped. Interface statistics, arbitrary source application strings, comments, names, addresses, decryption secrets and custom payloads are not promoted into observations. This is a partial metadata contract and cannot pass the full Kismet/PCAP provenance gate yet.

## Defensive validation and bounds

Framing checks total length alignment, leading/trailing length equality, byte-order magic, supported section version 1.0, explicit section lengths and EOF. The adopted option parser tolerates malformed trailing options; Kyberia checks complete option-byte consumption and rejects duplicate timestamp/drop/ID fields and invalid lengths. An end marker must be final. A malformed timestamp option cannot disappear and silently select a default.

Default ceilings are 8 GiB per stream, ten million blocks/packets, one MiB per block, 1024 sections, 1024 interfaces per section, 256 options per recognized metadata block and 120 seconds. Callers may lower limits. Framing bounds allocation before parsing a block. Cancellation/deadline checks run before and after reads, between blocks and immediately before publishing the final receipt; consumer stop returns a distinct error. Kernel I/O may block, so a process supervisor is still required for hard cancellation and timeout. The callback must also bound its own work. Unknown blocks are explicit skipped evidence, and parser errors carry no source bytes or paths.

## Verification

```sh
cargo test -p kyberia-packet-import --locked --offline
cargo clippy -p kyberia-packet-import --all-targets --locked --offline -- -D warnings
cargo test -p kyberia-packet-import --release --locked --offline packet_stream_benchmark -- --ignored --nocapture
```

Original fixture constructors implement small format examples independently from the adopted parser. Seventeen behavioral tests exercise little/big endian parity, mixed-section interfaces, binary/decimal resolution, negative offsets, subnanosecond rounding, snaplen and padding, missing times, payload/length discrepancy, duplicate/malformed options, every truncated prefix, section boundaries, exact stream hashes, short reads, consumer errors, actual packet/block/interface/section ceilings, midstream cancellation, cancellation/deadline during the final EOF read, explicit zero versus unknown, 2048 deterministic byte mutations and overflow guards. The ignored explicit benchmark measures 10,000 and 100,000 packets with 256 payload bytes each, including parsing and hashing. These are synthetic software-contract tests, not real-radio evidence or proof against all malformed captures.

On macOS 26.6.2 ARM64, Rust 1.98.1 release builds parsed and hashed 10,000 packets (2,880,072 bytes) in 9.457 ms and 100,000 packets (28,800,072 bytes) in 95.047 ms. Input construction is outside the timed region. These local baselines are not stable cross-platform CI thresholds.

Remaining work includes interface-statistics/source/custom metadata, legacy PCAP, field parity against a pinned actual capture producer, Radiotap/802.11 parsing, canonical mapping, privacy-aware transactional publication, parser-process isolation and broader fuzz campaigns. No ordinary packet RSSI is labeled true spectrum data.
