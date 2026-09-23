use kyberia_ieee80211::{InputFraming, ManagementSubtype, parse};
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
    for (frame, subtype) in frames.iter().zip(expected) {
        assert_eq!(
            parse(frame, InputFraming::FcsAbsent).unwrap().subtype(),
            subtype
        );
    }
    assert_eq!(frames[2][38..41], *b"lab");

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

    println!(
        "tcpdump authority: {}",
        version_text.lines().next().unwrap()
    );
    println!("retained differential evidence: {}", retained.display());
    println!("{text}");
}
