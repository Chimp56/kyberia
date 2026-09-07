use kyberia_domain::evidence::Evidence;
use kyberia_packet_import::pcapng::{Error, Limits, read};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Copy)]
struct Order(bool);
impl Order {
    fn u16(self, n: u16) -> [u8; 2] {
        if self.0 {
            n.to_be_bytes()
        } else {
            n.to_le_bytes()
        }
    }
    fn u32(self, n: u32) -> [u8; 4] {
        if self.0 {
            n.to_be_bytes()
        } else {
            n.to_le_bytes()
        }
    }
    fn u64(self, n: u64) -> [u8; 8] {
        if self.0 {
            n.to_be_bytes()
        } else {
            n.to_le_bytes()
        }
    }
}
fn block(o: Order, kind: u32, body: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let len = (body.len() + 12) as u32;
    result.extend(o.u32(kind));
    result.extend(o.u32(len));
    result.extend(body);
    result.extend(o.u32(len));
    result
}
fn section(o: Order) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(o.u32(0x1a2b3c4d));
    body.extend(o.u16(1));
    body.extend(o.u16(0));
    body.extend(o.u64(u64::MAX));
    block(o, 0x0a0d0d0a, &body)
}
fn option(o: Order, kind: u16, value: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(o.u16(kind));
    b.extend(o.u16(value.len() as u16));
    b.extend(value);
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
    b
}
fn interface(o: Order, resolution: u8, offset: i64, snaplen: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(o.u16(127));
    b.extend(o.u16(0));
    b.extend(o.u32(snaplen));
    b.extend(option(o, 9, &[resolution]));
    b.extend(option(o, 14, &o.u64(offset as u64)));
    b.extend(option(o, 0, &[]));
    block(o, 1, &b)
}
fn packet(o: Order, id: u32, ticks: u64, data: &[u8], original: u32, opts: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [
        id,
        (ticks >> 32) as u32,
        ticks as u32,
        data.len() as u32,
        original,
    ] {
        b.extend(o.u32(v));
    }
    b.extend(data);
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
    b.extend(opts);
    block(o, 6, &b)
}
fn fixture(o: Order) -> Vec<u8> {
    let mut b = section(o);
    b.extend(interface(o, 6, 0, 65535));
    b.extend(packet(o, 0, 1_700_000_000_123_456, &[1, 2, 3], 3, &[]));
    b
}
fn parse(bytes: &[u8]) -> Result<u64, Error> {
    read(bytes, Limits::default(), &AtomicBool::new(false), |_| true).map(|r| r.packets)
}

#[test]
fn little_and_big_endian_timestamps_payload_padding_and_hashes_agree() {
    for o in [Order(false), Order(true)] {
        let data = fixture(o);
        let mut count = 0;
        let receipt = read(
            data.as_slice(),
            Limits::default(),
            &AtomicBool::new(false),
            |p| {
                count += 1;
                assert_eq!(p.data, [1, 2, 3]);
                assert_eq!(p.captured_length, 3);
                assert_eq!(p.original_length, 3);
                assert_eq!(p.interface.link_type, 127);
                assert_eq!(
                    p.timestamp.as_known().unwrap().utc_nanoseconds.0,
                    1_700_000_000_123_456_000
                );
                assert!(matches!(p.dropped_since_previous, Evidence::Unknown(_)));
                true
            },
        )
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(receipt.sha256, <[u8; 32]>::from(Sha256::digest(&data)));
        assert_eq!(receipt.byte_length, data.len() as u64);
    }
}

#[test]
fn sections_reset_interfaces_and_timestamp_options_obey_section_endianness() {
    let mut data = Vec::new();
    for o in [Order(false), Order(true)] {
        data.extend(section(o));
        data.extend(interface(o, 0x80 | 10, -2, 100));
        let mut opts = option(o, 4, &o.u64(7));
        opts.extend(option(o, 5, &o.u64(99)));
        opts.extend(option(o, 2, &o.u32(1)));
        opts.extend(option(o, 0, &[]));
        data.extend(packet(o, 0, 2560, &[0, 1], 2, &opts));
    }
    let mut sections = Vec::new();
    let receipt = read(
        data.as_slice(),
        Limits::default(),
        &AtomicBool::new(false),
        |p| {
            sections.push(p.interface.section_index);
            assert_eq!(p.interface.interface_index, 0);
            assert_eq!(
                p.timestamp.as_known().unwrap().utc_nanoseconds.0,
                500_000_000
            );
            assert_eq!(p.dropped_since_previous, Evidence::Known(7));
            assert_eq!(p.correlation_packet_id, Evidence::Known(99));
            assert_eq!(p.flags, Evidence::Known(1));
            true
        },
    )
    .unwrap();
    assert_eq!(sections, vec![0, 1]);
    assert_eq!(receipt.sections, 2);
    let mut broken = fixture(Order(false));
    broken.extend(section(Order(true)));
    broken.extend(packet(Order(true), 0, 1, &[1], 1, &[]));
    assert_eq!(parse(&broken), Err(Error::Malformed));
}

