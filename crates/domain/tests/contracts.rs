use kyberia_domain::{
    ValidationError, capability::*, evidence::*, identity::*, observation::*, spatial::*, time::*,
    units::*,
};
use proptest::prelude::*;
use serde::Deserialize;

fn text(value: &str) -> Text {
    Text::new(value).unwrap()
}
fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}
fn timestamp(n: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: ClockEpochId::from_bytes([1; 16]).unwrap(),
        nanoseconds: n,
    }
}
fn fixture() -> EnvelopeData {
    EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes([1; 16]).unwrap(),
        session_id: SessionId::from_bytes([2; 16]).unwrap(),
        source: SourceDescriptor {
            source_id: SourceId::from_bytes([3; 16]).unwrap(),
            collector_id: CollectorId::from_bytes([4; 16]).unwrap(),
            sensor_id: unknown(),
            adapter_id: unknown(),
            kind: SourceKind::SyntheticFixture,
            source_name: text("independent contract fixture"),
            source_version: Evidence::Known(text("1")),
            source_schema_version: text("1"),
            adapter_name: text("test"),
            adapter_version: text("0.1.0"),
            parser_version: text("0.1.0"),
            driver_version: unknown(),
            os_version: unknown(),
        },
        time: CaptureTime {
            wall: unknown(),
            monotonic: Evidence::Known(timestamp(10)),
            synchronization: unknown(),
        },
        pose: unknown(),
        channel: unknown(),
        dwell: unknown(),
        privacy: PrivacyState {
            policy_version: text("metadata-v1"),
            identifiers: IdentifierPolicy::Redacted,
            payload: PayloadRetention::Discarded,
        },
        quality: vec![QualityFlag::SyntheticFixture],
        raw_source: unknown(),
        payload: ObservationPayload::Scan(ScanObservation {
            identity: RadioIdentityEvidence {
                physical_device: unknown(),
                radio: unknown(),
                bss: unknown(),
                bssid: unknown(),
                ess: unknown(),
                mld: unknown(),
                link_id: unknown(),
                client: unknown(),
                grouping_evidence: unknown(),
            },
            ssid: Evidence::Known(Ssid::new(vec![0xff, 0, 0x80]).unwrap()),
            signal: SignalReading {
                rssi_dbm: Evidence::Known(Dbm::new(-62.5).unwrap()),
                noise_dbm: Evidence::Unknown(UnknownReason::NotObservable),
                chains: vec![],
                calibration: Evidence::Known(CalibrationState::Uncalibrated),
                measurement_method: text("raw OS scan"),
            },
            information_elements: unknown(),
            result_age: unknown(),
        }),
    }
}

#[test]
fn every_unit_rejects_nonfinite_in_constructor_and_deserializer() {
    macro_rules! check {
        ($($t:ty),+) => {$ (
            for value in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY] {
                assert!(<$t>::new(value).is_err());
                let decoder = serde::de::value::F64Deserializer::<serde::de::value::Error>::new(value);
                assert!(<$t>::deserialize(decoder).is_err());
            }
        )+}
    }
    check!(
        Dbm,
        Db,
        Hertz,
        Megahertz,
        Gigahertz,
        Meters,
        CoordinateMeters,
        MetersPerPixel,
        Pixels,
        Seconds,
        Milliseconds,
        SignedSeconds,
        Mbps,
        Probability,
        Percentage,
        Radians,
        Degrees,
        PartsPerMillion
    );
}

