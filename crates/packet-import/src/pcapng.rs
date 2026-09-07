//! Strict streaming framing around the adopted pcap-parser implementation.
//! Callers stage borrowed packet evidence until a complete stream receipt exists.
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    time::UtcTimestamp,
};
use pcap_parser::{Block, PcapNGOption, parse_block_be, parse_block_le};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Io,
    Truncated,
    Malformed,
    UnsupportedVersion,
    ResourceLimit,
    Cancelled,
    Deadline,
    ConsumerStopped,
    TimestampOverflow,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PCAPNG import: {self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_bytes: u64,
    pub max_blocks: u64,
    pub max_packets: u64,
    pub max_block_bytes: usize,
    pub max_interfaces_per_section: usize,
    pub max_sections: u32,
    pub timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024 * 1024,
            max_blocks: 10_000_000,
            max_packets: 10_000_000,
            max_block_bytes: 1024 * 1024,
            max_interfaces_per_section: 1024,
            max_sections: 1024,
            timeout: Duration::from_secs(120),
        }
    }
}
impl Limits {
    fn validate(self) -> Result<(), Error> {
        let cap = Self::default();
        if self.max_bytes < 28
            || self.max_bytes > cap.max_bytes
            || self.max_blocks == 0
            || self.max_blocks > cap.max_blocks
            || self.max_packets == 0
            || self.max_packets > cap.max_packets
            || !(28..=cap.max_block_bytes).contains(&self.max_block_bytes)
            || !(1..=cap.max_interfaces_per_section).contains(&self.max_interfaces_per_section)
            || self.max_sections == 0
            || self.max_sections > cap.max_sections
            || self.timeout.is_zero()
            || self.timeout > cap.timeout
        {
            return Err(Error::ResourceLimit);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    Little,
    Big,
}
impl ByteOrder {
    fn u32(self, b: &[u8]) -> u32 {
        let a = b.try_into().expect("validated four-byte field");
        match self {
            Self::Little => u32::from_le_bytes(a),
            Self::Big => u32::from_be_bytes(a),
        }
    }
    fn u64(self, b: &[u8]) -> u64 {
        let a = b.try_into().expect("validated eight-byte field");
        match self {
            Self::Little => u64::from_le_bytes(a),
            Self::Big => u64::from_be_bytes(a),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub section_index: u32,
    pub interface_index: u32,
    pub link_type: u32,
    pub snaplen: u32,
    pub timestamp_resolution: u8,
    pub timestamp_offset_seconds: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportedTimestamp {
    pub raw_ticks: u64,
    pub resolution: u8,
    pub offset_seconds: i64,
    /// Rounded downward when raw precision exceeds canonical nanoseconds.
    pub utc_nanoseconds: UtcTimestamp,
    pub subnanosecond_remainder: bool,
}
fn timestamp(ticks: u64, interface: &Interface) -> Result<ReportedTimestamp, Error> {
    let r = interface.timestamp_resolution;
    let numerator = u128::from(ticks) * 1_000_000_000;
    let divisor = if r & 128 != 0 {
        Some(1u128 << (r & 127))
    } else {
        10u128.checked_pow(u32::from(r))
    };
    let (nanos, remainder) = match divisor {
        Some(d) => (numerator / d, numerator % d != 0),
        None => (0, numerator != 0),
    };
    let total = nanos as i128 + i128::from(interface.timestamp_offset_seconds) * 1_000_000_000;
    Ok(ReportedTimestamp {
        raw_ticks: ticks,
        resolution: r,
        offset_seconds: interface.timestamp_offset_seconds,
        utc_nanoseconds: UtcTimestamp(total.try_into().map_err(|_| Error::TimestampOverflow)?),
        subnanosecond_remainder: remainder,
    })
}

#[derive(Debug)]
pub struct Packet<'a> {
    pub block_offset: u64,
    pub interface: &'a Interface,
    pub timestamp: Evidence<ReportedTimestamp>,
    pub original_length: u32,
    pub captured_length: u32,
    /// Source discrepancy remains visible; no silent length correction.
    pub contradictory_original_length: bool,
    pub flags: Evidence<u32>,
    pub dropped_since_previous: Evidence<u64>,
    pub correlation_packet_id: Evidence<u64>,
    /// Borrowed bytes only, no persistence or identifier logging in this adapter.
    pub data: &'a [u8],
}

#[derive(Debug)]
pub struct Receipt {
    pub decoder_version: &'static str,
    pub sha256: [u8; 32],
    pub byte_length: u64,
    pub blocks: u64,
    pub packets: u64,
    pub sections: u32,
    pub skipped_blocks: u64,
}

fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}
fn check(deadline: Instant, cancel: &AtomicBool) -> Result<(), Error> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    Ok(())
}
fn read_part<R: Read>(
    reader: &mut R,
    bytes: &mut [u8],
    allow_empty: bool,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<bool, Error> {
    let mut used = 0;
    while used < bytes.len() {
        check(deadline, cancel)?;
        let result = reader.read(&mut bytes[used..]);
        // A read may finish after cancellation or its deadline, including EOF.
        check(deadline, cancel)?;
        match result {
            Ok(0) if used == 0 && allow_empty => return Ok(false),
            Ok(0) => return Err(Error::Truncated),
            Ok(n) => used += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error::Io),
        }
    }
    Ok(true)
}

// The adopted parser intentionally tolerates unparsed option tails. Kyberia
// requires complete bounded option framing so malformed timestamp metadata
// cannot silently disappear and select a default resolution or offset.
fn options<'a>(values: &'a [PcapNGOption<'a>], expected_bytes: usize) -> Result<(), Error> {
    if values.len() > 256 {
        return Err(Error::ResourceLimit);
    }
    let mut consumed = 0;
    for (index, opt) in values.iter().enumerate() {
        opt.as_bytes().map_err(|_| Error::Malformed)?;
        consumed += 4 + usize::from(opt.len).div_ceil(4) * 4;
        if opt.code.0 == 0 && (opt.len != 0 || index + 1 != values.len()) {
            return Err(Error::Malformed);
        }
    }
    if consumed != expected_bytes {
        return Err(Error::Malformed);
    }
    Ok(())
}
fn single<'a>(
    opts: &'a [PcapNGOption<'a>],
    code: u16,
    len: usize,
) -> Result<Option<&'a [u8]>, Error> {
    let mut found = None;
    for opt in opts.iter().filter(|o| o.code.0 == code) {
        if found.is_some() || usize::from(opt.len) != len {
            return Err(Error::Malformed);
        }
        found = Some(opt.as_bytes().map_err(|_| Error::Malformed)?);
    }
    Ok(found)
}

