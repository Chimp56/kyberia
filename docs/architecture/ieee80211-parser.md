# Independent IEEE 802.11 management-frame parser

`kyberia-ieee80211` is an outward adapter crate implementing the bounded
WIFI-001 foundation. It accepts a complete raw MAC MPDU only when the caller
selects `FcsAbsent` or `FcsPresentAndValidate`. There is intentionally no
framing autodetection: a CRC-looking final word is not reliable evidence that
an upstream source retained FCS. Radiotap and PCAPNG framing belong to their
own adapters.

The normalized record retains raw bytes as evidence and projects only stable
foundational structure: management subtype 4/5/8, the three address fields with
their precise roles, sequence/fragment, Beacon/Probe Response fixed fields,
and ordered IE TLVs. Address 3 in a Probe Request remains explicitly the BSSID
field and is never relabeled as the transmitter's BSSID. Supported management
subtypes carrying either ToDS or FromDS are rejected before those roles are
published.

Foundational typed views are non-destructive. Empty SSID means wildcard only
for Probe Request and hidden for Beacon/Probe Response; other SSIDs remain
binary. Rates retain their encoded byte/basic bit; DS retains only the raw
channel byte. TIM checks only Beacon context, payload length 4..=254, nonzero
DTIM period, and `count < period`. Country accepts only a minimum six-byte,
even-length legacy field with complete triplets and the exact required zero
pad; it retains the raw three-byte country/environment header without
regulatory inference. Malformed typed candidates still retain their raw IE.
Element 255 exposes its mandatory extension ID and raw body. Unknown and vendor
elements are preserved.

All repetitions appear in a deterministic occurrence summary whose indices
retain wire order. Only foundational
IDs 0, 1, 3, 5, 7, and 50 are marked as singleton-cardinality violations;
different singleton payloads are additionally marked contradictory. Vendor,
unknown, and extension repetitions are never declared invalid by this slice.

The normalized record and its owned collections have private fields and
read-only accessors, so safe Rust construction is confined to parsing and
canonical replay. The canonical `ky11ie` version-1 document contains only the
framing declaration and exact MPDU bytes. Encoding therefore needs no duplicate
reparse. Decoding rejects unknown envelope forms by exact length and reparses
the MPDU once. Derived views remain reproducible without becoming a second
source of truth.

Input, IE count/payload, canonical bytes, cumulative logical allocation, and
deterministic work are bounded with checked arithmetic under one control per
public operation. Allocation admission includes retained raw/IE/derived bytes,
exact-sized collection storage, repeat-group scratch and indices, and canonical
output. Cancellation is polled in CRC/TLV traversal, at typed-IE dispatch and
allocation checkpoints, in repeat grouping and replay validation, and during
canonical encoding. Per-IE typed value decoding is bounded by the IE's maximum
255-byte payload and does not poll inside each value-copy loop; cancellation
arriving mid-IE is observed at the next parser checkpoint. The bounds are
deterministic logical allocation/work limits, not allocator resident-memory or
wall-clock guarantees.

## Validation tooling boundary

The nested `crates/ieee80211/fuzz` project is developer-only validation
infrastructure and is deliberately outside the release workspace. Its pinned
nightly toolchain and `cargo-fuzz`/libFuzzer dependencies do not enter Kyberia's
runtime dependency graph. The target feeds both raw MPDUs and canonical
documents through fixed, finite parser-limit profiles, then requires every
successful parse to survive canonical encode/decode with exact normalized
equality. Generated campaign corpora are retained as local evidence; only the
six-input deterministic seed corpus is versioned. An executable seed contract
requires successful canonical-document replay, raw-frame parsing, and a
CRC-valid supported management frame with FCS validation, so those acceptance
paths cannot silently disappear from the seed set.

The ignored `tcpdump_differential` test is an explicit external-oracle check.
It writes deterministic classic PCAP bytes and requires the host's absolute
`/usr/sbin/tcpdump` executable to decode the same Beacon, Probe Request, and
Probe Response fixtures. The test directly compares Kyberia's typed frame
roles, printable/empty SSID display, supported rates where tcpdump emits them,
DS channel where displayed, and Beacon ESS/privacy flags against tcpdump's
parsed output. In the recorded tcpdump build, rates are emitted for Beacon and
Probe Request but not Probe Response; hidden-versus-wildcard SSID semantics are
checked internally, not distinguished by tcpdump's empty display. It neither
makes `tcpdump` a runtime dependency nor delegates Kyberia's parser authority
to that tool. The recorded run is host-specific and does not establish
cross-platform behavior, broader capture framing, or advanced Wi-Fi semantics.

This decision does not implement or validate WIFI-002 through WIFI-007,
INS-005, radiotap/PCAP/PCAPNG parsing, association/data/control frames,
collector wiring, security, HT/VHT/HE/EHT/MLO semantics, regulatory channel
legality, or identity linking.
