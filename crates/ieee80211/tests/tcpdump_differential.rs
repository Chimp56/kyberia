use kyberia_ieee80211::{
    ElementDecode, EncodedRate, InputFraming, ManagementFrame, ManagementSubtype, SsidValue, parse,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const TCPDUMP: &str = "/usr/sbin/tcpdump";

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .split_ascii_whitespace()
        .map(|part| u8::from_str_radix(part, 16).expect("checked-in fixture hex"))
        .collect()
}

fn subtype_name(subtype: ManagementSubtype) -> &'static str {
    match subtype {
        ManagementSubtype::AssociationRequest => "Association Request",
        ManagementSubtype::AssociationResponse => "Association Response",
        ManagementSubtype::ReassociationRequest => "Reassociation Request",
        ManagementSubtype::ReassociationResponse => "Reassociation Response",
        ManagementSubtype::Beacon => "Beacon",
        ManagementSubtype::ProbeRequest => "Probe Request",
        ManagementSubtype::ProbeResponse => "Probe Response",
    }
}

fn tcpdump_field<'a>(line: &'a str, name: &str) -> &'a str {
    let prefix = format!("{name}:");
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("tcpdump output lacks {name}: field: {line}"))
}

fn parse_mac(value: &str) -> [u8; 6] {
    let octets = value
        .split(':')
        .map(|octet| u8::from_str_radix(octet, 16).expect("tcpdump MAC octet"))
        .collect::<Vec<_>>();
    octets.try_into().expect("tcpdump MAC has six octets")
}

fn typed_ssid(frame: &ManagementFrame) -> String {
    let element = frame
        .elements()
        .iter()
        .find(|element| element.id() == 0)
        .expect("fixture has SSID IE");
    match element.decoded() {
        ElementDecode::Ssid(SsidValue::Hidden | SsidValue::Wildcard) => String::new(),
        ElementDecode::Ssid(SsidValue::Binary(bytes)) => {
            String::from_utf8(bytes.clone()).expect("fixture SSID is printable UTF-8")
        }
        other => panic!("fixture SSID has unexpected typed decode: {other:?}"),
    }
}

fn tcpdump_ssid(line: &str, subtype: ManagementSubtype) -> String {
    let marker = format!("{} (", subtype_name(subtype));
    line.split_once(&marker)
        .and_then(|(_, remainder)| remainder.split_once(')'))
        .map(|(ssid, _)| ssid.to_owned())
        .unwrap_or_else(|| panic!("tcpdump output lacks SSID display for {subtype:?}: {line}"))
}

fn rate_text(rate: &EncodedRate) -> String {
    let half_mbps = if rate.units_500_kbps().is_multiple_of(2) {
        "0"
    } else {
        "5"
    };
    let basic = if rate.basic() { "*" } else { "" };
    format!("{}.{}{basic}", rate.units_500_kbps() / 2, half_mbps)
}

fn typed_rates(frame: &ManagementFrame) -> Vec<String> {
    let mut rates = Vec::new();
    for element in frame.elements() {
        let decoded = match element.decoded() {
            ElementDecode::SupportedRates(rates) | ElementDecode::ExtendedSupportedRates(rates) => {
                rates
            }
            _ => continue,
        };
        rates.extend(decoded.iter().map(rate_text));
    }
    rates
}

fn tcpdump_rates(line: &str) -> Option<Vec<String>> {
    let (_, remainder) = line.split_once('[')?;
    let (rates, _) = remainder.split_once(" Mbit]")?;
    Some(rates.split_whitespace().map(str::to_owned).collect())
}

fn typed_channel(frame: &ManagementFrame) -> Option<u8> {
    frame
        .elements()
        .iter()
        .find_map(|element| match element.decoded() {
            ElementDecode::DsParameterChannel(channel) => Some(*channel),
            _ => None,
        })
}

fn tcpdump_channel(line: &str) -> Option<u8> {
    let (_, remainder) = line.split_once("CH: ")?;
    let digits = remainder
        .split(|character: char| !character.is_ascii_digit())
        .next()?;
    digits.parse().ok()
}

fn has_tcpdump_word(line: &str, expected: &str) -> bool {
    line.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| word == expected)
}

fn retained_run_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".trash/test-runs")
        .join(format!("wifi-ie-tcpdump-{}-{nonce}", std::process::id()))
}

fn write_ieee80211_pcap(path: &Path, frames: &[Vec<u8>]) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xa1b2c3d4_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&65_535_u32.to_le_bytes());
    bytes.extend_from_slice(&105_u32.to_le_bytes()); // DLT_IEEE802_11
    for (index, frame) in frames.iter().enumerate() {
        let length = u32::try_from(frame.len()).expect("fixture length fits PCAP");
        bytes.extend_from_slice(&u32::try_from(index + 1).unwrap().to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(frame);
    }
    fs::write(path, bytes).expect("write retained PCAP fixture");
}

