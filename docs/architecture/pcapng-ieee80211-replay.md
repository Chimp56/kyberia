# Bounded PCAPNG to IEEE 802.11 replay composition

`kyberia-pcap-ie-replay` composes the bounded PCAPNG reader and independent
IEEE 802.11 management parser at the workspace `composition` layer. It does
not make either lower-level adapter depend on the other.

The stable entry point is `replay(reader, framing, limits, cancellation)`.
The caller must declare `InputFraming::FcsAbsent` or
`InputFraming::FcsPresentAndValidate`; replay never guesses whether a packet
includes FCS. Only interface link type 105 (raw IEEE 802.11) is interpreted.
Link type 127 (radiotap) and every other non-105 link type are counted as
unsupported and skipped without stripping or inspecting a guessed radio
header. On link type 105, data/control frames, unsupported protocol versions,
and management subtypes not implemented by the current IEEE parser have
separate disposition counters. A packet too short to contain its frame-control
field is an error. A supported management subtype that fails IEEE parsing,
including FCS or structural validation, fails the whole replay.

Each returned `ReplayFrame` retains the original MPDU in the parser's
immutable frame and a `PacketProvenance` record containing packet ordinal,
block offset, section/interface identity, link type/snap length, original and
captured lengths, source length discrepancy, timestamp evidence, flags, drop
count, and correlation ID evidence. PCAPNG packet order is preserved; no
SSID/BSSID logging or radio/location inference is performed.

The bridge internally stages all derived frames. It returns a
`ReplayCapture` only after the reader has validated the complete container and
produced its final receipt; parse errors, malformed trailing blocks, resource
limits, cancellation, or I/O failures return no capture. The caller therefore
cannot observe a prefix through this API. Staging is bounded by a hard cap of
100,000 input packets, 50,000 parsed frames, 512 MiB input bytes, 50,000,000
aggregate parser work units, 256 MiB conservative retained logical bytes, and
the lower-level per-block/per-frame limits. Callers may lower these limits but
cannot raise the hard caps. Capacity grows geometrically; before each growth,
the bridge admits both the old and target vector capacities plus all prior
and current parser allocation charges against the retained-byte budget.

`ReplayCapture::replay_version()` identifies this composition release;
`parser_version()` identifies the exact IEEE parser crate release. The
PCAPNG decoder release, byte length, final SHA-256, block/packet counts and
section count are available through `receipt()`. These versions are not the
same as `ManagementFrame::schema_version()`.

This bridge is an offline parser/replay foundation only. It does not implement
live capture, radiotap decoding, Kismet enrichment, a desktop Lab inspector,
standards clause/help links, identity graphs, channel scheduling, physical or
cross-platform validation, or Phase 2 acceptance. `INS-005` and Phase 2 stay
in progress.
