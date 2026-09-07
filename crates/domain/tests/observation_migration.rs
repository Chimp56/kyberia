use kyberia_domain::{evidence::*, identity::Text, observation::*};
use proptest::prelude::*;
const LEGACY: &str = include_str!("fixtures/observation-v1.json");

fn current() -> serde_json::Value {
    serde_json::to_value(serde_json::from_str::<ObservationEnvelope>(LEGACY).unwrap()).unwrap()
}
#[test]
fn legacy_envelope_reads_and_emits_only_version_two() {
    let bytes = LEGACY;
    let envelope: ObservationEnvelope = serde_json::from_str(bytes).unwrap();
    let wire = serde_json::to_value(envelope).unwrap();
    assert_eq!(wire["schema_version"], "2");
    assert_eq!(
        wire["source"]["source_version"],
        serde_json::json!({"state":"known","detail":"1"})
    );
    let mut expected: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
    expected["schema_version"] = serde_json::json!("2");
    expected["source"]["source_version"] = serde_json::json!({"state":"known","detail":"1"});
    assert_eq!(wire, expected, "every other field is preserved");
    assert_ne!(serde_json::to_vec(&wire).unwrap(), LEGACY.as_bytes());
}

#[test]
fn decode_receipt_distinguishes_migration_from_native_v2_reading() {
    let decoded: DecodedObservation = serde_json::from_str(LEGACY).unwrap();
    assert_eq!(
        decoded.receipt.input_schema_version,
        ObservationInputVersion::V1
    );
    assert_eq!(
        decoded.receipt.output_schema_version,
        ObservationSchemaVersion::V2
    );
    assert_eq!(decoded.receipt.decoder_version, "kyberia-observation/2.0.0");
    assert!(decoded.receipt.migrated);
    let roundtrip: DecodedObservation =
        serde_json::from_value(serde_json::to_value(&decoded.envelope).unwrap()).unwrap();
    assert_eq!(roundtrip.envelope, decoded.envelope);
    assert_eq!(
        roundtrip.receipt.input_schema_version,
        ObservationInputVersion::V2
    );
    assert!(!roundtrip.receipt.migrated);
    assert_eq!(
        serde_json::to_value(decoded.receipt).unwrap()["input_schema_version"],
        "1"
    );
}

#[test]
fn every_unknown_source_version_reason_survives_without_becoming_text() {
    for reason in [
        UnknownReason::NotMeasured,
        UnknownReason::NotAdvertised,
        UnknownReason::NotObservable,
        UnknownReason::NotApplicable,
        UnknownReason::UnsupportedCapability,
        UnknownReason::PermissionDenied,
        UnknownReason::FilteredOut,
        UnknownReason::BelowDetectionThreshold,
        UnknownReason::FailedTest,
        UnknownReason::NoAssociation,
        UnknownReason::InvalidGeometry,
        UnknownReason::SolverFailure,
        UnknownReason::OutsideEvidenceSupport,
        UnknownReason::ClockUnavailable,
        UnknownReason::SourceDidNotProvide,
        UnknownReason::Redacted,
        UnknownReason::NotRetained,
    ] {
        let mut data = serde_json::from_str::<ObservationEnvelope>(LEGACY)
            .unwrap()
            .into_data();
        data.source.source_version = Evidence::Unknown(reason.clone());
        let envelope = ObservationEnvelope::new(data).unwrap();
        let roundtrip: ObservationEnvelope =
            serde_json::from_value(serde_json::to_value(envelope).unwrap()).unwrap();
        assert_eq!(
            roundtrip.data().source.source_version,
            Evidence::Unknown(reason)
        );
    }
}