#[test]
#[ignore = "requires explicit external tcpdump acceptance run"]
fn checked_in_management_frames_match_tcpdump_stable_fields() {
    assert!(
        Path::new(TCPDUMP).is_file(),
        "required tcpdump missing at {TCPDUMP}"
    );
    let version = Command::new(TCPDUMP)
        .arg("--version")
        .output()
        .expect("launch tcpdump version probe");
    assert!(version.status.success(), "tcpdump version probe failed");
    let version_text = String::from_utf8_lossy(&version.stdout);
    assert!(version_text.contains("tcpdump version"));
    assert!(version_text.contains("libpcap version"));

    let frames = [
        decode_hex(include_str!("corpus/beacon.hex")),
        decode_hex(include_str!("corpus/probe-request.hex")),
        decode_hex(include_str!("corpus/probe-response-text.hex")),
    ];
    let expected = [
        ManagementSubtype::Beacon,
        ManagementSubtype::ProbeRequest,
        ManagementSubtype::ProbeResponse,
    ];
    let parsed = frames
        .iter()
        .map(|frame| parse(frame, InputFraming::FcsAbsent).unwrap())
        .collect::<Vec<_>>();
    for (frame, subtype) in parsed.iter().zip(expected) {
        assert_eq!(frame.subtype(), subtype);
    }

    let retained = retained_run_dir();
    fs::create_dir_all(&retained).expect("create retained test directory");
    let pcap = retained.join("management-frames.pcap");
    write_ieee80211_pcap(&pcap, &frames);
    let output = Command::new(TCPDUMP)
        .args(["-tt", "-nn", "-e", "-vvv", "-r"])
        .arg(&pcap)
        .output()
        .expect("launch tcpdump differential authority");
    assert!(
        output.status.success(),
        "tcpdump failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("tcpdump output is UTF-8");
    fs::write(retained.join("tcpdump-normalized.txt"), &text).expect("retain tcpdump output");
    fs::write(
        retained.join("tcpdump-version.txt"),
        version_text.as_bytes(),
    )
    .expect("retain tcpdump version");

    assert_eq!(text.lines().count(), 3, "one external decode per fixture");
    assert!(text.contains("Beacon"));
    assert!(text.contains("Probe Request"));
    assert!(text.contains("Probe Response"));
    assert!(text.contains("SA:02:11:22:33:44:55"));
    assert!(text.contains("DA:ff:ff:ff:ff:ff:ff"));
    assert!(text.contains("BSSID:02:11:22:33:44:55"));
    assert!(text.contains("BSSID:ff:ff:ff:ff:ff:ff"));
    assert!(text.contains("Beacon () [1.0* 6.0 Mbit] ESS CH: 6, PRIVACY"));
    assert!(text.contains("Probe Request () [1.0* Mbit]"));
    assert!(text.contains("Probe Response (lab)"));

    for (line, frame) in text.lines().zip(&parsed) {
        assert!(line.contains(subtype_name(frame.subtype())));
        assert_eq!(
            parse_mac(tcpdump_field(line, "DA")),
            frame.addresses().receiver().octets(),
            "destination/receiver differs for {:?}",
            frame.subtype()
        );
        assert_eq!(
            parse_mac(tcpdump_field(line, "SA")),
            frame.addresses().transmitter().octets(),
            "source/transmitter differs for {:?}",
            frame.subtype()
        );
        assert_eq!(
            parse_mac(tcpdump_field(line, "BSSID")),
            frame.addresses().bssid_field().octets(),
            "BSSID field differs for {:?}",
            frame.subtype()
        );
        assert_eq!(
            tcpdump_ssid(line, frame.subtype()),
            typed_ssid(frame),
            "SSID differs for {:?}",
            frame.subtype()
        );
        if let Some(external_rates) = tcpdump_rates(line) {
            assert_eq!(
                external_rates,
                typed_rates(frame),
                "supported rates differ for {:?}",
                frame.subtype()
            );
        }
        assert_eq!(
            tcpdump_channel(line),
            typed_channel(frame),
            "DS channel differs for {:?}",
            frame.subtype()
        );
        if frame.subtype() == ManagementSubtype::Beacon {
            let capability = frame
                .fixed()
                .expect("Beacon fixed fields")
                .capability_information();
            assert_eq!(has_tcpdump_word(line, "ESS"), capability & 0x0001 != 0);
            assert_eq!(has_tcpdump_word(line, "PRIVACY"), capability & 0x0010 != 0);
        }
    }

    println!(
        "tcpdump authority: {}",
        version_text.lines().next().unwrap()
    );
    println!("retained differential evidence: {}", retained.display());
    println!("{text}");
}