/// Parse exactly one finite PCAPNG stream. `false` from the consumer cancels
/// publication and returns no success receipt. No partial stream hash is final.
/// Kernel I/O can block; applications require process isolation for hard timeouts.
pub fn read<R: Read, F>(
    mut input: R,
    limits: Limits,
    cancel: &AtomicBool,
    mut consume: F,
) -> Result<Receipt, Error>
where
    F: FnMut(Packet<'_>) -> bool,
{
    limits.validate()?;
    let deadline = Instant::now() + limits.timeout;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut blocks = 0u64;
    let mut packets = 0u64;
    let mut sections = 0u32;
    let mut skipped = 0u64;
    let mut order = None;
    let mut interfaces = Vec::<Interface>::new();
    let mut section_end = None;
    loop {
        check(deadline, cancel)?;
        let mut header = [0u8; 12];
        if !read_part(&mut input, &mut header[..8], true, deadline, cancel)? {
            if sections == 0 {
                return Err(Error::Truncated);
            }
            if section_end.is_some_and(|end| end != bytes) {
                return Err(Error::Truncated);
            }
            let receipt = Receipt {
                decoder_version: "kyberia-pcapng/1.0.0",
                sha256: hash.finalize().into(),
                byte_length: bytes,
                blocks,
                packets,
                sections,
                skipped_blocks: skipped,
            };
            check(deadline, cancel)?;
            return Ok(receipt);
        }
        let is_section = header[..4] == [0x0a, 0x0d, 0x0d, 0x0a];
        let prefix = if is_section {
            if section_end.is_some_and(|end| end != bytes) {
                return Err(Error::Malformed);
            }
            read_part(&mut input, &mut header[8..], false, deadline, cancel)?;
            order = Some(match &header[8..] {
                [0x4d, 0x3c, 0x2b, 0x1a] => ByteOrder::Little,
                [0x1a, 0x2b, 0x3c, 0x4d] => ByteOrder::Big,
                _ => return Err(Error::Malformed),
            });
            12
        } else {
            8
        };
        let order = order.ok_or(Error::Malformed)?;
        let size = order.u32(&header[4..8]) as usize;
        if size < 12 || !size.is_multiple_of(4) {
            return Err(Error::Malformed);
        }
        if size > limits.max_block_bytes
            || bytes
                .checked_add(size as u64)
                .is_none_or(|n| n > limits.max_bytes)
            || blocks >= limits.max_blocks
        {
            return Err(Error::ResourceLimit);
        }
        if !is_section && section_end.is_some_and(|end| bytes + size as u64 > end) {
            return Err(Error::Malformed);
        }
        let mut buffer = vec![0u8; size];
        buffer[..prefix].copy_from_slice(&header[..prefix]);
        read_part(&mut input, &mut buffer[prefix..], false, deadline, cancel)?;
        if order.u32(&buffer[size - 4..]) != size as u32 {
            return Err(Error::Malformed);
        }
        let parsed = match order {
            ByteOrder::Little => parse_block_le(&buffer),
            ByteOrder::Big => parse_block_be(&buffer),
        }
        .map_err(|_| Error::Malformed)?;
        if !parsed.0.is_empty() {
            return Err(Error::Malformed);
        }
        blocks += 1;
        match parsed.1 {
            Block::SectionHeader(s) => {
                if s.major_version != 1 || s.minor_version != 0 {
                    return Err(Error::UnsupportedVersion);
                }
                if sections >= limits.max_sections {
                    return Err(Error::ResourceLimit);
                }
                options(&s.options, size - 28)?;
                section_end = match s.section_len {
                    -1 => None,
                    n if n >= 0 => Some(
                        bytes
                            .checked_add(size as u64)
                            .and_then(|b| b.checked_add(n as u64))
                            .ok_or(Error::ResourceLimit)?,
                    ),
                    _ => return Err(Error::Malformed),
                };
                sections += 1;
                interfaces.clear();
            }
            Block::InterfaceDescription(i) => {
                if interfaces.len() >= limits.max_interfaces_per_section {
                    return Err(Error::ResourceLimit);
                }
                options(&i.options, size - 20)?;
                let resolution = single(&i.options, 9, 1)?.map_or(6, |b| b[0]);
                let offset = single(&i.options, 14, 8)?.map_or(0, |b| order.u64(b) as i64);
                interfaces.push(Interface {
                    section_index: sections - 1,
                    interface_index: interfaces.len() as u32,
                    link_type: i.linktype.0.try_into().map_err(|_| Error::Malformed)?,
                    snaplen: i.snaplen,
                    timestamp_resolution: resolution,
                    timestamp_offset_seconds: offset,
                });
            }
            Block::EnhancedPacket(p) => {
                let interface = interfaces.get(p.if_id as usize).ok_or(Error::Malformed)?;
                if p.caplen as usize > p.data.len()
                    || (interface.snaplen != 0 && p.caplen > interface.snaplen)
                {
                    return Err(Error::Malformed);
                }
                let padded = (p.caplen as usize).div_ceil(4) * 4;
                options(
                    &p.options,
                    size.checked_sub(32 + padded).ok_or(Error::Malformed)?,
                )?;
                if packets >= limits.max_packets {
                    return Err(Error::ResourceLimit);
                }
                let flags = single(&p.options, 2, 4)?
                    .map_or_else(unknown, |b| Evidence::Known(order.u32(b)));
                let drops = single(&p.options, 4, 8)?
                    .map_or_else(unknown, |b| Evidence::Known(order.u64(b)));
                let id = single(&p.options, 5, 8)?
                    .map_or_else(unknown, |b| Evidence::Known(order.u64(b)));
                let packet = Packet {
                    block_offset: bytes,
                    interface,
                    timestamp: Evidence::Known(timestamp(
                        (u64::from(p.ts_high) << 32) | u64::from(p.ts_low),
                        interface,
                    )?),
                    original_length: p.origlen,
                    captured_length: p.caplen,
                    contradictory_original_length: p.origlen < p.caplen,
                    flags,
                    dropped_since_previous: drops,
                    correlation_packet_id: id,
                    data: &p.data[..p.caplen as usize],
                };
                if !consume(packet) {
                    return Err(Error::ConsumerStopped);
                }
                packets += 1;
            }
            Block::SimplePacket(p) => {
                let interface = interfaces.first().ok_or(Error::Malformed)?;
                let captured = if interface.snaplen == 0 {
                    p.origlen
                } else {
                    p.origlen.min(interface.snaplen)
                };
                if captured as usize > p.data.len()
                    || (captured as usize).div_ceil(4) * 4 != p.data.len()
                {
                    return Err(Error::Malformed);
                }
                if packets >= limits.max_packets {
                    return Err(Error::ResourceLimit);
                }
                let packet = Packet {
                    block_offset: bytes,
                    interface,
                    timestamp: unknown(),
                    original_length: p.origlen,
                    captured_length: captured,
                    contradictory_original_length: false,
                    flags: unknown(),
                    dropped_since_previous: unknown(),
                    correlation_packet_id: unknown(),
                    data: &p.data[..captured as usize],
                };
                if !consume(packet) {
                    return Err(Error::ConsumerStopped);
                }
                packets += 1;
            }
            _ => skipped += 1,
        }
        hash.update(&buffer);
        bytes += size as u64;
    }
}