#[test]
fn ranges_and_checked_arithmetic() {
    assert!(Hertz::new(0.0).is_err());
    assert!(Meters::new(-1.0).is_err());
    assert!(Seconds::new(-1.0).is_err());
    assert!(Probability::new(1.001).is_err());
    assert!(Percentage::new(100.001).is_err());
    assert!(CoordinateMeters::new(-1.0).is_ok());
    assert_eq!(
        Dbm::new(-60.0)
            .unwrap()
            .difference(Dbm::new(-90.0).unwrap())
            .unwrap()
            .get(),
        30.0
    );
    assert!(
        Dbm::new(f64::MAX)
            .unwrap()
            .apply_gain(Db::new(f64::MAX).unwrap())
            .is_err()
    );
    assert!(Hertz::try_from(Gigahertz::new(f64::MAX).unwrap()).is_err());
    assert_eq!(
        Meters::new(-0.0).unwrap().get().to_bits(),
        0.0_f64.to_bits()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn frequency_round_trip(mhz in 0.001_f64..100_000.0) {
        let initial=Megahertz::new(mhz).unwrap();
        let result=Megahertz::try_from(Hertz::try_from(initial).unwrap()).unwrap();
        prop_assert!((result.get()-mhz).abs() <= mhz*1e-14);
    }
    #[test]
    fn probability_percentage_round_trip(p in 0.0_f64..=1.0) {
        let result=Probability::try_from(Percentage::try_from(Probability::new(p).unwrap()).unwrap()).unwrap();
        prop_assert!((result.get()-p).abs() <= 1e-14);
    }
    #[test]
    fn duration_round_trip(s in 0.0_f64..1e9) {
        let result=Seconds::try_from(Milliseconds::try_from(Seconds::new(s).unwrap()).unwrap()).unwrap();
        prop_assert!((result.get()-s).abs() <= s*1e-14);
    }
    #[test]
    fn covariance_accepts_gram_matrices(v in prop::array::uniform9(-1e3_f64..1e3)) {
        let dot=|a:usize,b:usize| (0..3).map(|i|v[a*3+i]*v[b*3+i]).sum::<f64>();
        prop_assert!(PositionCovariance::new([dot(0,0),dot(0,1),dot(0,2),dot(1,1),dot(1,2),dot(2,2)]).is_ok());
    }
    #[test]
    fn opaque_ids_round_trip(bytes in prop::array::uniform16(any::<u8>())) {
        prop_assume!(bytes != [0;16]);
        let id=ObservationId::from_bytes(bytes).unwrap();
        let json=serde_json::to_string(&id).unwrap();
        prop_assert_eq!(serde_json::from_str::<ObservationId>(&json).unwrap(),id);
    }
    #[test]
    fn arbitrary_json_bytes_never_panic(input in prop::collection::vec(any::<u8>(),0..4096)) {
        let _=serde_json::from_slice::<ObservationEnvelope>(&input);
    }
}

#[test]
fn covariance_checks_all_principal_minors_and_scale() {
    assert!(PositionCovariance::new([1., 2., 0., 1., 0., 1.]).is_err());
    // All 2x2 minors positive but full determinant negative.
    assert!(PositionCovariance::new([1., -0.9, -0.9, 1., -0.9, 1.]).is_err());
    assert!(PositionCovariance::new([-1., 0., 0., 1., 0., 1.]).is_err());
    assert!(PositionCovariance::new([f64::NAN, 0., 0., 1., 0., 1.]).is_err());
    assert!(PositionCovariance::new([1e300, 0., 0., 1e300, 0., 1e300]).is_ok());
    assert!(PositionCovariance::new([1., 1., 1., 1., 1., 1.]).is_ok());
    assert!(serde_json::from_str::<PositionCovariance>("[1,2,0,1,0,1]").is_err());
}

#[test]
fn covariance_rejects_small_indefinite_subspace_next_to_large_variance() {
    for coupling in [1e-8, 1e-6, 0.1] {
        let invalid = [1.0, 0.0, 0.0, 0.0, coupling, 0.0];
        assert!(PositionCovariance::new(invalid).is_err());
        assert!(serde_json::from_value::<PositionCovariance>(serde_json::json!(invalid)).is_err());
        // Positive tiny diagonals must not evade the same test.
        assert!(
            PositionCovariance::new([1.0, 0.0, 0.0, coupling / 10.0, coupling, coupling / 10.0])
                .is_err()
        );
    }
    // The first pivot is singular only after eliminating the large direction.
    assert!(PositionCovariance::new([1.0, 1.0, 0.0, 1.0, 1e-8, 0.0]).is_err());
    assert!(PositionCovariance::new([1.0, 0.0, 0.0, 1e-8, 0.0, 1e-8]).is_ok());
}

#[test]
fn cross_sensor_and_restarted_clocks_do_not_compare() {
    let a = timestamp(100);
    let mut b = timestamp(200);
    assert_eq!(b.elapsed_since(a).unwrap().get(), 1e-7);
    assert_eq!(a.elapsed_since(b), Err(ValidationError::ReversedTime));
    b.epoch = ClockEpochId::from_bytes([2; 16]).unwrap();
    assert_eq!(b.elapsed_since(a), Err(ValidationError::ClockEpochMismatch));
    assert!(MonotonicWindow::new(a, b).is_err());
    assert!(
        serde_json::from_value::<MonotonicWindow>(
            serde_json::json!({"start":timestamp(20),"end":timestamp(10)})
        )
        .is_err()
    );
}

#[test]
fn schema_round_trip_additive_compatibility_and_missing_field_rejection() {
    let envelope = ObservationEnvelope::new(fixture()).unwrap();
    let mut json = serde_json::to_value(&envelope).unwrap();
    json["future_optional_diagnostic"] = serde_json::json!({"name":"extra"});
    assert_eq!(
        serde_json::from_value::<ObservationEnvelope>(json.clone()).unwrap(),
        envelope
    );
    json["schema_version"] = serde_json::json!("3");
    assert!(serde_json::from_value::<ObservationEnvelope>(json).is_err());
    let mut json = serde_json::to_value(&envelope).unwrap();
    json.as_object_mut().unwrap().remove("time");
    assert!(serde_json::from_value::<ObservationEnvelope>(json).is_err());
}

#[test]
fn missing_noise_survives_round_trip_and_is_never_zero() {
    let envelope = ObservationEnvelope::new(fixture()).unwrap();
    let serialized = serde_json::to_string(&envelope).unwrap();
    let replay: ObservationEnvelope = serde_json::from_str(&serialized).unwrap();
    let ObservationPayload::Scan(scan) = &replay.data().payload else {
        panic!("scan expected")
    };
    assert_eq!(
        scan.signal.noise_dbm,
        Evidence::Unknown(UnknownReason::NotObservable)
    );
    assert_eq!(scan.ssid.as_known().unwrap().bytes(), [0xff, 0, 0x80]);
}

#[test]
fn admission_rejects_unlabeled_fixtures_and_duplicate_chains() {
    let mut data = fixture();
    data.quality.clear();
    assert!(ObservationEnvelope::new(data).is_err());
    let mut data = fixture();
    let ObservationPayload::Scan(scan) = &mut data.payload else {
        unreachable!()
    };
    scan.signal.chains = vec![
        ChainSignal {
            chain_index: 0,
            rssi_dbm: unknown(),
            noise_dbm: unknown()
        };
        2
    ];
    let json = serde_json::to_value(data).unwrap();
    assert!(serde_json::from_value::<ObservationEnvelope>(json).is_err());
}

#[test]
fn malformed_identity_and_metadata_rejected() {
    for s in [
        "",
        "../outside",
        "00000000000000000000000000000000",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        assert!(ObservationId::try_from(s.to_owned()).is_err());
    }
    assert!(Text::new("\n").is_err());
    assert!(Text::new("x".repeat(1025)).is_err());
    assert!(Ssid::new(vec![0; 33]).is_err());
    assert!(serde_json::from_str::<ObservationId>("null").is_err());
}

#[test]
fn absent_or_conditional_capability_never_grants_permission() {
    let mut document = CapabilityDocument {
        schema_version: SchemaVersion::V1,
        collector_id: CollectorId::from_bytes([1; 16]).unwrap(),
        collector_version: text("1"),
        probed_at: CaptureTime {
            wall: unknown(),
            monotonic: unknown(),
            synchronization: unknown(),
        },
        entries: std::collections::BTreeMap::new(),
        raw_payload_policy: RawPayloadPolicy::Discard,
    };
    assert_eq!(document.capability(&Capability::NoiseDbm), unknown());
    assert!(document.require_available(&[Capability::NoiseDbm]).is_err());
    document.entries.insert(
        Capability::NoiseDbm,
        CapabilityState::Conditional {
            condition: text("hardware dependent"),
            evidence: text("fixture"),
        },
    );
    assert!(document.require_available(&[Capability::NoiseDbm]).is_err());
    document.entries.insert(
        Capability::NoiseDbm,
        CapabilityState::Available {
            evidence: text("runtime probe"),
        },
    );
    assert!(document.require_available(&[Capability::NoiseDbm]).is_ok());
}

#[test]
fn envelope_rejects_clock_model_and_dwell_contradictions() {
    let mut data = fixture();
    data.time.synchronization = Evidence::Known(ClockModel {
        epoch: ClockEpochId::from_bytes([9; 16]).unwrap(),
        reference_monotonic_nanoseconds: 0,
        reference_utc: UtcTimestamp(0),
        offset_to_reference: unknown(),
        drift: unknown(),
        error: unknown(),
        method_version: text("clock-v1"),
    });
    assert_eq!(
        ObservationEnvelope::new(data).unwrap_err(),
        ValidationError::ClockEpochMismatch
    );
    let channel = ChannelContext {
        band: unknown(),
        primary_channel: unknown(),
        primary_frequency: unknown(),
        center_frequency: unknown(),
        second_center_frequency: unknown(),
        width: unknown(),
        puncturing: unknown(),
    };
    let mut data = fixture();
    data.dwell = Evidence::Known(DwellContext {
        schedule_id: unknown(),
        cycle_index: unknown(),
        tuned_channel: channel,
        window: Evidence::Known(MonotonicWindow::new(timestamp(20), timestamp(30)).unwrap()),
        reported_duration: unknown(),
        method_version: text("dwell-v1"),
    });
    assert!(ObservationEnvelope::new(data).is_err());
}

#[test]
fn malformed_structured_envelopes_fail_instead_of_defaulting() {
    let valid = serde_json::to_value(ObservationEnvelope::new(fixture()).unwrap()).unwrap();
    for replacement in [
        serde_json::json!(null),
        serde_json::json!("NaN"),
        serde_json::json!({}),
        serde_json::json!([]),
    ] {
        let mut json = valid.clone();
        json["payload"]["data"]["signal"]["rssi_dbm"]["detail"] = replacement;
        assert!(serde_json::from_value::<ObservationEnvelope>(json).is_err());
    }
    let mut json = valid;
    json["quality"] = serde_json::json!(vec!["synthetic_fixture"; 33]);
    assert!(serde_json::from_value::<ObservationEnvelope>(json).is_err());
}