#[test]
fn subnanosecond_precision_and_extreme_resolution_retain_exact_raw_time() {
    for resolution in [10, 127, 128 | 127] {
        let o = Order(false);
        let mut data = section(o);
        data.extend(interface(o, resolution, -1, 100));
        data.extend(packet(o, 0, 1, &[0], 1, &[]));
        read(
            data.as_slice(),
            Limits::default(),
            &AtomicBool::new(false),
            |p| {
                let t = p.timestamp.as_known().unwrap();
                assert_eq!(t.raw_ticks, 1);
                assert_eq!(t.resolution, resolution);
                assert_eq!(t.utc_nanoseconds.0, -1_000_000_000);
                assert!(t.subnanosecond_remainder);
                true
            },
        )
        .unwrap();
    }
}

#[test]
fn simple_packets_have_unknown_time_and_snaplen_limits_the_actual_bytes() {
    for o in [Order(false), Order(true)] {
        let mut data = section(o);
        data.extend(interface(o, 6, 0, 3));
        let mut body = o.u32(10).to_vec();
        body.extend([1, 2, 3, 0]);
        data.extend(block(o, 3, &body));
        read(
            data.as_slice(),
            Limits::default(),
            &AtomicBool::new(false),
            |p| {
                assert!(matches!(p.timestamp, Evidence::Unknown(_)));
                assert_eq!(p.data, [1, 2, 3]);
                assert_eq!(p.original_length, 10);
                assert_eq!(p.captured_length, 3);
                true
            },
        )
        .unwrap();
    }
}

#[test]
fn contradictory_original_length_is_preserved_and_visibly_flagged() {
    let o = Order(false);
    let mut data = section(o);
    data.extend(interface(o, 6, 0, 100));
    data.extend(packet(o, 0, 1, &[1, 2, 3], 1, &[]));
    read(
        data.as_slice(),
        Limits::default(),
        &AtomicBool::new(false),
        |p| {
            assert_eq!(p.original_length, 1);
            assert!(p.contradictory_original_length);
            true
        },
    )
    .unwrap();
}

#[test]
fn malformed_options_cannot_disappear_into_timestamp_defaults() {
    for o in [Order(false), Order(true)] {
        let mut body = Vec::new();
        body.extend(o.u16(127));
        body.extend(o.u16(0));
        body.extend(o.u32(100));
        for bad in [
            vec![9, 0, 255, 255],
            option(o, 9, &[6, 0]),
            [option(o, 9, &[6]), option(o, 9, &[9])].concat(),
            [option(o, 0, &[]), option(o, 9, &[6])].concat(),
        ] {
            let mut data = section(o);
            data.extend(block(o, 1, &[body.clone(), bad].concat()));
            assert_eq!(parse(&data), Err(Error::Malformed));
        }
        let mut data = section(o);
        data.extend(interface(o, 6, 0, 100));
        data.extend(packet(
            o,
            0,
            1,
            &[0],
            1,
            &[option(o, 4, &o.u64(1)), option(o, 4, &o.u64(2))].concat(),
        ));
        assert_eq!(parse(&data), Err(Error::Malformed));
    }
}

#[test]
fn truncated_or_mismatched_frames_never_return_success_receipts() {
    let valid = fixture(Order(false));
    for n in 0..valid.len() {
        // Exact complete-section/interface boundaries are valid empty captures.
        if n == 28 || n == 72 {
            continue;
        }
        assert!(parse(&valid[..n]).is_err(), "accepted prefix {n}");
    }
    let mut wrong = valid.clone();
    wrong[24] = 0;
    assert_eq!(parse(&wrong), Err(Error::Malformed));
    let mut wrong = valid.clone();
    wrong[12] = 2;
    assert_eq!(parse(&wrong), Err(Error::UnsupportedVersion));
    let mut wrong = valid.clone();
    wrong[4..8].copy_from_slice(&12u32.to_le_bytes());
    assert!(parse(&wrong).is_err());
    let mut wrong = valid;
    wrong[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(&wrong).is_err());
}

