//! Bounded composition of the PCAPNG container reader and the independent
//! IEEE 802.11 management-frame parser.
//!
//! [`replay`] accepts complete PCAPNG input, but only interprets packets whose
//! interface declares raw IEEE 802.11 (`LINKTYPE_IEEE802_11`, 105). Other
//! link types (including radiotap, 127) are counted and skipped; they are not
//! guessed or stripped. The caller must explicitly select [`InputFraming`],
//! including whether an FCS is absent or present and validated.
//!
//! Results are staged internally and returned only after the complete
//! PCAPNG reader produces its final receipt. Any later malformed supported
//! frame, input error, cancellation, or resource-limit failure drops the
//! entire staged result. There is no callback that can publish a prefix.
//! Streaming kernel/device reads can still block; callers needing hard I/O
//! timeouts must isolate the reader in a killable process, as documented by
//! the underlying packet importer.
#![forbid(unsafe_code)]

use kyberia_domain::evidence::Evidence;
use kyberia_ieee80211::{
    Cancellation, Error as IeeeError, ManagementFrame, ParseLimits, ResourceUsage, parse_with_usage,
};
use kyberia_packet_import::pcapng::{
    self, Limits as ImportLimits, Packet, Receipt as ImportReceipt, ReportedTimestamp,
};
use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub use kyberia_ieee80211::InputFraming;

const IEEE802_11_LINKTYPE: u32 = 105;
const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_BLOCKS: u64 = 2_000_000;
const MAX_INPUT_PACKETS: u64 = 100_000;
const MAX_BLOCK_BYTES: usize = 1024 * 1024;
const MAX_INTERFACES_PER_SECTION: usize = 256;
const MAX_SECTIONS: u32 = 32;
const MAX_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_FRAMES: usize = 50_000;
const MAX_TOTAL_PARSE_WORK: usize = 50_000_000;
const MAX_RETAINED_BYTES: usize = 256 * 1024 * 1024;
const REPLAY_VERSION: &str = "kyberia-pcap-ie-replay/0.1.0";
const PARSER_VERSION: &str = "kyberia-ieee80211/0.1.0";

/// Caller-adjustable limits that may reduce, but never raise, the replay
/// crate's hard safety ceilings. Packet limits include packets later skipped
/// by link type or frame classification.
#[derive(Clone, Copy)]
pub struct Limits {
    pub import: ImportLimits,
    pub parse: ParseLimits,
    pub max_frames: usize,
    pub max_total_parse_work_units: usize,
    /// Conservative logical retained bytes: IEEE parser allocation charges
    /// plus the actual capacity of the staged `ReplayFrame` vector.
    pub max_retained_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        let import = ImportLimits {
            max_bytes: MAX_INPUT_BYTES,
            max_blocks: MAX_BLOCKS,
            max_packets: MAX_INPUT_PACKETS,
            max_block_bytes: MAX_BLOCK_BYTES,
            max_interfaces_per_section: MAX_INTERFACES_PER_SECTION,
            max_sections: MAX_SECTIONS,
            timeout: MAX_TIMEOUT,
        };
        Self {
            import,
            parse: ParseLimits::default(),
            max_frames: MAX_FRAMES,
            max_total_parse_work_units: MAX_TOTAL_PARSE_WORK,
            max_retained_bytes: MAX_RETAINED_BYTES,
        }
    }
}

