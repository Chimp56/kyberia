use kyberia_ieee80211::{InputFraming, ManagementFrame, NeverCancel, ParseLimits, parse_with};

pub const MAX_FUZZ_INPUT_BYTES: usize = 11_500;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Reachability {
    pub canonical_document: bool,
    pub parsed_frame: bool,
    pub parsed_fcs_frame: bool,
}

pub fn decode_seed_or_raw(input: &[u8]) -> Vec<u8> {
    let Some(hex) = input.strip_prefix(b"hex:") else {
        return input.to_vec();
    };
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut high = None;
    for byte in hex.iter().copied() {
        let nibble = match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            b' ' | b'\n' | b'\r' | b'\t' => None,
            _ => return input.to_vec(),
        };
        if let Some(nibble) = nibble {
            if let Some(prefix) = high.take() {
                out.push((prefix << 4) | nibble);
            } else {
                high = Some(nibble);
            }
        }
    }
    if high.is_some() { input.to_vec() } else { out }
}

pub fn limits(profile: u8) -> ParseLimits {
    match profile % 3 {
        0 => ParseLimits::default(),
        1 => ParseLimits {
            max_frame_bytes: 512,
            max_elements: 32,
            max_ie_payload_bytes: 384,
            max_work_units: 8_192,
            max_canonical_bytes: 526,
            max_allocation_bytes: 16 * 1_024,
        },
        _ => ParseLimits {
            max_frame_bytes: 96,
            max_elements: 8,
            max_ie_payload_bytes: 64,
            max_work_units: 2_048,
            max_canonical_bytes: 110,
            max_allocation_bytes: 4 * 1_024,
        },
    }
}

pub fn exercise(input: &[u8]) -> Reachability {
    assert!(
        input.len() <= MAX_FUZZ_INPUT_BYTES,
        "the harness input must respect its declared libFuzzer maximum"
    );
    let Some((&control, encoded)) = input.split_first() else {
        return Reachability::default();
    };
    let bytes = decode_seed_or_raw(encoded);
    let limits = limits(control >> 2);
    let framing = if control & 1 == 0 {
        InputFraming::FcsAbsent
    } else {
        InputFraming::FcsPresentAndValidate
    };

    let mut reached = Reachability::default();
    if let Ok(frame) = ManagementFrame::from_canonical_bytes_with(&bytes, limits, &NeverCancel) {
        let canonical = frame
            .canonical_bytes_with(limits, &NeverCancel)
            .expect("an accepted canonical document must re-encode under the same limits");
        assert_eq!(
            canonical, bytes,
            "accepted canonical bytes must already be exact"
        );
        reached.canonical_document = true;
    }

    if let Ok(frame) = parse_with(&bytes, framing, limits, &NeverCancel) {
        let canonical = frame
            .canonical_bytes_with(limits, &NeverCancel)
            .expect("a parsed frame must canonicalize under the same limits");
        let replayed = ManagementFrame::from_canonical_bytes_with(&canonical, limits, &NeverCancel)
            .expect("a canonical record produced under the same limits must replay");
        assert_eq!(replayed, frame);
        reached.parsed_frame = true;
        reached.parsed_fcs_frame = framing == InputFraming::FcsPresentAndValidate;
    }
    reached
}
