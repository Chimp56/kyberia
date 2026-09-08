use kyberia_domain::{
    capability::*, evidence::*, identity::*, observation::*, spatial::*, time::*, units::*,
};
use kyberia_survey::*;
use std::{collections::BTreeMap, num::NonZeroU32};

fn text(value: &str) -> Text {
    Text::new(value).unwrap()
}

fn epoch() -> ClockEpochId {
    ClockEpochId::from_bytes([1; 16]).unwrap()
}

fn stamp(nanoseconds: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds,
    }
}

fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}

fn anchor() -> PoseReference {
    PoseReference {
        pose_id: PoseId::from_bytes([2; 16]).unwrap(),
        frame_id: FrameId::from_bytes([3; 16]).unwrap(),
        assignment_version: text("anchor/v1"),
        position: Point3 {
            x: CoordinateMeters::new(2.).unwrap(),
            y: CoordinateMeters::new(4.).unwrap(),
            z: CoordinateMeters::new(0.).unwrap(),
        },
        covariance: Evidence::Known(
            PositionCovariance::new([0.01, 0., 0., 0.01, 0., 0.01]).unwrap(),
        ),
        orientation: unknown(),
        method_version: text("manual/v1"),
    }
}

fn config() -> PointConfig {
    let collector_id = CollectorId::from_bytes([4; 16]).unwrap();
    PointConfig::new(PointConfigData {
        schema_version: SchemaVersion::V1,
        point_id: PointId::from_bytes([5; 16]).unwrap(),
        session_id: SessionId::from_bytes([6; 16]).unwrap(),
        anchor: anchor(),
        map_calibration: Evidence::Unknown(UnknownReason::NotApplicable),
        source_id: SourceId::from_bytes([7; 16]).unwrap(),
        collector_id,
        adapter_version: text("collector/1"),
        epoch: epoch(),
        capabilities: CapabilityDocument {
            schema_version: SchemaVersion::V1,
            collector_id,
            collector_version: text("collector/1"),
            probed_at: CaptureTime {
                wall: unknown(),
                monotonic: Evidence::Known(stamp(0)),
                synchronization: unknown(),
            },
            entries: BTreeMap::from([(
                Capability::NearbyScan,
                CapabilityState::Available {
                    evidence: text("test capability"),
                },
            )]),
            raw_payload_policy: RawPayloadPolicy::Discard,
        },
        mode: CaptureMode::Scan,
        required_capabilities: vec![],
        metrics: BTreeMap::from([(PointMetric::Rssi, NonZeroU32::new(1).unwrap())]),
        channels: vec![],
        minimum_active_time: Seconds::new(0.).unwrap(),
        maximum_scan_age: Seconds::new(1.).unwrap(),
        target: Target::AnyBssid,
        pose_policy: PosePolicy::ManualAnchor {
            maximum_reported_offset: Meters::new(1.).unwrap(),
        },
        allow_synthetic: false,
        method_version: text("point/v1"),
    })
    .unwrap()
}

fn source() -> SourceDescriptor {
    let cfg = config();
    SourceDescriptor {
        source_id: cfg.data().source_id,
        collector_id: cfg.data().collector_id,
        sensor_id: unknown(),
        adapter_id: unknown(),
        kind: SourceKind::NativeApi,
        source_name: text("CoreWLAN"),
        source_version: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        source_schema_version: text("macos-wire/1"),
        adapter_name: text("kyberia-macos"),
        adapter_version: text("collector/1"),
        parser_version: text("parser/1"),
        driver_version: unknown(),
        os_version: unknown(),
    }
}

fn envelope(id: u8) -> ObservationEnvelope {
    ObservationEnvelope::new(EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes([id; 16]).unwrap(),
        session_id: config().data().session_id,
        source: source(),
        time: CaptureTime {
            wall: Evidence::Unknown(UnknownReason::NotMeasured),
            monotonic: Evidence::Unknown(UnknownReason::NotMeasured),
            synchronization: Evidence::Unknown(UnknownReason::ClockUnavailable),
        },
        pose: Evidence::Unknown(UnknownReason::NotMeasured),
        channel: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        dwell: Evidence::Unknown(UnknownReason::NotObservable),
        privacy: PrivacyState {
            policy_version: text("privacy/v1"),
            identifiers: IdentifierPolicy::OwnedInfrastructure,
            payload: PayloadRetention::Discarded,
        },
        quality: vec![QualityFlag::ClockUncertain],
        raw_source: Evidence::Unknown(UnknownReason::NotRetained),
        payload: ObservationPayload::Scan(ScanObservation {
            identity: RadioIdentityEvidence {
                physical_device: unknown(),
                radio: unknown(),
                bss: unknown(),
                bssid: Evidence::Known(MacAddress([0, 1, 2, 3, 4, id])),
                ess: unknown(),
                mld: unknown(),
                link_id: unknown(),
                client: Evidence::Unknown(UnknownReason::NotApplicable),
                grouping_evidence: unknown(),
            },
            ssid: unknown(),
            signal: SignalReading {
                rssi_dbm: Evidence::Known(Dbm::new(-55.).unwrap()),
                noise_dbm: Evidence::Unknown(UnknownReason::NotObservable),
                chains: vec![],
                calibration: Evidence::Known(CalibrationState::Uncalibrated),
                measurement_method: text("native scan"),
            },
            information_elements: Evidence::Unknown(UnknownReason::NotRetained),
            result_age: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        }),
    })
    .unwrap()
}

