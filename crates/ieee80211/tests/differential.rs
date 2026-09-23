use kyberia_ieee80211::{ElementDecode, InputFraming, ManagementSubtype, SsidValue, parse};

struct OracleFrame {
    subtype: u8,
    receiver: [u8; 6],
    transmitter: [u8; 6],
    bssid_field: [u8; 6],
    elements: Vec<(u8, Vec<u8>)>,
}

fn oracle(bytes: &[u8]) -> OracleFrame {
    assert!(bytes.len() >= 24);
    let subtype = bytes[0] >> 4;
    let mut a1 = [0; 6];
    a1.copy_from_slice(&bytes[4..10]);
    let mut a2 = [0; 6];
    a2.copy_from_slice(&bytes[10..16]);
    let mut a3 = [0; 6];
    a3.copy_from_slice(&bytes[16..22]);
    let mut offset = if matches!(subtype, 5 | 8) { 36 } else { 24 };
    let mut ies = Vec::new();
    while offset < bytes.len() {
        let len = usize::from(bytes[offset + 1]);
        ies.push((bytes[offset], bytes[offset + 2..offset + 2 + len].to_vec()));
        offset += 2 + len;
    }
    OracleFrame {
        subtype,
        receiver: a1,
        transmitter: a2,
        bssid_field: a3,
        elements: ies,
    }
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .split_ascii_whitespace()
        .map(|part| u8::from_str_radix(part, 16).unwrap())
        .collect()
}

#[test]
fn independent_oracle_matches_checked_in_management_fixtures() {
    for bytes in [
        decode_hex(include_str!("corpus/beacon.hex")),
        decode_hex(include_str!("corpus/probe-request.hex")),
        decode_hex(include_str!("corpus/probe-response.hex")),
    ] {
        let expected = oracle(&bytes);
        let parsed = parse(&bytes, InputFraming::FcsAbsent).unwrap();
        assert_eq!(parsed.addresses().receiver().octets(), expected.receiver);
        assert_eq!(
            parsed.addresses().transmitter().octets(),
            expected.transmitter
        );
        assert_eq!(
            parsed.addresses().bssid_field().octets(),
            expected.bssid_field
        );
        assert_eq!(
            parsed
                .elements()
                .iter()
                .map(|ie| (ie.id(), ie.payload().to_vec()))
                .collect::<Vec<_>>(),
            expected.elements
        );
        assert_eq!(
            parsed.subtype(),
            match expected.subtype {
                4 => ManagementSubtype::ProbeRequest,
                5 => ManagementSubtype::ProbeResponse,
                8 => ManagementSubtype::Beacon,
                _ => unreachable!(),
            }
        );
    }
}

#[test]
fn element_order_and_permutations_are_distinct_and_lossless() {
    let original = decode_hex(include_str!("corpus/probe-request.hex"));
    let mut permuted = original[..24].to_vec();
    permuted.extend_from_slice(&original[29..]);
    permuted.extend_from_slice(&original[24..29]);
    let a = parse(&original, InputFraming::FcsAbsent).unwrap();
    let b = parse(&permuted, InputFraming::FcsAbsent).unwrap();
    assert_ne!(a.raw_mpdu(), b.raw_mpdu());
    assert_ne!(a.elements(), b.elements());
    assert_ne!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
}

#[test]
fn fixture_empty_ssids_are_context_sensitive() {
    let request_bytes = decode_hex(include_str!("corpus/probe-request.hex"));
    let beacon_bytes = decode_hex(include_str!("corpus/beacon.hex"));
    let request = parse(&request_bytes, InputFraming::FcsAbsent).unwrap();
    let beacon = parse(&beacon_bytes, InputFraming::FcsAbsent).unwrap();
    assert_eq!(
        request.elements()[0].decoded(),
        &ElementDecode::Ssid(SsidValue::Wildcard)
    );
    assert_eq!(
        beacon.elements()[0].decoded(),
        &ElementDecode::Ssid(SsidValue::Hidden)
    );
}