#[test]
fn explicit_section_lengths_must_match_boundaries_and_eof() {
    let mut valid = fixture(Order(false));
    let remaining = valid.len() as u64 - 28;
    valid[16..24].copy_from_slice(&remaining.to_le_bytes());
    assert_eq!(parse(&valid), Ok(1));
    for delta in [-4i64, 4] {
        let mut bad = valid.clone();
        bad[16..24].copy_from_slice(&(remaining as i64 + delta).to_le_bytes());
        assert!(parse(&bad).is_err());
    }
    let mut bad = valid;
    bad[16..24].copy_from_slice(&(-2i64).to_le_bytes());
    assert!(parse(&bad).is_err());
}

#[test]
fn cancellation_consumer_stop_and_limits_prevent_success() {
    let data = fixture(Order(false));
    let cancel = AtomicBool::new(true);
    assert_eq!(
        read(data.as_slice(), Limits::default(), &cancel, |_| true).unwrap_err(),
        Error::Cancelled
    );
    cancel.store(false, Ordering::Relaxed);
    assert_eq!(
        read(data.as_slice(), Limits::default(), &cancel, |_| false).unwrap_err(),
        Error::ConsumerStopped
    );
    let limits = Limits {
        max_bytes: 28,
        ..Limits::default()
    };
    assert_eq!(
        read(data.as_slice(), limits, &cancel, |_| true).unwrap_err(),
        Error::ResourceLimit
    );
    let limits = Limits {
        timeout: Duration::from_nanos(1),
        ..Limits::default()
    };
    assert_eq!(
        read(data.as_slice(), limits, &cancel, |_| true).unwrap_err(),
        Error::Deadline
    );
    let limits = Limits {
        max_interfaces_per_section: 0,
        ..Limits::default()
    };
    assert_eq!(
        read(data.as_slice(), limits, &cancel, |_| true).unwrap_err(),
        Error::ResourceLimit
    );
}

struct ShortReads<'a>(&'a [u8]);

struct EofAction<'a> {
    bytes: &'a [u8],
    cancel_at_eof: Option<&'a AtomicBool>,
    delay: Duration,
    reached_eof: &'a AtomicBool,
}
impl Read for EofAction<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.bytes.read(buffer)?;
        if count == 0 {
            self.reached_eof.store(true, Ordering::Relaxed);
            if let Some(cancel) = self.cancel_at_eof {
                cancel.store(true, Ordering::Relaxed);
            }
            std::thread::sleep(self.delay);
        }
        Ok(count)
    }
}

#[test]
fn cancellation_during_final_eof_read_invalidates_the_receipt() {
    let bytes = fixture(Order(false));
    let cancel = AtomicBool::new(false);
    let eof = AtomicBool::new(false);
    let reader = EofAction {
        bytes: &bytes,
        cancel_at_eof: Some(&cancel),
        delay: Duration::ZERO,
        reached_eof: &eof,
    };
    let result = read(reader, Limits::default(), &cancel, |_| true);
    assert!(eof.load(Ordering::Relaxed));
    assert_eq!(result.unwrap_err(), Error::Cancelled);
}

#[test]
fn deadline_during_final_eof_read_invalidates_the_receipt() {
    let bytes = fixture(Order(false));
    let cancel = AtomicBool::new(false);
    let eof = AtomicBool::new(false);
    let reader = EofAction {
        bytes: &bytes,
        cancel_at_eof: None,
        delay: Duration::from_millis(110),
        reached_eof: &eof,
    };
    let limits = Limits {
        timeout: Duration::from_millis(100),
        ..Limits::default()
    };
    let result = read(reader, limits, &cancel, |_| true);
    assert!(eof.load(Ordering::Relaxed));
    assert_eq!(result.unwrap_err(), Error::Deadline);
}

impl Read for ShortReads<'_> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let n = b.len().min(1).min(self.0.len());
        b[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}
#[test]
fn short_reads_work_and_unknown_blocks_are_explicitly_counted() {
    let mut data = fixture(Order(false));
    data.extend(block(Order(false), 0x76543210, &[0, 1, 2, 3]));
    let receipt = read(
        ShortReads(&data),
        Limits::default(),
        &AtomicBool::new(false),
        |_| true,
    )
    .unwrap();
    assert_eq!(receipt.packets, 1);
    assert_eq!(receipt.skipped_blocks, 1);
}

#[test]
fn deterministic_mutation_fuzz_has_no_panics_or_unbounded_allocations() {
    let base = fixture(Order(false));
    let mut state = 19u32;
    for _ in 0..2048 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let index = state as usize % base.len();
        let mut data = base.clone();
        data[index] = (state >> 24) as u8;
        let _ = parse(&data);
    }
}