fn response(returned: Option<u64>, window: Option<(u64, u64)>) -> SourceResponseTiming {
    let returned_at = CaptureTime {
        wall: Evidence::Unknown(UnknownReason::ClockUnavailable),
        monotonic: returned.map_or_else(
            || Evidence::Unknown(UnknownReason::ClockUnavailable),
            |n| Evidence::Known(stamp(n)),
        ),
        synchronization: Evidence::Unknown(UnknownReason::ClockUnavailable),
    };
    let api_window = window.map_or_else(
        || Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        |(start, end)| Evidence::Known(MonotonicWindow::new(stamp(start), stamp(end)).unwrap()),
    );
    SourceResponseTiming::new(returned_at, api_window).unwrap()
}

fn received(id: u8, returned: Option<u64>, window: Option<(u64, u64)>) -> ReceivedObservation {
    ReceivedObservation::new(envelope(id), Evidence::Known(response(returned, window))).unwrap()
}

fn start() -> PointSurvey {
    PointSurvey::start(config(), stamp(100)).unwrap()
}

fn strict_envelope(id: u8, captured: u64) -> ObservationEnvelope {
    let mut data = envelope(id).into_data();
    data.quality.clear();
    data.pose = Evidence::Known(anchor());
    data.time.monotonic = Evidence::Known(stamp(captured));
    let ObservationPayload::Scan(scan) = &mut data.payload else {
        unreachable!()
    };
    scan.result_age = Evidence::Known(Seconds::new(0.).unwrap());
    ObservationEnvelope::new(data).unwrap()
}

#[test]
fn receipt_association_preserves_unknown_capture_and_does_not_complete_point() {
    let (state, association) = start()
        .associate_received(&received(1, Some(150), Some((120, 140))))
        .unwrap();
    assert_eq!(state.associations(), std::slice::from_ref(&association));
    assert_eq!(association.point_id(), config().data().point_id);
    assert_eq!(
        association.observation_id(),
        ObservationId::from_bytes([1; 16]).unwrap()
    );
    assert!(matches!(
        association.time_basis(),
        PointAssociationTimeBasis::Receipt { returned_at } if returned_at.nanoseconds == 150
    ));
    assert!(matches!(
        association.capture_time().monotonic,
        Evidence::Unknown(UnknownReason::NotMeasured)
    ));
    assert!(matches!(
        association.dwell(),
        Evidence::Unknown(UnknownReason::NotObservable)
    ));
    assert!(matches!(
        association.result_age(),
        Evidence::Unknown(UnknownReason::SourceDidNotProvide)
    ));
    assert!(matches!(
        association.observation_pose(),
        Evidence::Unknown(UnknownReason::NotMeasured)
    ));
    assert!(association.quality().contains(&QualityFlag::ClockUncertain));
    assert!(!association.counts_toward_strict_point_gate());
    assert_eq!(state.progress().metrics[&PointMetric::Rssi], 0);
    assert_eq!(state.progress().associated_observation_ids.len(), 1);
    assert!(!state.progress().ready);

    let mut cfg = config().data().clone();
    cfg.minimum_active_time = Seconds::new(2.).unwrap();
    let strict = PointSurvey::start(PointConfig::new(cfg).unwrap(), stamp(0))
        .unwrap()
        .admit(&strict_envelope(1, 100_000_000), stamp(100_000_000))
        .unwrap();
    let before = strict.progress();
    assert_eq!(before.metrics[&PointMetric::Rssi], 1);
    assert_eq!(before.active_seconds.get(), 0.1);
    assert!(!before.ready);

    let (associated, _) = strict
        .associate_received(&received(2, Some(1_100_000_000), None))
        .unwrap();
    let after = associated.progress();
    assert_eq!(after.metrics[&PointMetric::Rssi], 1);
    assert_eq!(after.active_seconds.get(), 0.1);
    assert!(!after.ready);
    assert_eq!(
        associated.finish(stamp(1_100_000_000)),
        Err(SurveyError::NotReady)
    );
}

