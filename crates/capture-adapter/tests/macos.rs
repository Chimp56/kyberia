use kyberia_capture_adapter::macos::*;
use serde_json::{Value, json};
const VALID: &[u8] = include_bytes!("../../../collectors/macos/fixtures/valid.ndjson");
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
            let mut bytes = serde_json::to_vec(v).unwrap();
            bytes.push(b'\n');
            bytes
        })
        .collect()
}
#[test]
fn seven_original_fixtures_decode() {
    for bytes in [
        VALID,
        include_bytes!("../../../collectors/macos/fixtures/partial.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/empty.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/error.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/unsupported.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/denied.ndjson"),
        include_bytes!("../../../collectors/macos/fixtures/probe.ndjson"),
    ] {
        decode(bytes).unwrap();
    }
}
#[test]
fn malformed_and_future_records_fail_without_panics() {
    let mutations = [
        ("/protocol", json!("kyberia.macos.collector/2")),
        ("/sequence", json!(false)),
        ("/time/receipt_monotonic_ns", json!("18446744073709551616")),
        ("/time/receipt_monotonic_ns", json!("01")),
        ("/time/receipt_utc", json!("2026-02-30T00:00:00.000Z")),
        ("/time/receipt_utc", json!("9999-01-01T00:00:00.000Z")),
        ("/time/receipt_utc", json!("2016-12-31T23:59:60.000Z")),
        ("/time/capture_time", json!({"state":"known","value":0})),
        ("/collector_build", json!("sha256:bad")),
    ];
    for (path, value) in mutations {
        let mut e = events();
        *e[0].pointer_mut(path).unwrap() = value;
        assert!(decode(&encode(&e)).is_err(), "{path}");
    }
    assert!(decode(&VALID[..VALID.len() - 1]).is_err());
    assert!(decode(b"{\"protocol\":NaN}\n").is_err());
    assert!(decode(b"{\"protocol\":1e999}\n").is_err());
    assert!(decode(b"{\"x\":{\"a\":1,\"a\":2}}\n").is_err());
    assert!(decode(&[255, b'\n']).is_err());
    let mut large = vec![b' '; MAX_RECORD_BYTES];
    large.push(b'\n');
    assert!(decode(&large).is_err());
}
#[test]
fn scientific_and_privacy_contradictions_fail() {
    let mutations = [
        ("/rssi_dbm", json!({"state":"known","value":0})),
        ("/noise_dbm", json!({"state":"known","value":-201})),
        ("/dwell_seconds", json!({"state":"known","value":0})),
        (
            "/channel/value/width_mhz",
            json!({"state":"known","value":80}),
        ),
        (
            "/channel/value/frequency_hz",
            json!({"state":"known","value":6145000000u64}),
        ),
        (
            "/bssid",
            json!({"state":"known","value":"ff:ff:ff:ff:ff:ff"}),
        ),
        (
            "/ssid_octets_base64",
            json!({"state":"known","value":"Zg="}),
        ),
        ("/api_window/start_monotonic_ns", json!("0")),
        ("/source/framework_version", json!("changed")),
        (
            "/source/driver_version",
            json!({"state":"known","value":"invented"}),
        ),
    ];
    for (path, value) in mutations {
        let mut e = events();
        *e[3].pointer_mut(path).unwrap() = value;
        assert!(decode(&encode(&e)).is_err(), "{path}");
    }
    let mut e = events();
    e[0]["identifier_policy"] = json!("redacted");
    assert!(decode(&encode(&e)).is_err());
    let mut e = events();
    e[1]["sources"][0]["power_on"] = json!(false);
    assert!(decode(&encode(&e)).is_err());
    let mut e = events();
    e[1]["sources"][0]["power_on"] = json!(false);
    e[1]["nearby_scan"]["state"] = json!("unavailable");
    assert!(decode(&encode(&e)).is_err());
}
#[test]
fn authorization_requires_real_final_permission_evidence() {
    let mut e = events();
    e.truncate(3);
    e[0]["command"] = json!("authorize");
    let common = e[1]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| ["protocol", "sequence", "session_id", "time"].contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    e[1] = Value::Object(common);
    e[1]["kind"] = json!("authorization");
    e[1]["state"] = json!("not_determined");
    e[1]["requested_by_operator"] = json!(true);
    e[1]["location_services_enabled"] = json!(true);
    e[1]["prompt_requested"] = json!(true);
    let common = e[2]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| ["protocol", "sequence", "session_id", "time"].contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    e[2] = Value::Object(common);
    e[2]["kind"] = json!("complete");
    e[2]["status"] = json!("ok");
    e[2]["reason"] = json!("location_authorized");
    e[2]["observation_count"] = json!(0);
    e[2]["partial"] = json!(false);
    e[2]["final_authorization"] = json!("authorized");
    e[2]["final_location_services_enabled"] = json!(true);
    decode(&encode(&e)).unwrap();
    e[1]["state"] = json!("denied");
    e[1]["prompt_requested"] = json!(false);
    assert!(decode(&encode(&e)).is_err());
}
proptest::proptest! {
    #[test]
    fn arbitrary_bounded_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..20000)) { let _=decode(&bytes); }
    #[test]
    fn valid_stream_byte_mutations_never_panic(offset in 0usize..VALID.len(),byte in proptest::prelude::any::<u8>()) {let mut bytes=VALID.to_vec();bytes[offset]=byte;let _=decode(&bytes);}
}