#[test]
fn schema_and_source_version_shapes_must_agree_without_fallback() {
    let legacy: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
    for schema in [serde_json::json!("1"), serde_json::json!("2")] {
        for bad in [
            serde_json::Value::Null,
            serde_json::json!(42),
            serde_json::json!(""),
            serde_json::json!("\n"),
            serde_json::json!({"state":"known","detail":""}),
            serde_json::json!({"state":"unknown","detail":"future_reason"}),
        ] {
            let mut wire = legacy.clone();
            wire["schema_version"] = schema.clone();
            wire["source"]["source_version"] = bad;
            assert!(serde_json::from_value::<ObservationEnvelope>(wire).is_err());
        }
        let mut missing = legacy.clone();
        missing["schema_version"] = schema;
        missing["source"]
            .as_object_mut()
            .unwrap()
            .remove("source_version");
        assert!(serde_json::from_value::<ObservationEnvelope>(missing).is_err());
    }
    let mut wrong = current();
    wrong["source"]["source_version"] = serde_json::json!("1");
    assert!(serde_json::from_value::<ObservationEnvelope>(wrong).is_err());
    let mut wrong = legacy;
    wrong["source"]["source_version"] = serde_json::json!({"state":"known","detail":"1"});
    assert!(serde_json::from_value::<ObservationEnvelope>(wrong).is_err());
    for schema in [
        serde_json::Value::Null,
        serde_json::json!(2),
        serde_json::json!("3"),
        serde_json::json!("0"),
    ] {
        let mut wrong = current();
        wrong["schema_version"] = schema;
        assert!(serde_json::from_value::<ObservationEnvelope>(wrong).is_err());
    }
    let mut missing = current();
    missing.as_object_mut().unwrap().remove("schema_version");
    assert!(serde_json::from_value::<ObservationEnvelope>(missing).is_err());
}

#[test]
fn both_wire_versions_apply_full_envelope_validation() {
    for original in [serde_json::from_str(LEGACY).unwrap(), current()] {
        let mut bad = original.clone();
        bad["quality"] = serde_json::json!([]);
        assert!(serde_json::from_value::<ObservationEnvelope>(bad).is_err());
        let mut bad = original.clone();
        bad["source"]["source_id"] = serde_json::json!("00000000000000000000000000000000");
        assert!(serde_json::from_value::<ObservationEnvelope>(bad).is_err());
        let mut bad = original;
        bad["payload"]["data"]["signal"]["rssi_dbm"]["detail"] = serde_json::json!("NaN");
        assert!(serde_json::from_value::<ObservationEnvelope>(bad).is_err());
    }
}

#[test]
fn duplicate_version_source_and_nested_version_keys_are_rejected() {
    for value in [serde_json::from_str(LEGACY).unwrap(), current()] {
        let wire = serde_json::to_string(&value).unwrap();
        for key in ["schema_version", "source"] {
            let field = format!("\"{key}\":{}", value[key]);
            let bad = wire.replacen(&field, &format!("{field},{field}"), 1);
            assert_ne!(bad, wire);
            assert!(
                serde_json::from_str::<ObservationEnvelope>(&bad).is_err(),
                "{key}"
            );
        }
        let field = format!("\"source_version\":{}", value["source"]["source_version"]);
        let bad = wire.replacen(&field, &format!("{field},{field}"), 1);
        assert!(serde_json::from_str::<ObservationEnvelope>(&bad).is_err());
    }
}

#[test]
fn canonical_staging_and_unrelated_schema_types_remain_closed() {
    assert!(serde_json::from_str::<EnvelopeData>(LEGACY).is_err());
    assert!(serde_json::from_str::<SchemaVersion>("\"2\"").is_err());
    assert!(serde_json::from_str::<ObservationSchemaVersion>("\"1\"").is_err());
    let data: EnvelopeData = serde_json::from_value(current()).unwrap();
    assert_eq!(data.schema_version, ObservationSchemaVersion::V2);
}

proptest! {
    #[test]
    fn valid_legacy_versions_migrate_exactly(version in "[A-Za-z0-9._+-]{1,128}") {
        let mut old: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
        old["source"]["source_version"] = serde_json::json!(version);
        let migrated: ObservationEnvelope = serde_json::from_value(old).unwrap();
        prop_assert_eq!(migrated.data().source.source_version.clone(), Evidence::Known(Text::new(version).unwrap()));
    }
    #[test]
    fn migration_decoders_do_not_panic_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(),0..4096)) {
        let _ = serde_json::from_slice::<DecodedObservation>(&bytes);
    }
}