impl Limits {
    fn validate(self) -> Result<(), Error> {
        let hard_parse = ParseLimits::default();
        let p = self.import;
        let valid_import = p.max_bytes >= 28
            && p.max_bytes <= MAX_INPUT_BYTES
            && (1..=MAX_BLOCKS).contains(&p.max_blocks)
            && (1..=MAX_INPUT_PACKETS).contains(&p.max_packets)
            && (28..=MAX_BLOCK_BYTES).contains(&p.max_block_bytes)
            && (1..=MAX_INTERFACES_PER_SECTION).contains(&p.max_interfaces_per_section)
            && (1..=MAX_SECTIONS).contains(&p.max_sections)
            && !p.timeout.is_zero()
            && p.timeout <= MAX_TIMEOUT;
        let q = self.parse;
        let valid_parse = q.max_frame_bytes != 0
            && q.max_frame_bytes <= hard_parse.max_frame_bytes
            && q.max_elements <= hard_parse.max_elements
            && q.max_ie_payload_bytes <= hard_parse.max_ie_payload_bytes
            && q.max_work_units != 0
            && q.max_work_units <= hard_parse.max_work_units
            && q.max_canonical_bytes <= hard_parse.max_canonical_bytes
            && q.max_allocation_bytes != 0
            && q.max_allocation_bytes <= hard_parse.max_allocation_bytes;
        if !valid_import
            || !valid_parse
            || self.max_frames == 0
            || self.max_frames > MAX_FRAMES
            || self.max_total_parse_work_units == 0
            || self.max_total_parse_work_units > MAX_TOTAL_PARSE_WORK
            || self.max_retained_bytes == 0
            || self.max_retained_bytes > MAX_RETAINED_BYTES
        {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// Failure is all-or-error: no `ReplayCapture` is returned for any failure.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidLimits,
    Input(pcapng::Error),
    Cancelled,
    ResourceLimit(&'static str),
    MalformedPacketHeader {
        packet_index: u64,
    },
    MalformedSupportedFrame {
        packet_index: u64,
        source: IeeeError,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimits => f.write_str("invalid or excessive replay limits"),
            Self::Input(error) => write!(f, "PCAPNG replay input failed: {error}"),
            Self::Cancelled => f.write_str("PCAPNG replay cancelled"),
            Self::ResourceLimit(name) => write!(f, "PCAPNG replay resource limit: {name}"),
            Self::MalformedPacketHeader { packet_index } => {
                write!(
                    f,
                    "raw 802.11 packet {packet_index} has no frame-control field"
                )
            }
            Self::MalformedSupportedFrame {
                packet_index,
                source,
            } => write!(
                f,
                "supported 802.11 packet {packet_index} is malformed: {source}"
            ),
        }
    }
}
impl std::error::Error for Error {}

/// Packet-source identity and timestamp copied without normalization loss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PacketProvenance {
    packet_index: u64,
    block_offset: u64,
    section_index: u32,
    interface_index: u32,
    link_type: u32,
    snaplen: u32,
    timestamp: Evidence<ReportedTimestamp>,
    original_length: u32,
    captured_length: u32,
    contradictory_original_length: bool,
    flags: Evidence<u32>,
    dropped_since_previous: Evidence<u64>,
    correlation_packet_id: Evidence<u64>,
}

impl PacketProvenance {
    pub fn packet_index(&self) -> u64 {
        self.packet_index
    }
    pub fn block_offset(&self) -> u64 {
        self.block_offset
    }
    pub fn section_index(&self) -> u32 {
        self.section_index
    }
    pub fn interface_index(&self) -> u32 {
        self.interface_index
    }
    pub fn link_type(&self) -> u32 {
        self.link_type
    }
    pub fn snaplen(&self) -> u32 {
        self.snaplen
    }
    pub fn timestamp(&self) -> &Evidence<ReportedTimestamp> {
        &self.timestamp
    }
    pub fn original_length(&self) -> u32 {
        self.original_length
    }
    pub fn captured_length(&self) -> u32 {
        self.captured_length
    }
    pub fn contradictory_original_length(&self) -> bool {
        self.contradictory_original_length
    }
    pub fn flags(&self) -> &Evidence<u32> {
        &self.flags
    }
    pub fn dropped_since_previous(&self) -> &Evidence<u64> {
        &self.dropped_since_previous
    }
    pub fn correlation_packet_id(&self) -> &Evidence<u64> {
        &self.correlation_packet_id
    }
}

/// One parsed supported management frame in deterministic PCAPNG packet order.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayFrame {
    provenance: PacketProvenance,
    frame: ManagementFrame,
    parse_usage: ResourceUsage,
}

impl ReplayFrame {
    pub fn provenance(&self) -> &PacketProvenance {
        &self.provenance
    }
    pub fn frame(&self) -> &ManagementFrame {
        &self.frame
    }
    pub fn parse_usage(&self) -> ResourceUsage {
        self.parse_usage
    }
}

/// Packet dispositions are mutually exclusive; their sum equals the final
/// packet count in the PCAPNG receipt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dispositions {
    packets: u64,
    parsed_management_frames: u64,
    skipped_unsupported_link_type: u64,
    skipped_non_management: u64,
    skipped_unsupported_protocol_version: u64,
    skipped_unsupported_management_subtype: u64,
}