#[test]
fn receipt_boundary_and_outside_windows_are_deterministic() {
    assert!(
        start()
            .associate_received(&received(1, Some(99), Some((90, 95))))
            .is_err()
    );
    let (state, _) = start()
        .associate_received(&received(1, Some(100), Some((90, 95))))
        .unwrap();
    assert_eq!(state.associations().len(), 1);
    assert_eq!(
        start().associate_received(&received(1, None, None)),
        Err(SurveyError::AssociationTimeUnavailable)
    );
}

#[test]
fn api_window_association_requires_a_nonambiguous_active_interval() {
    let (state, association) = start()
        .associate_received(&received(1, None, Some((100, 120))))
        .unwrap();
    assert!(matches!(
        association.time_basis(),
        PointAssociationTimeBasis::ApiWindow { window }
            if window.start().nanoseconds == 100 && window.end().nanoseconds == 120
    ));
    assert_eq!(state.associations().len(), 1);
    assert_eq!(
        start().associate_received(&received(1, None, Some((99, 120)))),
        Err(SurveyError::AssociationAmbiguous)
    );
    assert_eq!(
        start().associate_received(&received(1, None, Some((80, 100)))),
        Err(SurveyError::AssociationOutsidePoint)
    );
}

#[test]
fn pause_resume_restricts_receipt_associations_to_the_current_active_window() {
    let state = start()
        .associate_received(&received(1, Some(150), None))
        .unwrap()
        .0
        .pause(stamp(200))
        .unwrap()
        .resume(stamp(300))
        .unwrap();
    assert_eq!(
        state.associate_received(&received(2, Some(250), None)),
        Err(SurveyError::AssociationOutsidePoint)
    );
    let (resumed, _) = state
        .associate_received(&received(2, Some(300), None))
        .unwrap();
    assert_eq!(resumed.associations().len(), 2);
    assert_eq!(resumed.progress().active_windows.len(), 2);
}

#[test]
fn wrong_session_source_or_reported_frame_is_rejected_without_mutating_state() {
    let baseline = start();
    let mut data = envelope(1).into_data();
    data.session_id = SessionId::from_bytes([8; 16]).unwrap();
    let wrong_session = ReceivedObservation::new(
        ObservationEnvelope::new(data).unwrap(),
        Evidence::Known(response(Some(150), None)),
    )
    .unwrap();
    assert_eq!(
        baseline.associate_received(&wrong_session),
        Err(SurveyError::WrongSession)
    );

    let mut data = envelope(2).into_data();
    data.source.source_id = SourceId::from_bytes([8; 16]).unwrap();
    let wrong_source = ReceivedObservation::new(
        ObservationEnvelope::new(data).unwrap(),
        Evidence::Known(response(Some(150), None)),
    )
    .unwrap();
    assert_eq!(
        baseline.associate_received(&wrong_source),
        Err(SurveyError::WrongSource)
    );

    let mut data = envelope(3).into_data();
    data.pose = Evidence::Known(PoseReference {
        frame_id: FrameId::from_bytes([9; 16]).unwrap(),
        ..anchor()
    });
    let wrong_frame = ReceivedObservation::new(
        ObservationEnvelope::new(data).unwrap(),
        Evidence::Known(response(Some(150), None)),
    )
    .unwrap();
    assert_eq!(
        baseline.associate_received(&wrong_frame),
        Err(SurveyError::WrongFrame)
    );
    assert!(baseline.associations().is_empty());
}

