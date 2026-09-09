use kyberia_capture_adapter::macos::*;
use kyberia_domain::{capability::*, evidence::*, identity::*, observation::*};
use serde_json::{Value, json};
use std::collections::BTreeMap;
const VALID: &[u8] = include_bytes!("../../../collectors/macos/fixtures/valid.ndjson");
fn context(stream: &DecodedStream, redacted: bool) -> MappingContext {
    MappingContext {
        expected_process_session: stream.process_session().into(),
        session_id: SessionId::from_bytes([1; 16]).unwrap(),
        collector_id: CollectorId::from_bytes([2; 16]).unwrap(),
        clock_epoch: ClockEpochId::from_bytes([3; 16]).unwrap(),
        sources: stream
            .source_keys()
            .enumerate()
            .map(|(i, k)| {
                (
                    k.into(),
                    SourceMapping {
                        source_id: SourceId::from_bytes([i as u8 + 4; 16]).unwrap(),
                        sensor_id: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                        adapter_id: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    },
                )
            })
            .collect(),
        observations: stream
            .observation_keys()
            .enumerate()
            .map(|(i, k)| {
                (
                    k.into(),
                    ObservationMapping {
                        observation_id: ObservationId::from_bytes([i as u8 + 8; 16]).unwrap(),
                        transmitter_radio: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                        transmitter_bss: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                        identity_evidence: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    },
                )
            })
            .collect(),
        privacy: PrivacyState {
            policy_version: Text::new("test-policy/1").unwrap(),
            identifiers: if redacted {
                IdentifierPolicy::Redacted
            } else {
                IdentifierPolicy::ExplicitResearchConsent
            },
            payload: PayloadRetention::Discarded,
        },
    }
}