impl Dispositions {
    pub fn packets(self) -> u64 {
        self.packets
    }
    pub fn parsed_management_frames(self) -> u64 {
        self.parsed_management_frames
    }
    pub fn skipped_unsupported_link_type(self) -> u64 {
        self.skipped_unsupported_link_type
    }
    pub fn skipped_non_management(self) -> u64 {
        self.skipped_non_management
    }
    pub fn skipped_unsupported_protocol_version(self) -> u64 {
        self.skipped_unsupported_protocol_version
    }
    pub fn skipped_unsupported_management_subtype(self) -> u64 {
        self.skipped_unsupported_management_subtype
    }
}

/// Complete verified container receipt plus bounded normalized frames.
#[derive(Debug)]
pub struct ReplayCapture {
    replay_version: &'static str,
    parser_version: &'static str,
    receipt: ImportReceipt,
    dispositions: Dispositions,
    frames: Vec<ReplayFrame>,
    total_parse_work_units: usize,
    retained_logical_bytes: usize,
}

impl ReplayCapture {
    /// Version of this replay composition crate; independent of frame schema.
    pub fn replay_version(&self) -> &'static str {
        self.replay_version
    }
    /// Exact version of the IEEE parser crate used for this result.
    pub fn parser_version(&self) -> &'static str {
        self.parser_version
    }
    pub fn receipt(&self) -> &ImportReceipt {
        &self.receipt
    }
    pub fn dispositions(&self) -> Dispositions {
        self.dispositions
    }
    pub fn frames(&self) -> &[ReplayFrame] {
        &self.frames
    }
    pub fn total_parse_work_units(&self) -> usize {
        self.total_parse_work_units
    }
    pub fn retained_logical_bytes(&self) -> usize {
        self.retained_logical_bytes
    }
}

struct ParserCancellation<'a>(&'a AtomicBool);
impl Cancellation for ParserCancellation<'_> {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Staging {
    frames: Vec<ReplayFrame>,
    dispositions: Dispositions,
    next_packet_index: u64,
    total_parse_work_units: usize,
    frame_allocation_bytes: usize,
    retained_logical_bytes: usize,
}

impl Staging {
    fn increment(value: &mut u64, limit_name: &'static str) -> Result<(), Error> {
        *value = value
            .checked_add(1)
            .ok_or(Error::ResourceLimit(limit_name))?;
        Ok(())
    }

    fn provenance(packet: &Packet<'_>, packet_index: u64) -> PacketProvenance {
        PacketProvenance {
            packet_index,
            block_offset: packet.block_offset,
            section_index: packet.interface.section_index,
            interface_index: packet.interface.interface_index,
            link_type: packet.interface.link_type,
            snaplen: packet.interface.snaplen,
            timestamp: packet.timestamp.clone(),
            original_length: packet.original_length,
            captured_length: packet.captured_length,
            contradictory_original_length: packet.contradictory_original_length,
            flags: packet.flags.clone(),
            dropped_since_previous: packet.dropped_since_previous.clone(),
            correlation_packet_id: packet.correlation_packet_id.clone(),
        }
    }