#[test]
fn extreme_simple_length_and_overflowing_timestamp_are_rejected() {
    let o = Order(false);
    let mut data = section(o);
    data.extend(interface(o, 6, 0, 0));
    data.extend(block(o, 3, &o.u32(u32::MAX)));
    assert_eq!(parse(&data), Err(Error::Malformed));
    let mut data = section(o);
    data.extend(interface(o, 0, i64::MAX, 100));
    data.extend(packet(o, 0, u64::MAX, &[0], 1, &[]));
    assert_eq!(parse(&data), Err(Error::TimestampOverflow));
}

#[test]
fn trailing_error_invalidates_a_previously_delivered_packet_prefix() {
    let mut data = fixture(Order(false));
    data.extend([1, 2, 3]);
    let mut delivered = 0;
    let result = read(
        data.as_slice(),
        Limits::default(),
        &AtomicBool::new(false),
        |_| {
            delivered += 1;
            true
        },
    );
    assert_eq!(delivered, 1);
    assert_eq!(result.unwrap_err(), Error::Truncated);
}

#[test]
#[ignore = "explicit bounded PCAPNG throughput benchmark"]
fn packet_stream_benchmark() {
    for count in [10_000, 100_000] {
        let o = Order(false);
        let mut data = section(o);
        data.extend(interface(o, 6, 0, 65535));
        for i in 0..count {
            data.extend(packet(o, 0, i, &[0u8; 256], 256, &[]));
        }
        let start = std::time::Instant::now();
        let mut seen = 0;
        let receipt = read(
            data.as_slice(),
            Limits::default(),
            &AtomicBool::new(false),
            |_| {
                seen += 1;
                true
            },
        )
        .unwrap();
        assert_eq!(receipt.packets, count);
        assert_eq!(seen, count);
        eprintln!(
            "PCAPNG packets={count} payload_bytes=256 input_bytes={} parse_hash_ms={:.3}",
            data.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

#[test]
fn format_defaults_and_explicit_zero_values_remain_distinct_from_unknown() {
    let o = Order(false);
    let mut data = section(o);
    let mut body = Vec::new();
    body.extend(o.u16(127));
    body.extend(o.u16(0));
    body.extend(o.u32(65535));
    data.extend(block(o, 1, &body));
    data.extend(packet(o, 0, 0, &[0], 1, &option(o, 4, &o.u64(0))));
    read(
        data.as_slice(),
        Limits::default(),
        &AtomicBool::new(false),
        |p| {
            let time = p.timestamp.as_known().unwrap();
            assert_eq!(time.resolution, 6);
            assert_eq!(time.offset_seconds, 0);
            assert_eq!(time.utc_nanoseconds.0, 0);
            assert_eq!(p.dropped_since_previous, Evidence::Known(0));
            assert!(matches!(p.flags, Evidence::Unknown(_)));
            true
        },
    )
    .unwrap();
}

#[test]
fn real_stream_limits_and_midstream_cancellation_abort_publication() {
    let o = Order(false);
    let mut two = fixture(o);
    two.extend(packet(o, 0, 2, &[1], 1, &[]));
    let cancel = AtomicBool::new(false);
    let limits = Limits {
        max_packets: 1,
        ..Limits::default()
    };
    let mut delivered = 0;
    assert_eq!(
        read(two.as_slice(), limits, &cancel, |_| {
            delivered += 1;
            true
        })
        .unwrap_err(),
        Error::ResourceLimit
    );
    assert_eq!(delivered, 1);
    let limits = Limits {
        max_blocks: 1,
        ..Limits::default()
    };
    assert_eq!(
        read(two.as_slice(), limits, &cancel, |_| true).unwrap_err(),
        Error::ResourceLimit
    );
    let mut sources = section(o);
    sources.extend(interface(o, 6, 0, 100));
    sources.extend(interface(o, 6, 0, 100));
    let limits = Limits {
        max_interfaces_per_section: 1,
        ..Limits::default()
    };
    assert_eq!(
        read(sources.as_slice(), limits, &cancel, |_| true).unwrap_err(),
        Error::ResourceLimit
    );
    let sections = [section(o), section(o)].concat();
    let limits = Limits {
        max_sections: 1,
        ..Limits::default()
    };
    assert_eq!(
        read(sections.as_slice(), limits, &cancel, |_| true).unwrap_err(),
        Error::ResourceLimit
    );
    let mut delivered = 0;
    assert_eq!(
        read(two.as_slice(), Limits::default(), &cancel, |_| {
            delivered += 1;
            cancel.store(true, Ordering::Relaxed);
            true
        })
        .unwrap_err(),
        Error::Cancelled
    );
    assert_eq!(delivered, 1);
}
