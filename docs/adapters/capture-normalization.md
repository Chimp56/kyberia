# Native macOS canonical normalization

`kyberia-capture-adapter::macos` validates complete `kyberia.macos.collector/1` NDJSON streams and converts their scan results into canonical domain V2 observations inside the canonical versioned `ReceivedObservation` response wrapper. This is an outward pure adapter; it neither calls CoreWLAN nor starts a process, requests consent, stores a project, or admits a survey sample.

The caller first runs `decode(&bytes)`, then supplies a `MappingContext` to `normalize`. The decoded stream exposes exact process, source and observation keys for a registry to resolve. The context must match the process session and provide distinct canonical source and observation IDs, a collector ID, a canonical session, and a clock epoch. IDs are never generated from BSSID, SSID, interface names or timestamps. Receiver sensor/adapter mappings remain separate from per-observation transmitter radio/BSS correlation. Optional transmitter correlation requires a known BSSID and explicit assignment evidence; unmapped mandatory IDs fail. Unknown sensor/adapter/radio evidence stays unknown. Redacted BSSID/SSID is permitted because those are optional canonical evidence, but cannot satisfy any downstream requirement for a known identifier.

Canonical privacy must agree with the wire policy. A redacted stream requires `IdentifierPolicy::Redacted`. An explicitly unredacted stream requires the caller's owned-infrastructure or explicit-research-consent policy; claiming project pseudonymization without actually transforming the identifiers fails. This unit does not implement pseudonymization. It returns exact source records which may contain identifiers, so the application must apply its policy before storage or export. Raw packet payload retention is not enabled by this collector.

## Scientific interpretation

| Source fact | Canonical meaning |
| --- | --- |
| CoreWLAN result receipt UTC and monotonic clock | `SourceResponseTiming.returned_at`; never observation capture time or host-ingestion time |
| API start/end window | `SourceResponseTiming.api_window`; never channel dwell |
| Actual capture time, result cache age, dwell | Unknown, with no receipt-time substitution |
| Source RSSI/noise | Known dBm only for the collector's accepted integer range -200 through -1; zero is the documented source sentinel, not known 0 dBm |
| Explicit uncalibrated state | `CalibrationState::Uncalibrated`; no calibration correction is invented |
| Recognized raw band and width enums | Canonical band and MHz, after checking enum consistency |
| API-reported channel number | Preserved in exact raw source record; canonical primary channel/frequency and center frequency remain unknown |
| Physical AP/BSS/MLD/PHY/IE identity | Unknown unless the corresponding explicit caller transmitter registry mapping is permitted; no SSID grouping or inferred generation |
| Position | Unknown; a clicked location or host receipt pose is not manufactured as capture pose |
| Synthetic fixture origin | Explicit normalized result origin plus `SyntheticFixture` source and observation quality flags, including empty/probe streams |
| Partial process completion | Structured terminal status plus `PartialCapture` on every retained observation |

The wire protocol exposes only millisecond UTC formatting. This adapter accepts its exact 24-byte UTC form, parses the calendar with `time`, rejects leap-second normalization, and checks conversion into canonical signed nanoseconds. Monotonic decimal strings preserve the complete u64 range and true zero. Neither a cross-clock synchronization model nor wall-clock uncertainty is invented. Source clock epoch mapping belongs to the caller's process registry and must not be reused for an unrelated process.

CoreWLAN does not establish whether its reported channel number is a primary channel or a center index in this integration. Therefore primary-channel and frequency fields remain unknown. A future coherent Wi-Fi channel semantics contract and live inspector integration must expose this source-reported quantity without relabeling it. This unit does not satisfy the complete channel inspector requirement.

Collector version is canonical `adapter_version`; the Rust normalizer has its own `parser_version`. Framework version is canonical `source_version`, with the collector's literal `unknown` sentinel converted to Unknown. OS version, adapter name, source schema, source-build hash and original per-record provenance remain inspectable. Exact LF-terminated source records accompany SHA-256 references, including hello, capabilities, API windows, raw enum values, native diagnostic codes and terminal events. An artifact reference identifies content; successful persistence is a separate operation.

## Defensive contract