    fn accept(
        &mut self,
        packet: Packet<'_>,
        framing: InputFraming,
        limits: Limits,
        cancel: &AtomicBool,
    ) -> Result<(), Error> {
        let packet_index = self.next_packet_index;
        self.next_packet_index = self
            .next_packet_index
            .checked_add(1)
            .ok_or(Error::ResourceLimit("packet index"))?;
        self.dispositions.packets = self
            .dispositions
            .packets
            .checked_add(1)
            .ok_or(Error::ResourceLimit("packet count"))?;

        if packet.interface.link_type != IEEE802_11_LINKTYPE {
            return Self::increment(
                &mut self.dispositions.skipped_unsupported_link_type,
                "skipped packet count",
            );
        }
        let [fc_low, fc_high, ..] = packet.data else {
            return Err(Error::MalformedPacketHeader { packet_index });
        };
        let frame_control = u16::from_le_bytes([*fc_low, *fc_high]);
        let protocol_version = frame_control & 0b11;
        if protocol_version != 0 {
            return Self::increment(
                &mut self.dispositions.skipped_unsupported_protocol_version,
                "skipped packet count",
            );
        }
        let frame_type = (frame_control >> 2) & 0b11;
        if frame_type != 0 {
            return Self::increment(
                &mut self.dispositions.skipped_non_management,
                "skipped packet count",
            );
        }
        let subtype = ((frame_control >> 4) & 0b1111) as u8;
        if !supported_subtype(subtype) {
            return Self::increment(
                &mut self.dispositions.skipped_unsupported_management_subtype,
                "skipped packet count",
            );
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if self.frames.len() >= limits.max_frames {
            return Err(Error::ResourceLimit("parsed frame count"));
        }

        let required_len = self
            .frames
            .len()
            .checked_add(1)
            .ok_or(Error::ResourceLimit("parsed frame count"))?;
        let old_capacity = self.frames.capacity();
        let grows = required_len > old_capacity;
        let planned_capacity = if grows {
            old_capacity
                .max(4)
                .checked_mul(2)
                .unwrap_or(MAX_FRAMES)
                .max(required_len)
                .min(limits.max_frames)
        } else {
            old_capacity
        };
        let planned_vector_bytes = planned_capacity
            .checked_mul(size_of::<ReplayFrame>())
            .ok_or(Error::ResourceLimit("retained allocation bytes"))?;
        // During growth, account for both the old allocation and the target
        // allocation while `try_reserve_exact` moves the staged records.
        let vector_growth_peak = if grows {
            old_capacity
                .checked_add(planned_capacity)
                .and_then(|n| n.checked_mul(size_of::<ReplayFrame>()))
                .ok_or(Error::ResourceLimit("retained allocation bytes"))?
        } else {
            planned_vector_bytes
        };
        let parse_budget_base = self
            .frame_allocation_bytes
            .checked_add(vector_growth_peak)
            .ok_or(Error::ResourceLimit("retained allocation bytes"))?;
        let parse_allocation_budget = limits
            .max_retained_bytes
            .checked_sub(parse_budget_base)
            .ok_or(Error::ResourceLimit("retained allocation bytes"))?;
        let parse_work_budget = limits
            .max_total_parse_work_units
            .checked_sub(self.total_parse_work_units)
            .ok_or(Error::ResourceLimit("total parser work"))?;
        if parse_allocation_budget == 0 || parse_work_budget == 0 {
            return Err(Error::ResourceLimit("aggregate parser budget"));
        }
        let mut parse_limits = limits.parse;
        parse_limits.max_allocation_bytes = parse_limits
            .max_allocation_bytes
            .min(parse_allocation_budget);
        parse_limits.max_work_units = parse_limits.max_work_units.min(parse_work_budget);

        let (frame, usage) = parse_with_usage(
            packet.data,
            framing,
            parse_limits,
            &ParserCancellation(cancel),
        )
        .map_err(|source| match source {
            IeeeError::Cancelled => Error::Cancelled,
            IeeeError::LimitExceeded("allocation bytes")
                if parse_limits.max_allocation_bytes < limits.parse.max_allocation_bytes =>
            {
                Error::ResourceLimit("retained allocation bytes")
            }
            IeeeError::LimitExceeded("work units")
                if parse_limits.max_work_units < limits.parse.max_work_units =>
            {
                Error::ResourceLimit("total parser work")
            }
            IeeeError::LimitExceeded(_) => Error::ResourceLimit("per-frame parser"),
            source => Error::MalformedSupportedFrame {
                packet_index,
                source,
            },
        })?;
        let total_work = self
            .total_parse_work_units
            .checked_add(usage.work_units())
            .ok_or(Error::ResourceLimit("total parser work"))?;
        if total_work > limits.max_total_parse_work_units {
            return Err(Error::ResourceLimit("total parser work"));
        }
        let allocation_total = self
            .frame_allocation_bytes
            .checked_add(usage.allocation_bytes())
            .ok_or(Error::ResourceLimit("retained allocation bytes"))?;
        let planned_total = planned_vector_bytes
            .checked_add(allocation_total)
            .ok_or(Error::ResourceLimit("retained allocation bytes"))?;
        if planned_total > limits.max_retained_bytes {
            return Err(Error::ResourceLimit("retained allocation bytes"));
        }

        if grows {
            self.frames
                .try_reserve_exact(planned_capacity - self.frames.len())
                .map_err(|_| Error::ResourceLimit("staged frame vector allocation"))?;
        }
        let actual_vector_bytes = self
            .frames
            .capacity()
            .checked_mul(size_of::<ReplayFrame>())
            .ok_or(Error::ResourceLimit("retained allocation bytes"))?;
        let actual_total = actual_vector_bytes
            .checked_add(allocation_total)
            .ok_or(Error::ResourceLimit("retained allocation bytes"))?;
        let actual_growth_peak = if grows {
            old_capacity
                .checked_add(self.frames.capacity())
                .and_then(|n| n.checked_mul(size_of::<ReplayFrame>()))
                .and_then(|n| n.checked_add(self.frame_allocation_bytes))
                .and_then(|n| n.checked_add(usage.allocation_bytes()))
                .ok_or(Error::ResourceLimit("retained allocation bytes"))?
        } else {
            actual_total
        };
        if actual_total > limits.max_retained_bytes
            || actual_growth_peak > limits.max_retained_bytes
        {
            return Err(Error::ResourceLimit("retained allocation bytes"));
        }

        let provenance = Self::provenance(&packet, packet_index);
        self.frames.push(ReplayFrame {
            provenance,
            frame,
            parse_usage: usage,
        });
        self.total_parse_work_units = total_work;
        self.frame_allocation_bytes = allocation_total;
        self.retained_logical_bytes = actual_total;
        self.dispositions.parsed_management_frames = self
            .dispositions
            .parsed_management_frames
            .checked_add(1)
            .ok_or(Error::ResourceLimit("parsed frame count"))?;
        Ok(())
    }
}

fn supported_subtype(subtype: u8) -> bool {
    matches!(subtype, 0 | 1 | 2 | 3 | 4 | 5 | 8)
}

/// Replay one complete PCAPNG byte stream into bounded normalized management
/// frames. `framing` is mandatory and is applied only to supported raw 802.11
/// management frames. No partially staged result is observable on failure.
pub fn replay<R: std::io::Read>(
    input: R,
    framing: InputFraming,
    limits: Limits,
    cancel: &AtomicBool,
) -> Result<ReplayCapture, Error> {
    limits.validate()?;
    let mut staged = Staging::default();
    let mut callback_error = None;
    let receipt = pcapng::read(input, limits.import, cancel, |packet| {
        match staged.accept(packet, framing, limits, cancel) {
            Ok(()) => true,
            Err(error) => {
                callback_error = Some(error);
                false
            }
        }
    });
    let receipt = match receipt {
        Ok(receipt) => receipt,
        Err(_) if callback_error.is_some() => return Err(callback_error.expect("checked")),
        Err(pcapng::Error::Cancelled) => return Err(Error::Cancelled),
        Err(error) => return Err(Error::Input(error)),
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if staged.next_packet_index != receipt.packets {
        return Err(Error::Input(pcapng::Error::Malformed));
    }
    let classified = staged
        .dispositions
        .parsed_management_frames
        .checked_add(staged.dispositions.skipped_unsupported_link_type)
        .and_then(|n| n.checked_add(staged.dispositions.skipped_non_management))
        .and_then(|n| n.checked_add(staged.dispositions.skipped_unsupported_protocol_version))
        .and_then(|n| n.checked_add(staged.dispositions.skipped_unsupported_management_subtype))
        .ok_or(Error::ResourceLimit("packet disposition count"))?;
    if classified != staged.dispositions.packets || classified != receipt.packets {
        return Err(Error::Input(pcapng::Error::Malformed));
    }
    Ok(ReplayCapture {
        replay_version: REPLAY_VERSION,
        parser_version: PARSER_VERSION,
        receipt,
        dispositions: staged.dispositions,
        frames: staged.frames,
        total_parse_work_units: staged.total_parse_work_units,
        retained_logical_bytes: staged.retained_logical_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_ieee80211::ManagementSubtype;
    use std::{
        io::{self, Cursor, Read},
        sync::atomic::AtomicBool,
    };

    fn u16le(value: u16) -> [u8; 2] {
        value.to_le_bytes()
    }
    fn u32le(value: u32) -> [u8; 4] {
        value.to_le_bytes()
    }
    fn block(kind: u32, mut body: Vec<u8>) -> Vec<u8> {
        let length = (12 + body.len()).div_ceil(4) * 4;
        body.resize(length - 12, 0);
        let mut result = Vec::with_capacity(length);
        result.extend_from_slice(&u32le(kind));
        result.extend_from_slice(&u32le(length as u32));
        result.extend_from_slice(&body);
        result.extend_from_slice(&u32le(length as u32));
        result
    }
    fn shb() -> Vec<u8> {
        let mut body = vec![0x4d, 0x3c, 0x2b, 0x1a];
        body.extend_from_slice(&u16le(1));
        body.extend_from_slice(&u16le(0));
        body.extend_from_slice(&(-1i64).to_le_bytes());
        block(0x0a0d0d0a, body)
    }
    fn idb(link_type: u16, snaplen: u32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&u16le(link_type));
        body.extend_from_slice(&u16le(0));
        body.extend_from_slice(&u32le(snaplen));
        block(1, body)
    }
    fn epb(interface: u32, ticks: u64, data: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&u32le(interface));
        body.extend_from_slice(&u32le((ticks >> 32) as u32));
        body.extend_from_slice(&u32le(ticks as u32));
        body.extend_from_slice(&u32le(data.len() as u32));
        body.extend_from_slice(&u32le(data.len() as u32));
        body.extend_from_slice(data);
        block(6, body)
    }
    fn capture(interfaces: &[(u16, u32)], packets: &[(u32, u64, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = shb();
        for (link_type, snaplen) in interfaces {
            bytes.extend_from_slice(&idb(*link_type, *snaplen));
        }
        for (interface, ticks, data) in packets {
            bytes.extend_from_slice(&epb(*interface, *ticks, data));
        }
        bytes
    }
    fn beacon(ssid: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x80, 0x00, 0x00, 0x00];
        bytes.extend_from_slice(&[0xff; 6]);
        bytes.extend_from_slice(&[0x02, 1, 2, 3, 4, 5]);
        bytes.extend_from_slice(&[0x02, 1, 2, 3, 4, 5]);
        bytes.extend_from_slice(&[0x10, 0x00]);
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&[100, 0, 0x01, 0x00]);
        bytes.push(0);
        bytes.push(ssid.len() as u8);
        bytes.extend_from_slice(ssid);
        bytes
    }
    fn fcs(bytes: &[u8]) -> [u8; 4] {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        (!crc).to_le_bytes()
    }
    fn limits() -> Limits {
        Limits::default()
    }
    fn run(bytes: Vec<u8>, framing: InputFraming, limits: Limits) -> Result<ReplayCapture, Error> {
        replay(Cursor::new(bytes), framing, limits, &AtomicBool::new(false))
    }

    #[test]
    fn parses_real_pcapng_management_ie_and_preserves_provenance_in_order() {
        let raw = beacon(b"lab");
        let bytes = capture(&[(105, 4096)], &[(0, 7_000_000, raw.clone())]);
        let result = run(bytes, InputFraming::FcsAbsent, limits()).unwrap();
        assert_eq!(result.replay_version(), "kyberia-pcap-ie-replay/0.1.0");
        assert_eq!(result.parser_version(), "kyberia-ieee80211/0.1.0");
        assert_eq!(result.receipt().packets, 1);
        assert_eq!(result.dispositions().parsed_management_frames(), 1);
        let replayed = &result.frames()[0];
        assert_eq!(replayed.frame().raw_mpdu(), raw);
        assert_eq!(replayed.frame().subtype(), ManagementSubtype::Beacon);
        assert_eq!(replayed.frame().elements()[0].payload(), b"lab");
        assert_eq!(replayed.provenance().packet_index(), 0);
        assert_eq!(replayed.provenance().section_index(), 0);
        assert_eq!(replayed.provenance().interface_index(), 0);
        assert_eq!(replayed.provenance().link_type(), 105);
        let Some(ReportedTimestamp { raw_ticks, .. }) =
            replayed.provenance().timestamp().as_known()
        else {
            panic!("enhanced packet timestamp must remain known")
        };
        assert_eq!(*raw_ticks, 7_000_000);
        assert!(result.retained_logical_bytes() > 0);
    }

    #[test]
    fn unsupported_linktypes_including_radiotap_are_counted_not_guessed() {
        let bytes = capture(&[(127, 4096)], &[(0, 0, vec![0, 0, 0, 0])]);
        let result = run(bytes, InputFraming::FcsAbsent, limits()).unwrap();
        assert!(result.frames().is_empty());
        assert_eq!(result.dispositions().skipped_unsupported_link_type(), 1);
    }

    #[test]
    fn explicit_fcs_policy_accepts_valid_and_rejects_invalid_fcs() {
        let mut valid = beacon(b"fcs");
        valid.extend_from_slice(&fcs(&valid));
        let expected_raw = valid.clone();
        let valid_capture = capture(&[(105, 4096)], &[(0, 0, valid.clone())]);
        let replayed = run(valid_capture, InputFraming::FcsPresentAndValidate, limits()).unwrap();
        assert_eq!(replayed.frames()[0].frame().raw_mpdu(), expected_raw);

        let final_octet = valid.len() - 1;
        valid[final_octet] ^= 1;
        let invalid_capture = capture(&[(105, 4096)], &[(0, 0, valid)]);
        assert!(matches!(
            run(
                invalid_capture,
                InputFraming::FcsPresentAndValidate,
                limits()
            ),
            Err(Error::MalformedSupportedFrame {
                source: IeeeError::InvalidFcs { .. },
                ..
            })
        ));
    }

    #[test]
    fn counts_nonmanagement_unsupported_subtype_and_protocol_version() {
        let nonmanagement = vec![0x08, 0];
        let unsupported_subtype = vec![0xb0, 0];
        let unsupported_version = vec![0x81, 0];
        let bytes = capture(
            &[(105, 4096)],
            &[
                (0, 1, nonmanagement),
                (0, 2, unsupported_subtype),
                (0, 3, unsupported_version),
            ],
        );
        let result = run(bytes, InputFraming::FcsAbsent, limits()).unwrap();
        assert_eq!(result.dispositions().packets(), 3);
        assert_eq!(result.dispositions().skipped_non_management(), 1);
        assert_eq!(
            result
                .dispositions()
                .skipped_unsupported_management_subtype(),
            1
        );
        assert_eq!(
            result.dispositions().skipped_unsupported_protocol_version(),
            1
        );
    }

    #[test]
    fn malformed_supported_frames_fail_without_returning_staged_prefix() {
        let good = beacon(b"ok");
        let malformed = vec![0x80, 0];
        let bytes = capture(&[(105, 4096)], &[(0, 1, good), (0, 2, malformed)]);
        assert!(matches!(
            run(bytes, InputFraming::FcsAbsent, limits()),
            Err(Error::MalformedSupportedFrame {
                packet_index: 1,
                ..
            })
        ));
    }

    #[test]
    fn malformed_container_tail_prevents_publishing_a_valid_packet_prefix() {
        let mut bytes = capture(&[(105, 4096)], &[(0, 1, beacon(b"valid-prefix"))]);
        bytes.extend_from_slice(&u32le(1));
        bytes.extend_from_slice(&u32le(12));
        assert!(matches!(
            run(bytes, InputFraming::FcsAbsent, limits()),
            Err(Error::Input(pcapng::Error::Truncated))
        ));
    }

    #[test]
    fn frame_count_and_retained_byte_limits_have_exact_acceptance_boundaries() {
        let raw = beacon(b"x");
        let one = capture(&[(105, 4096)], &[(0, 1, raw.clone())]);
        let mut one_frame_limit = limits();
        one_frame_limit.max_frames = 1;
        assert_eq!(
            run(one, InputFraming::FcsAbsent, one_frame_limit)
                .unwrap()
                .frames()
                .len(),
            1
        );

        let two = capture(&[(105, 4096)], &[(0, 1, raw.clone()), (0, 2, raw)]);
        assert!(matches!(
            run(two, InputFraming::FcsAbsent, one_frame_limit),
            Err(Error::ResourceLimit("parsed frame count"))
        ));

        let probe = run(
            capture(&[(105, 4096)], &[(0, 1, beacon(b"x"))]),
            InputFraming::FcsAbsent,
            limits(),
        )
        .unwrap();
        let exact = probe.retained_logical_bytes();
        let mut exact_limit = limits();
        exact_limit.max_retained_bytes = exact;
        assert!(
            run(
                capture(&[(105, 4096)], &[(0, 1, beacon(b"x"))]),
                InputFraming::FcsAbsent,
                exact_limit,
            )
            .is_ok()
        );
        exact_limit.max_retained_bytes = exact - 1;
        assert!(matches!(
            run(
                capture(&[(105, 4096)], &[(0, 1, beacon(b"x"))]),
                InputFraming::FcsAbsent,
                exact_limit,
            ),
            Err(Error::ResourceLimit("retained allocation bytes"))
        ));
    }

    #[test]
    fn total_parse_work_limit_has_an_exact_boundary() {
        let bytes = capture(&[(105, 4096)], &[(0, 1, beacon(b"work"))]);
        let measured = run(bytes.clone(), InputFraming::FcsAbsent, limits()).unwrap();
        let exact = measured.total_parse_work_units();
        let mut exact_limit = limits();
        exact_limit.max_total_parse_work_units = exact;
        assert_eq!(
            run(bytes.clone(), InputFraming::FcsAbsent, exact_limit)
                .unwrap()
                .total_parse_work_units(),
            exact
        );
        exact_limit.max_total_parse_work_units = exact - 1;
        assert!(matches!(
            run(bytes, InputFraming::FcsAbsent, exact_limit),
            Err(Error::ResourceLimit("total parser work"))
        ));
    }

    #[test]
    fn input_packet_limit_stops_and_does_not_publish_a_prefix() {
        let mut small = limits();
        small.import.max_packets = 1;
        let bytes = capture(
            &[(105, 4096)],
            &[(0, 1, beacon(b"one")), (0, 2, beacon(b"two"))],
        );
        assert!(matches!(
            run(bytes, InputFraming::FcsAbsent, small),
            Err(Error::Input(pcapng::Error::ResourceLimit))
        ));
    }

    #[test]
    fn cancellation_returns_no_capture() {
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            replay(
                Cursor::new(capture(&[(105, 4096)], &[(0, 1, beacon(b"x"))])),
                InputFraming::FcsAbsent,
                limits(),
                &cancelled,
            ),
            Err(Error::Cancelled)
        ));
    }