#[test]
fn duplicate_malformed_and_future_association_evidence_fail_closed() {
    let (state, _) = start()
        .associate_received(&received(1, Some(150), None))
        .unwrap();
    assert_eq!(
        state.associate_received(&received(1, Some(150), None)),
        Err(SurveyError::DuplicateAssociation)
    );

    let mut data = envelope(2).into_data();
    data.quality.push(QualityFlag::Malformed);
    let malformed = ReceivedObservation::new(
        ObservationEnvelope::new(data).unwrap(),
        Evidence::Known(response(Some(150), None)),
    )
    .unwrap();
    assert_eq!(
        start().associate_received(&malformed),
        Err(SurveyError::UnusableQuality)
    );

    let mut future = serde_json::to_value(&state).unwrap();
    future["associations"][0]["schema_version"] = serde_json::json!("2");
    assert!(serde_json::from_value::<PointSurvey>(future).is_err());
    let mut malformed_wire = serde_json::to_value(&state).unwrap();
    malformed_wire["associations"][0]
        .as_object_mut()
        .unwrap()
        .remove("source_response");
    assert!(serde_json::from_value::<PointSurvey>(malformed_wire).is_err());

    let mut uncertain_wire = serde_json::to_value(&state).unwrap();
    uncertain_wire["associations"][0]["temporal_uncertainty"] =
        serde_json::json!({"state":"known", "detail":0.1});
    assert!(serde_json::from_value::<PointSurvey>(uncertain_wire).is_err());

    let mut wrong_unknown_wire = serde_json::to_value(&state).unwrap();
    wrong_unknown_wire["associations"][0]["temporal_uncertainty"] =
        serde_json::json!({"state":"unknown", "detail":"source_did_not_provide"});
    assert!(serde_json::from_value::<PointSurvey>(wrong_unknown_wire).is_err());

    let mut synthetic_wire = serde_json::to_value(&state).unwrap();
    synthetic_wire["associations"][0]["quality"] =
        serde_json::json!(["clock_uncertain", "synthetic_fixture"]);
    assert!(serde_json::from_value::<PointSurvey>(synthetic_wire).is_err());

    let mut allowed_config = config().data().clone();
    allowed_config.allow_synthetic = true;
    let allowed = PointSurvey::start(PointConfig::new(allowed_config).unwrap(), stamp(100))
        .unwrap()
        .associate_received(&received(3, Some(150), None))
        .unwrap()
        .0;
    let mut allowed_wire = serde_json::to_value(&allowed).unwrap();
    allowed_wire["associations"][0]["quality"] =
        serde_json::json!(["clock_uncertain", "synthetic_fixture"]);
    assert!(serde_json::from_value::<PointSurvey>(allowed_wire).is_ok());

    let strict = start().admit(&strict_envelope(1, 120), stamp(150)).unwrap();
    assert_eq!(
        strict.associate_received(&received(1, Some(160), None)),
        Err(SurveyError::DuplicateObservation)
    );

    let associated = start()
        .associate_received(&received(2, Some(150), None))
        .unwrap()
        .0;
    assert_eq!(
        associated.admit(&strict_envelope(2, 160), stamp(170)),
        Err(SurveyError::DuplicateObservation)
    );

    let both = start()
        .admit(&strict_envelope(1, 120), stamp(150))
        .unwrap()
        .associate_received(&received(2, Some(160), None))
        .unwrap()
        .0;
    let mut collision_wire = serde_json::to_value(&both).unwrap();
    let strict_id = collision_wire["records"][0]["observation_id"].clone();
    collision_wire["associations"][0]["observation_id"] = strict_id;
    assert!(serde_json::from_value::<PointSurvey>(collision_wire).is_err());

    let (state, _) = start()
        .associate_received(&received(1, Some(150), None))
        .unwrap();
    let mut wire = serde_json::to_value(&state).unwrap();
    let association = wire["associations"][0].clone();
    wire["associations"] = serde_json::Value::Array(vec![association; 4097]);
    assert!(serde_json::from_value::<PointSurvey>(wire).is_err());
}

#[test]
fn foreign_receipt_clock_and_health_payload_cannot_be_associated() {
    let mut foreign_time = CaptureTime {
        wall: Evidence::Unknown(UnknownReason::ClockUnavailable),
        monotonic: Evidence::Known(stamp(150)),
        synchronization: Evidence::Unknown(UnknownReason::ClockUnavailable),
    };
    foreign_time.monotonic = Evidence::Known(MonotonicTimestamp {
        epoch: ClockEpochId::from_bytes([9; 16]).unwrap(),
        nanoseconds: 150,
    });
    let foreign_response = SourceResponseTiming::new(
        foreign_time,
        Evidence::Unknown(UnknownReason::SourceDidNotProvide),
    )
    .unwrap();
    let foreign = ReceivedObservation::new(envelope(2), Evidence::Known(foreign_response)).unwrap();
    assert_eq!(
        start().associate_received(&foreign),
        Err(SurveyError::WrongClock)
    );

    let mut data = envelope(3).into_data();
    data.payload = ObservationPayload::Health(CaptureHealth {
        dropped_events: Evidence::Known(0),
        queued_events: Evidence::Known(0),
        connected: Evidence::Known(true),
        diagnostic: text("healthy"),
    });
    let health = ReceivedObservation::new(
        ObservationEnvelope::new(data).unwrap(),
        Evidence::Known(response(Some(150), None)),
    )
    .unwrap();
    assert_eq!(
        start().associate_received(&health),
        Err(SurveyError::UnsupportedPayload)
    );
}

#[test]
fn association_serialization_replays_deterministically() {
    let (state, _) = start()
        .associate_received(&received(1, Some(150), Some((120, 140))))
        .unwrap();
    let bytes = serde_json::to_vec(&state).unwrap();
    let replay: PointSurvey = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(replay, state);
    assert_eq!(serde_json::to_vec(&replay).unwrap(), bytes);
}