#[test]
fn unknown_fields_duplicate_keys_and_truncated_processes_are_rejected() {
    for (record, path) in [
        (0, ""),
        (1, ""),
        (1, "/sources/0"),
        (1, "/nearby_scan"),
        (3, "/source"),
        (3, "/channel/value"),
        (3, "/rssi_dbm"),
        (3, "/api_window"),
        (4, ""),
    ] {
        let mut e = events();
        e[record]
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("future_unknown".into(), json!(0));
        assert!(decode(&encode(&e)).is_err(), "{record}/{path}");
    }
    for n in 1..events().len() {
        assert!(decode(&encode(&events()[..n])).is_err());
    }
    let text = std::str::from_utf8(VALID)
        .unwrap()
        .replace("\"rssi_dbm\":{", "\"rssi_dbm\":{\"state\":\"known\",");
    assert!(decode(text.as_bytes()).is_err());
    let mut e = events();
    e[3]["ssid_octets_base64"] = json!({"state":"known","value":"Zh=="});
    assert!(decode(&encode(&e)).is_err());
    let mut e = events();
    e.push(e.last().unwrap().clone());
    e.last_mut().unwrap()["sequence"] = json!(5);
    assert!(decode(&encode(&e)).is_err());
}

#[test]
fn scan_call_must_end_after_the_start_event_receipt() {
    let mut e = events();
    e[3]["api_window"]["end_monotonic_ns"] = json!("2901");
    assert!(decode(&encode(&e)).is_err());
}

#[test]
fn all_results_from_one_scan_share_the_same_api_completion() {
    let mut e = events();
    e[0]["max_observations"] = json!(2);
    let mut second = e[3].clone();
    second["sequence"] = json!(4);
    second["observation_id"] = json!("10000000-0000-4000-8000-000000000004");
    second["api_window"]["end_monotonic_ns"] = json!("3501");
    e.insert(4, second);
    e[5]["sequence"] = json!(5);
    e[5]["observation_count"] = json!(2);
    assert!(decode(&encode(&e)).is_err());
    e[4]["api_window"]["end_monotonic_ns"] = json!("3500");
    decode(&encode(&e)).unwrap();
}

#[test]
fn identical_monotonic_times_and_wall_clock_steps_are_not_rejected() {
    let mut e = events();
    e[3]["api_window"]["end_monotonic_ns"] = json!("3000");
    e[3]["time"]["receipt_monotonic_ns"] = json!("3000");
    e[3]["time"]["receipt_utc"] = json!("2026-09-06T00:00:00.000Z");
    decode(&encode(&e)).unwrap();
}
