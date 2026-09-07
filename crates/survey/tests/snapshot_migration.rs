use kyberia_domain::{evidence::*, observation::*, time::MonotonicTimestamp};
use kyberia_survey::*;
use proptest::prelude::*;
const LEGACY: &str = include_str!("fixtures/point-v1.json");
fn current() -> serde_json::Value {
    serde_json::to_value(serde_json::from_str::<PointSurvey>(LEGACY).unwrap()).unwrap()
}
#[test]
fn legacy_snapshot_reads_and_emits_only_version_two() {
    let state: PointSurvey = serde_json::from_str(LEGACY).unwrap();
    assert!(state.progress().ready);
    assert_eq!(state.phase(), &PointPhase::Completed);
    let wire = serde_json::to_value(state).unwrap();
    assert_eq!(wire["schema_version"], "2");
    assert_eq!(
        wire["records"][0]["source_version"],
        serde_json::json!({"state":"known","detail":"1"})
    );
    let mut expected: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
    expected["schema_version"] = serde_json::json!("2");
    for record in expected["records"].as_array_mut().unwrap() {
        record["source_version"] = serde_json::json!({"state":"known","detail":"1"});
    }
    assert_eq!(
        wire, expected,
        "identity, timing, progress and references remain unchanged"
    );
    assert_eq!(wire["config"]["schema_version"], "1");
}

#[test]
fn snapshot_decode_receipt_records_explicit_legacy_migration() {
    let decoded: DecodedPointSurvey = serde_json::from_str(LEGACY).unwrap();
    assert_eq!(
        decoded.receipt.input_schema_version,
        PointSnapshotInputVersion::LegacyUntaggedV1
    );
    assert_eq!(
        decoded.receipt.output_schema_version,
        PointSnapshotSchemaVersion::V2
    );
    assert!(decoded.receipt.migrated);
    assert_eq!(
        decoded.receipt.decoder_version,
        "kyberia-point-snapshot/2.0.0"
    );
    let canonical: DecodedPointSurvey =
        serde_json::from_value(serde_json::to_value(&decoded.survey).unwrap()).unwrap();
    assert!(!canonical.receipt.migrated);
    assert_eq!(
        canonical.receipt.input_schema_version,
        PointSnapshotInputVersion::V2
    );
    assert_eq!(canonical.survey, decoded.survey);
    assert_eq!(
        serde_json::to_value(decoded.receipt).unwrap()["input_schema_version"],
        "legacy_untagged_v1"
    );
}

#[test]
fn unknown_upstream_version_survives_real_admission_and_snapshot_roundtrip() {
    let config: PointConfig = serde_json::from_value(current()["config"].clone()).unwrap();
    let mut observation = serde_json::from_str::<ObservationEnvelope>(include_str!(
        "fixtures/point-observation-v1.json"
    ))
    .unwrap()
    .into_data();
    observation.source.source_version = Evidence::Unknown(UnknownReason::SourceDidNotProvide);
    let captured = *observation.time.monotonic.as_known().unwrap();
    let state = PointSurvey::start(
        config,
        MonotonicTimestamp {
            epoch: captured.epoch,
            nanoseconds: 0,
        },
    )
    .unwrap()
    .admit(&ObservationEnvelope::new(observation).unwrap(), captured)
    .unwrap();
    let wire = serde_json::to_value(&state).unwrap();
    assert_eq!(
        wire["records"][0]["source_version"],
        serde_json::json!({"state":"unknown","detail":"source_did_not_provide"})
    );
    assert_eq!(serde_json::from_value::<PointSurvey>(wire).unwrap(), state);
    assert_eq!(state.progress().metrics[&PointMetric::Rssi], 1);
}

#[test]
fn snapshot_shape_mismatches_and_unknown_tags_never_fall_back() {
    for mut bad in [
        serde_json::from_str::<serde_json::Value>(LEGACY).unwrap(),
        current(),
    ] {
        let legacy = bad.get("schema_version").is_none();
        bad["records"][0]["source_version"] = if legacy {
            serde_json::json!({"state":"known","detail":"1"})
        } else {
            serde_json::json!("1")
        };
        assert!(serde_json::from_value::<PointSurvey>(bad).is_err());
    }
    for tag in [
        serde_json::Value::Null,
        serde_json::json!(2),
        serde_json::json!("1"),
        serde_json::json!("3"),
        serde_json::json!("LegacyUntaggedV1"),
    ] {
        let mut bad = current();
        bad["schema_version"] = tag;
        assert!(serde_json::from_value::<PointSurvey>(bad).is_err());
    }
    let mut bad = current();
    bad.as_object_mut().unwrap().remove("schema_version");
    assert!(serde_json::from_value::<PointSurvey>(bad).is_err());
    for mut bad in [
        serde_json::from_str::<serde_json::Value>(LEGACY).unwrap(),
        current(),
    ] {
        bad["records"][0]
            .as_object_mut()
            .unwrap()
            .remove("source_version");
        assert!(serde_json::from_value::<PointSurvey>(bad).is_err());
    }
    let mut bad = current();
    bad["config"]["schema_version"] = serde_json::json!("2");
    assert!(serde_json::from_value::<PointSurvey>(bad).is_err());
}

#[test]
fn legacy_and_current_snapshots_both_validate_semantics_and_duplicates() {
    for value in [
        serde_json::from_str::<serde_json::Value>(LEGACY).unwrap(),
        current(),
    ] {
        let mut bad = value.clone();
        bad["records"][0]["captured"] = serde_json::json!(2_000_000_000_u64);
        assert!(serde_json::from_value::<PointSurvey>(bad).is_err());
        let mut bad = value.clone();
        bad["records"][0]["quality"] = serde_json::json!(["stale"]);
        assert!(serde_json::from_value::<PointSurvey>(bad).is_err());
        let wire = serde_json::to_string(&value).unwrap();
        let source = format!(
            "\"source_version\":{}",
            value["records"][0]["source_version"]
        );
        let duplicate = wire.replacen(&source, &format!("{source},{source}"), 1);
        assert_ne!(duplicate, wire);
        assert!(serde_json::from_str::<PointSurvey>(&duplicate).is_err());
    }
    let wire = serde_json::to_string(&current()).unwrap();
    let bad = wire.replace(
        "\"schema_version\":\"2\"",
        "\"schema_version\":\"2\",\"schema_version\":\"2\"",
    );
    assert!(serde_json::from_str::<PointSurvey>(&bad).is_err());
}

proptest! {
    #[test]
    fn snapshot_migration_does_not_panic_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(),0..4096)) {
        let _ = serde_json::from_slice::<DecodedPointSurvey>(&bytes);
    }
}