#[test]
fn decoded_stream_exposes_only_its_validated_source_clock_epoch_and_terminal() {
    let stream = decode(VALID).unwrap();
    assert_eq!(stream.clock_epoch(), stream.process_session());
    assert_eq!(stream.clock_epoch(), "00000000-0000-4000-8000-000000000001");
    assert_eq!(stream.terminal_status(), TerminalStatus::Ok);
}
fn events() -> Vec<Value> {
    std::str::from_utf8(VALID)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn encode(v: &[Value]) -> Vec<u8> {
    v.iter()
        .flat_map(|v| {
            let mut b = serde_json::to_vec(v).unwrap();
            b.push(b'\n');
            b
        })
        .collect()
}
#[test]
fn source_receipt_is_never_measurement_time_or_dwell() {
    let stream = decode(VALID).unwrap();
    let normalized = normalize(&stream, &context(&stream, false)).unwrap();
    let received = &normalized.observations[0];
    let e = received.envelope().data();
    assert!(matches!(e.time.wall, Evidence::Unknown(_)));
    assert!(matches!(e.time.monotonic, Evidence::Unknown(_)));
    assert!(matches!(e.dwell, Evidence::Unknown(_)));
    assert!(matches!(e.pose, Evidence::Unknown(_)));
    let response = received.source_response().as_known().unwrap();
    assert_eq!(
        response.returned_at().wall.as_known().unwrap().time.0,
        1788739200000000000
    );
    assert_eq!(
        response
            .returned_at()
            .monotonic
            .as_known()
            .unwrap()
            .nanoseconds,
        4000
    );
    assert_eq!(
        response
            .api_window()
            .as_known()
            .unwrap()
            .start()
            .nanoseconds,
        2900
    );
    assert!(e.quality.contains(&QualityFlag::SyntheticFixture));
    assert!(e.quality.contains(&QualityFlag::ClockUncertain));
    assert_eq!(e.source.adapter_version.as_str(), "0.1.0");
    assert_eq!(
        e.source.source_version.as_known().unwrap().as_str(),
        "synthetic-framework"
    );
    let ObservationPayload::Scan(scan) = &e.payload else {
        panic!("scan")
    };
    assert_eq!(scan.ssid.as_known().unwrap().bytes(), b"test-only");
    assert_eq!(
        scan.identity.bssid.as_known().unwrap().0,
        [2, 0, 0, 0, 0, 1]
    );
    assert_eq!(scan.signal.rssi_dbm.as_known().unwrap().get(), -61.0);
    assert!(matches!(scan.signal.noise_dbm, Evidence::Unknown(_)));
    assert!(matches!(
        scan.signal.calibration,
        Evidence::Known(CalibrationState::Uncalibrated)
    ));
    assert!(matches!(
        e.channel.as_known().unwrap().primary_channel,
        Evidence::Unknown(_)
    ));
    assert!(matches!(
        e.channel.as_known().unwrap().primary_frequency,
        Evidence::Unknown(_)
    ));
    assert_eq!(
        normalize(&stream, &context(&stream, false))
            .unwrap()
            .completion
            .status,
        TerminalStatus::Ok
    );
    assert_eq!(
        serde_json::to_vec(&normalized).unwrap(),
        serde_json::to_vec(&normalize(&stream, &context(&stream, false)).unwrap()).unwrap()
    );
}
#[test]
fn partial_redaction_and_capability_honesty_survive() {
    let stream = decode(include_bytes!(
        "../../../collectors/macos/fixtures/partial.ndjson"
    ))
    .unwrap();
    let result = normalize(&stream, &context(&stream, true)).unwrap();
    assert!(result.completion.partial);
    let e = result.observations[0].envelope().data();
    assert!(e.quality.contains(&QualityFlag::PartialCapture));
    let ObservationPayload::Scan(s) = &e.payload else {
        panic!("scan")
    };
    assert_eq!(s.identity.bssid, Evidence::Unknown(UnknownReason::Redacted));
    let caps = result.capabilities.as_known().unwrap();
    assert!(matches!(
        caps.entries.get(&Capability::NoiseDbm),
        Some(CapabilityState::Conditional { .. })
    ));
    assert!(
        caps.require_available(&[Capability::MonitorFrames])
            .is_err()
    );
    assert!(caps.require_available(&[Capability::Band6Ghz]).is_err());
}
#[test]
fn caller_mappings_and_privacy_are_mandatory() {
    let stream = decode(VALID).unwrap();
    let mut c = context(&stream, false);
    c.observations = BTreeMap::new();
    assert!(normalize(&stream, &c).is_err());
    let mut c = context(&stream, false);
    c.sources = BTreeMap::new();
    assert!(normalize(&stream, &c).is_err());
    let mut c = context(&stream, false);
    c.expected_process_session = "another".into();
    assert!(normalize(&stream, &c).is_err());
    let mut c = context(&stream, false);
    c.privacy.identifiers = IdentifierPolicy::ProjectPseudonymized;
    assert!(normalize(&stream, &c).is_err());
    let mut c = context(&stream, false);
    c.observations
        .values_mut()
        .next()
        .unwrap()
        .transmitter_radio = Evidence::Known(RadioId::from_bytes([9; 16]).unwrap());
    assert!(normalize(&stream, &c).is_err());
    let c = context(&stream, true);
    assert!(normalize(&stream, &c).is_err());
}
#[test]
fn zero_clock_zero_channel_count_and_binary_ssid_remain_real_values() {
    let mut e = events();
    for r in &mut e {
        r["time"]["receipt_monotonic_ns"] = json!("0");
    }
    e[1]["sources"][0]["supported_channel_count"] = json!({"state":"known","value":0});
    e[2]["api_started_monotonic_ns"] = json!("0");
    e[3]["api_window"]["start_monotonic_ns"] = json!("0");
    e[3]["api_window"]["end_monotonic_ns"] = json!("0");
    e[3]["ssid_octets_base64"] = json!({"state":"known","value":"AP8="});
    e[3]["noise_dbm"] = json!({"state":"known","value":-100});
    let stream = decode(&encode(&e)).unwrap();
    let result = normalize(&stream, &context(&stream, false)).unwrap();
    assert_eq!(
        result.observations[0]
            .source_response()
            .as_known()
            .unwrap()
            .returned_at()
            .monotonic
            .as_known()
            .unwrap()
            .nanoseconds,
        0
    );
    let ObservationPayload::Scan(s) = &result.observations[0].envelope().data().payload else {
        panic!("scan")
    };
    assert_eq!(s.ssid.as_known().unwrap().bytes(), &[0, 255]);
    assert_eq!(s.signal.noise_dbm.as_known().unwrap().get(), -100.0);
}
#[test]
fn all_terminal_fixtures_normalize_without_fake_measurements() {
    for bytes in [
        include_bytes!("../../../collectors/macos/fixtures/empty.ndjson").as_slice(),
        include_bytes!("../../../collectors/macos/fixtures/error.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/unsupported.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/denied.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/probe.ndjson"),
    ] {
        let s = decode(bytes).unwrap();
        let n = normalize(&s, &context(&s, true)).unwrap();
        assert!(n.observations.is_empty());
    }
}

#[test]
#[ignore = "Explicit golden regeneration only; inspect diff and independent assertions before accepting"]
fn regenerate_canonical_golden() {
    let stream = decode(VALID).unwrap();
    let n = normalize(&stream, &context(&stream, false)).unwrap();
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/macos-valid-canonical.json"
        ),
        serde_json::to_string_pretty(&n.observations[0]).unwrap() + "\n",
    )
    .unwrap();
}

#[test]
fn exact_source_bytes_and_hashes_survive_normalization() {
    use sha2::{Digest, Sha256};
    let stream = decode(VALID).unwrap();
    let n = normalize(&stream, &context(&stream, false)).unwrap();
    assert_eq!(n.evidence_origin, SourceKind::SyntheticFixture);
    assert_eq!(
        n.source_records
            .iter()
            .flat_map(|r| r.bytes.iter().copied())
            .collect::<Vec<_>>(),
        VALID
    );
    for r in &n.source_records {
        assert_eq!(r.reference.byte_length, r.bytes.len() as u64);
        assert_eq!(
            r.reference.sha256.bytes(),
            <[u8; 32]>::from(Sha256::digest(&r.bytes))
        );
    }
    assert_eq!(
        n.observations[0]
            .envelope()
            .data()
            .raw_source
            .as_known()
            .unwrap(),
        &n.source_records[3].reference
    );
}