A whole stream is at most 68,222,976 bytes (16,384 × 4,164), each record includes its terminating LF within 16,384 bytes, source lists have at most 32 interfaces, and observations have the collector-declared limit of at most 4,096. The input is an already acquired byte slice; process deadlines and bounded acquisition are caller responsibilities. Per-record decoding retains Serde JSON's nesting limit and rejects invalid UTF-8, duplicate keys at any depth, nonfinite numbers, malformed types, unknown fields and protocol versions, incomplete streams, discontinuous sequence numbers, mixed sessions, backwards receipt clocks, duplicate observation IDs, mismatched source descriptors and impossible API windows. API completion must follow the pre-call scan-start event receipt, and every observation sharing a scan ID must report the same completion time. Equal monotonic readings and UTC wall-clock steps remain valid. Error messages contain only bounded field labels and record indices, never imported identifiers or parser excerpts.

Capability claims are verified against authorization, global services, listed interfaces and power state. Noise is conditional. Reported bands are conditional rather than proof of an authorized scan. Monitor capture, channel control, per-chain readings, PHY extraction, spectrum, position, GPS and active testing are unavailable in this collector. Unsupported/denied/error/timeout/cancelled are structured terminal outcomes, not fake zero measurements. All input validation precedes publication; mapping/conversion failure returns no partial normalized result.

## Validation and scope

```sh
cargo test -p kyberia-capture-adapter --offline
cargo clippy -p kyberia-capture-adapter --all-targets --offline -- -D warnings
cargo fmt -p kyberia-capture-adapter -- --check
```

The current suite has 19 passing behavioral/property tests and two explicitly ignored support commands (golden regeneration and benchmark). Tests consume the repository's seven original macOS synthetic fixtures and add independently constructed mutations, permission transitions, unit/clock/identity cases and canonical expectations. Proptest exercises bounded arbitrary bytes and valid-stream byte mutations; this is bounded property/mutation fuzz coverage, not a claim of exhaustive fuzzing. No native permission prompt or real measurement is used in those tests. Authorized hardware scans, process transport supervision, live UI, project storage, read-time survey assignment, strict point-survey admission and continuous survey positioning remain open integration work. In particular, this adapter cannot supply actual capture time or pose needed by strict point admission.


The explicit release benchmark is `cargo test -p kyberia-capture-adapter --release --offline benchmark_maximum_scan -- --ignored --nocapture`. On this macOS arm64 host with Rust 1.98.1, the fixed original-fixture-derived workload measured:

| Observations | Input bytes | Decode | Normalize |
| --- | --- | --- | --- |
| 256 | 646,705 | 9.984 ms | 0.501 ms |
| 4,096 | 10,295,887 | 89.212 ms | 5.797 ms |

These single-run measurements are baselines, not stable CI thresholds or UI latency guarantees. Retaining decoded structures plus exact source bytes has bounded additional memory cost; normalization also returns its own source byte copies. The maximum allowed stream can contain up to 68 MB of whitespace-padded records, so acquisition limits and worker scheduling remain necessary. The maximum observation count benchmark does not claim worst-case padding, allocation, UI responsiveness or cancellation proof. The slice parser performs no external I/O and does not promise mid-call cancellation.

## Source and dependency provenance

The DTO contract comes from the independently implemented `collectors/macos/Sources/{Wire,main}.swift` and its documented v1 boundary. Fixtures are original Kyberia synthetic inputs from `collectors/macos/fixtures`; no vendor data, external fixture, GPL implementation or CoreWLAN object is embedded. The canonical JSON golden records the valid fixture output under explicit fixed test mappings and is checked alongside independently asserted UTC, RSSI, identity, unknown and source-hash expectations. Its regeneration test is ignored unless explicitly selected; ordinary tests never rewrite it. Their existing source ledger designation remains NOASSERTION pending the repository distribution decision. This adapter's new tests are original and do not change that license review.

Commodity parsers are isolated outward and pinned: [base64 0.22.1](https://docs.rs/crate/base64/0.22.1) (MIT OR Apache-2.0), [time 0.3.55](https://docs.rs/crate/time/0.3.55) (MIT OR Apache-2.0, parsing only), existing Serde 1.0.228 / serde_json 1.0.149 and SHA-2 0.10.9. Base64 uses the standard alphabet with required canonical padding and rejects nonzero trailing padding bits. Time performs Gregorian/RFC3339 validation; this adapter applies the narrower collector contract and checked domain range afterward. No date parser or base64 codec is reimplemented. Root integration owns the lockfile, transitive source inventory, license ledger and automated audit. Updates require contract, numerical, malformed-input and canonical golden tests before the pin changes.

Dependency review rejected an earlier `time` 0.3.44 candidate because [RUSTSEC-2026-0009](https://rustsec.org/advisories/RUSTSEC-2026-0009.html) affects its RFC 2822 parser. This adapter only accepts fixed-length RFC3339, but the final 0.3.55 pin includes the upstream fix and requires no advisory exception.
