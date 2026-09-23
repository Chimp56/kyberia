# Kyberia IEEE 802.11 foundation

This adapter-layer crate parses complete raw IEEE 802.11 MAC management MPDUs
for Beacon, Probe Response, and Probe Request frames. The caller must state
whether a four-byte FCS is absent or present; present FCS bytes are validated
with the IEEE CRC-32. The parser never guesses framing and does not accept
radiotap, PCAP/PCAPNG containers, data frames, control frames, or association
frames.

The normalized record retains the raw MPDU, MAC address roles, sequence and
fragment numbers, fixed fields, and every IE in exact wire order with absolute
offsets and payload bytes. Foundational typed views are deliberately narrow:
SSID, supported rates, DS parameter channel byte, TIM structure, Country
triplet structure, and the extension wrapper. Unknown and vendor IEs remain
lossless. No RSN, regulatory, HT, VHT, HE, EHT, MLO, channel-width, identity,
or capability conclusion is made.

`ParseLimits` bounds input bytes, element count, retained IE payload bytes,
canonical bytes, cumulative logical allocation bytes, and deterministic work
units for each public operation. The allocation admission includes raw and IE
copies, derived byte views, repeat indices/grouping scratch, and canonical
output. `Cancellation` is polled throughout CRC, TLV, typed-decode, repeat
grouping, canonical encode, and replay validation loops. Canonical documents
contain the exact input MPDU plus its explicit framing policy; decoding uses
one cumulative control while reparsing that evidence. Parsed frame fields and
owned collections are exposed only through read-only accessors.

Run:

```text
cargo test -p kyberia-ieee80211 --locked --offline
cargo run -p kyberia-ieee80211 --example deterministic_mutation --locked --offline
cargo test -p kyberia-ieee80211 --test tcpdump_differential \
  checked_in_management_frames_match_tcpdump_stable_fields -- \
  --ignored --exact --nocapture
```

The deterministic mutation target remains a bounded regression harness. The
separate `fuzz/` project provides the pinned libFuzzer target and retained
bounded campaign evidence. The explicit ignored differential test constructs
an original DLT_IEEE802_11 PCAP from checked-in fixtures and fails closed unless
the required external `tcpdump` executable successfully agrees on its stable
decoded fields. Neither tool is a Kyberia runtime dependency or authority.