    struct CancelDuringRead<'a> {
        inner: Cursor<Vec<u8>>,
        cancel: &'a AtomicBool,
    }
    impl Read for CancelDuringRead<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let result = self.inner.read(output)?;
            if result != 0 {
                self.cancel.store(true, Ordering::Relaxed);
            }
            Ok(result)
        }
    }

    #[test]
    fn cancellation_arriving_from_the_reader_prevents_publication() {
        let cancel = AtomicBool::new(false);
        let input = CancelDuringRead {
            inner: Cursor::new(capture(&[(105, 4096)], &[(0, 1, beacon(b"x"))])),
            cancel: &cancel,
        };
        assert!(matches!(
            replay(input, InputFraming::FcsAbsent, limits(), &cancel),
            Err(Error::Cancelled)
        ));
    }

    struct CancelAfterOffset<'a> {
        inner: Cursor<Vec<u8>>,
        offset: u64,
        cancel: &'a AtomicBool,
        triggered: &'a AtomicBool,
    }
    impl Read for CancelAfterOffset<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.inner.position() >= self.offset && !self.triggered.load(Ordering::Relaxed) {
                self.triggered.store(true, Ordering::Relaxed);
                self.cancel.store(true, Ordering::Relaxed);
            }
            self.inner.read(output)
        }
    }

    #[test]
    fn cancellation_after_staging_one_valid_frame_returns_no_capture() {
        let first_packet = capture(&[(105, 4096)], &[(0, 1, beacon(b"staged"))]);
        let after_first_packet = first_packet.len() as u64;
        let mut bytes = first_packet;
        bytes.extend_from_slice(&epb(0, 2, &beacon(b"not-reached")));

        let cancel = AtomicBool::new(false);
        let triggered = AtomicBool::new(false);
        let input = CancelAfterOffset {
            inner: Cursor::new(bytes),
            offset: after_first_packet,
            cancel: &cancel,
            triggered: &triggered,
        };
        let result = replay(input, InputFraming::FcsAbsent, limits(), &cancel);

        assert!(triggered.load(Ordering::Relaxed));
        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[test]
    fn deterministic_order_and_interface_identity_survive_multiple_interfaces() {
        let bytes = capture(
            &[(127, 4096), (105, 4096), (105, 4096)],
            &[
                (1, 8_000_000, beacon(b"first")),
                (2, 9_000_000, beacon(b"second")),
            ],
        );
        let result = run(bytes, InputFraming::FcsAbsent, limits()).unwrap();
        assert_eq!(result.frames()[0].provenance().packet_index(), 0);
        assert_eq!(result.frames()[0].provenance().interface_index(), 1);
        assert_eq!(result.frames()[1].provenance().packet_index(), 1);
        assert_eq!(result.frames()[1].provenance().interface_index(), 2);
        assert_eq!(result.frames()[0].frame().elements()[0].payload(), b"first");
        assert_eq!(
            result.frames()[1].frame().elements()[0].payload(),
            b"second"
        );
    }

    #[test]
    fn geometric_staging_preserves_order_across_capacity_growth() {
        let packets: Vec<_> = (0..33)
            .map(|index| (0, index, beacon(&[index as u8])))
            .collect();
        let result = run(
            capture(&[(105, 4096)], &packets),
            InputFraming::FcsAbsent,
            limits(),
        )
        .unwrap();
        assert_eq!(result.frames().len(), 33);
        for (index, frame) in result.frames().iter().enumerate() {
            assert_eq!(frame.provenance().packet_index(), index as u64);
            assert_eq!(frame.frame().elements()[0].payload(), &[index as u8]);
        }
        assert!(result.retained_logical_bytes() < limits().max_retained_bytes);
    }
}