#[test]
#[ignore = "Explicit reproducible benchmark; no timing assertion on shared CI hosts"]
fn benchmark_maximum_scan() {
    use std::time::Instant;
    for count in [256, 4096] {
        let e = events();
        let mut records = e[..3].to_vec();
        records[0]["max_observations"] = json!(count);
        for i in 0..count {
            let mut o = e[3].clone();
            o["sequence"] = json!(i + 3);
            o["observation_id"] = json!(format!("00000000-0000-4000-8000-{:012x}", i + 100));
            records.push(o);
        }
        let mut end = e[4].clone();
        end["sequence"] = json!(count + 3);
        end["observation_count"] = json!(count);
        records.push(end);
        let bytes = encode(&records);
        let start = Instant::now();
        let s = decode(&bytes).unwrap();
        let decoded = start.elapsed();
        let mut c = context(&decode(VALID).unwrap(), false);
        c.expected_process_session = s.process_session().into();
        c.observations = s
            .observation_keys()
            .enumerate()
            .map(|(i, k)| {
                let mut id = [1; 16];
                id[..8].copy_from_slice(&(i as u64).to_be_bytes());
                (
                    k.into(),
                    ObservationMapping {
                        observation_id: ObservationId::from_bytes(id).unwrap(),
                        transmitter_radio: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                        transmitter_bss: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                        identity_evidence: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    },
                )
            })
            .collect();
        let start = Instant::now();
        let n = normalize(&s, &c).unwrap();
        let normalized = start.elapsed();
        assert_eq!(n.observations.len(), count);
        println!(
            "observations={count} bytes={} decode_ms={:.3} normalize_ms={:.3}",
            bytes.len(),
            decoded.as_secs_f64() * 1000.0,
            normalized.as_secs_f64() * 1000.0
        );
    }
}

#[test]
fn canonical_golden_is_stable() {
    let s = decode(VALID).unwrap();
    let n = normalize(&s, &context(&s, false)).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/macos-valid-canonical.json")).unwrap();
    assert_eq!(serde_json::to_value(&n.observations[0]).unwrap(), expected);
}

#[test]
fn receiver_and_transmitter_identities_are_separate() {
    let s = decode(VALID).unwrap();
    let mut c = context(&s, false);
    let sensor = SensorId::from_bytes([40; 16]).unwrap();
    let adapter = AdapterId::from_bytes([41; 16]).unwrap();
    c.sources.values_mut().next().unwrap().sensor_id = Evidence::Known(sensor);
    c.sources.values_mut().next().unwrap().adapter_id = Evidence::Known(adapter);
    let m = c.observations.values_mut().next().unwrap();
    m.transmitter_radio = Evidence::Known(RadioId::from_bytes([42; 16]).unwrap());
    m.transmitter_bss = Evidence::Known(BssId::from_bytes([43; 16]).unwrap());
    m.identity_evidence = Evidence::Known(ArtifactReference {
        sha256: ContentHash::from_sha256([1; 32]),
        media_type: Text::new("application/vnd.kyberia.identity-assignment").unwrap(),
        byte_length: 50,
    });
    let n = normalize(&s, &c).unwrap();
    let e = n.observations[0].envelope().data();
    assert_eq!(e.source.sensor_id, Evidence::Known(sensor));
    assert_eq!(e.source.adapter_id, Evidence::Known(adapter));
    let ObservationPayload::Scan(scan) = &e.payload else {
        panic!("scan")
    };
    assert_eq!(
        scan.identity.radio,
        Evidence::Known(RadioId::from_bytes([42; 16]).unwrap())
    );
    let s = decode(include_bytes!(
        "../../../collectors/macos/fixtures/partial.ndjson"
    ))
    .unwrap();
    c.privacy.identifiers = IdentifierPolicy::Redacted;
    assert!(normalize(&s, &c).is_err());
}

#[test]
fn full_monotonic_range_and_unknown_framework_are_preserved() {
    let mut e = events();
    for r in &mut e {
        r["time"]["receipt_monotonic_ns"] = json!(u64::MAX.to_string());
    }
    e[2]["api_started_monotonic_ns"] = json!(u64::MAX.to_string());
    e[3]["api_window"]["start_monotonic_ns"] = json!(u64::MAX.to_string());
    e[3]["api_window"]["end_monotonic_ns"] = json!(u64::MAX.to_string());
    e[1]["sources"][0]["framework_version"] = json!("unknown");
    e[3]["source"]["framework_version"] = json!("unknown");
    let s = decode(&encode(&e)).unwrap();
    let n = normalize(&s, &context(&s, false)).unwrap();
    assert_eq!(
        n.observations[0]
            .source_response()
            .as_known()
            .unwrap()
            .returned_at()
            .monotonic
            .as_known()
            .unwrap()
            .nanoseconds,
        u64::MAX
    );
    assert!(matches!(
        n.observations[0].envelope().data().source.source_version,
        Evidence::Unknown(_)
    ));
}
