use kyberia_domain::{
    capability::*, evidence::*, identity::*, observation::*, spatial::*, time::*, units::*,
};
use kyberia_survey::*;
use proptest::prelude::*;
use std::{collections::BTreeMap, num::NonZeroU32};

fn text(s: &str) -> Text {
    Text::new(s).unwrap()
}
fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}
fn stamp(n: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: ClockEpochId::from_bytes([1; 16]).unwrap(),
        nanoseconds: n,
    }
}
fn pose() -> PoseReference {
    PoseReference {
        pose_id: PoseId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([1; 16]).unwrap(),
        assignment_version: text("1"),
        position: Point3 {
            x: CoordinateMeters::new(1.).unwrap(),
            y: CoordinateMeters::new(2.).unwrap(),
            z: CoordinateMeters::new(0.).unwrap(),
        },
        covariance: Evidence::Known(
            PositionCovariance::new([0.01, 0., 0., 0.01, 0., 0.01]).unwrap(),
        ),
        orientation: unknown(),
        method_version: text("manual anchor/v1"),
    }
}
fn channel() -> ChannelContext {
    ChannelContext {
        band: Evidence::Known(Band::Ghz2_4),
        primary_channel: Evidence::Known(std::num::NonZeroU16::new(1).unwrap()),
        primary_frequency: Evidence::Known(Hertz::new(2_412_000_000.).unwrap()),
        center_frequency: unknown(),
        second_center_frequency: unknown(),
        width: unknown(),
        puncturing: unknown(),
    }
}
fn config() -> PointConfigData {
    let collector = CollectorId::from_bytes([1; 16]).unwrap();
    PointConfigData {
        schema_version: SchemaVersion::V1,
        point_id: PointId::from_bytes([1; 16]).unwrap(),
        session_id: SessionId::from_bytes([1; 16]).unwrap(),
        anchor: pose(),
        map_calibration: Evidence::Unknown(UnknownReason::NotApplicable),
        source_id: SourceId::from_bytes([1; 16]).unwrap(),
        collector_id: collector,
        adapter_version: text("1"),
        epoch: stamp(0).epoch,
        capabilities: CapabilityDocument {
            schema_version: SchemaVersion::V1,
            collector_id: collector,
            collector_version: text("1"),
            probed_at: CaptureTime {
                wall: unknown(),
                monotonic: Evidence::Known(stamp(0)),
                synchronization: unknown(),
            },
            entries: BTreeMap::from([(
                Capability::NearbyScan,
                CapabilityState::Available {
                    evidence: text("recorded fixture"),
                },
            )]),
            raw_payload_policy: RawPayloadPolicy::Discard,
        },
        mode: CaptureMode::Scan,
        required_capabilities: vec![],
        metrics: BTreeMap::from([(PointMetric::Rssi, NonZeroU32::new(2).unwrap())]),
        channels: vec![ChannelRequirement {
            frequency: Hertz::new(2_412_000_000.).unwrap(),
            minimum_dwell: Seconds::new(1.).unwrap(),
        }],
        minimum_active_time: Seconds::new(1.).unwrap(),
        maximum_scan_age: Seconds::new(0.5).unwrap(),
        target: Target::AnyBssid,
        pose_policy: PosePolicy::RequireReported {
            maximum_offset: Meters::new(0.5).unwrap(),
            maximum_axis_stddev: Meters::new(0.5).unwrap(),
        },
        allow_synthetic: true,
        method_version: text("point/v1"),
    }
}
fn observation(n: u8, time: u64) -> ObservationEnvelope {
    let cfg = config();
    ObservationEnvelope::new(EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes([n; 16]).unwrap(),
        session_id: cfg.session_id,
        source: SourceDescriptor {
            source_id: cfg.source_id,
            collector_id: cfg.collector_id,
            sensor_id: unknown(),
            adapter_id: unknown(),
            kind: SourceKind::SyntheticFixture,
            source_name: text("fixture"),
            source_version: Evidence::Known(text("1")),
            source_schema_version: text("1"),
            adapter_name: text("fixture"),
            adapter_version: text("1"),
            parser_version: text("1"),
            driver_version: unknown(),
            os_version: unknown(),
        },
        time: CaptureTime {
            wall: unknown(),
            monotonic: Evidence::Known(stamp(time)),
            synchronization: unknown(),
        },
        pose: Evidence::Known(pose()),
        channel: Evidence::Known(channel()),
        dwell: Evidence::Known(DwellContext {
            schedule_id: unknown(),
            cycle_index: unknown(),
            tuned_channel: channel(),
            window: Evidence::Known(MonotonicWindow::new(stamp(0), stamp(time)).unwrap()),
            reported_duration: unknown(),
            method_version: text("actual dwell fixture"),
        }),
        privacy: PrivacyState {
            policy_version: text("fixture"),
            identifiers: IdentifierPolicy::ProjectPseudonymized,
            payload: PayloadRetention::Discarded,
        },
        quality: vec![QualityFlag::SyntheticFixture],
        raw_source: unknown(),
        payload: ObservationPayload::Scan(ScanObservation {
            identity: RadioIdentityEvidence {
                physical_device: unknown(),
                radio: unknown(),
                bss: unknown(),
                bssid: Evidence::Known(MacAddress([0, 1, 2, 3, 4, 5])),
                ess: unknown(),
                mld: unknown(),
                link_id: unknown(),
                client: unknown(),
                grouping_evidence: unknown(),
            },
            ssid: unknown(),
            signal: SignalReading {
                rssi_dbm: Evidence::Known(Dbm::new(-60.).unwrap()),
                noise_dbm: Evidence::Unknown(UnknownReason::NotObservable),
                chains: vec![],
                calibration: Evidence::Known(CalibrationState::Uncalibrated),
                measurement_method: text("fixture"),
            },
            information_elements: unknown(),
            result_age: Evidence::Known(Seconds::new(0.).unwrap()),
        }),
    })
    .unwrap()
}
fn start() -> PointSurvey {
    PointSurvey::start(PointConfig::new(config()).unwrap(), stamp(0)).unwrap()
}

#[test]
fn completion_uses_evidence_counts_and_actual_dwell_union() {
    let state = start();
    assert!(!state.progress().ready);
    assert!(state.finish(stamp(10_000_000_000)).is_err());
    let first = state
        .admit(&observation(1, 500_000_000), stamp(500_000_000))
        .unwrap();
    assert_eq!(first.progress().metrics[&PointMetric::Rssi], 1);
    assert!(!first.progress().ready);
    let second = first
        .admit(&observation(2, 1_000_000_000), stamp(1_000_000_000))
        .unwrap();
    assert_eq!(second.progress().dwell_seconds[0].get(), 1.);
    assert!(second.progress().ready);
    let complete = second.finish(stamp(1_000_000_000)).unwrap();
    assert_eq!(complete.phase(), &PointPhase::Completed);
    assert!(
        complete
            .admit(&observation(3, 2_000_000_000), stamp(2_000_000_000))
            .is_err()
    );
}

#[test]
fn unsupported_and_unknown_noise_never_complete() {
    let mut cfg = config();
    cfg.metrics
        .insert(PointMetric::Noise, NonZeroU32::new(1).unwrap());
    assert!(PointSurvey::start(PointConfig::new(cfg.clone()).unwrap(), stamp(0)).is_err());
    cfg.capabilities.entries.insert(
        Capability::NoiseDbm,
        CapabilityState::Available {
            evidence: text("optional per scan"),
        },
    );
    let state = PointSurvey::start(PointConfig::new(cfg).unwrap(), stamp(0))
        .unwrap()
        .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
        .unwrap()
        .admit(&observation(2, 2_000_000_000), stamp(2_000_000_000))
        .unwrap();
    assert_eq!(state.progress().metrics[&PointMetric::Noise], 0);
    assert!(!state.progress().ready);
}

#[test]
fn pause_gap_excludes_samples_elapsed_time_and_channel_dwell() {
    let state = start()
        .admit(&observation(1, 500_000_000), stamp(500_000_000))
        .unwrap()
        .pause(stamp(500_000_000))
        .unwrap();
    assert!(
        state
            .admit(&observation(2, 1_000_000_000), stamp(1_000_000_000))
            .is_err()
    );
    let state = state.resume(stamp(10_000_000_000)).unwrap();
    assert_eq!(state.progress().active_seconds.get(), 0.5);
    assert!(
        state
            .admit(&observation(2, 9_000_000_000), stamp(10_000_000_000))
            .is_err()
    );
    let resumed = state
        .admit(&observation(3, 10_500_000_000), stamp(10_500_000_000))
        .unwrap();
    assert_eq!(resumed.progress().active_seconds.get(), 1.);
    assert_eq!(resumed.progress().dwell_seconds[0].get(), 1.);
}

#[test]
fn stale_cached_scan_and_duplicate_source_sample_are_rejected() {
    let state = start();
    let mut raw = observation(1, 1_000_000_000).into_data();
    let ObservationPayload::Scan(scan) = &mut raw.payload else {
        unreachable!()
    };
    scan.result_age = Evidence::Known(Seconds::new(2.).unwrap());
    assert!(
        state
            .admit(
                &ObservationEnvelope::new(raw).unwrap(),
                stamp(1_000_000_000)
            )
            .is_err()
    );
    let first = state
        .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
        .unwrap();
    assert!(
        first
            .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
            .is_err()
    );
    assert!(
        first
            .admit(&observation(2, 1_000_000_000), stamp(1_000_000_000))
            .is_err()
    );
}

#[test]
fn wrong_session_source_frame_or_clock_never_changes_point() {
    let state = start();
    let original = serde_json::to_vec(&state).unwrap();
    let mut raw = observation(1, 1_000_000_000).into_data();
    raw.session_id = SessionId::from_bytes([9; 16]).unwrap();
    assert!(
        state
            .admit(
                &ObservationEnvelope::new(raw).unwrap(),
                stamp(1_000_000_000)
            )
            .is_err()
    );
    let mut raw = observation(1, 1_000_000_000).into_data();
    raw.source.source_id = SourceId::from_bytes([9; 16]).unwrap();
    assert!(
        state
            .admit(
                &ObservationEnvelope::new(raw).unwrap(),
                stamp(1_000_000_000)
            )
            .is_err()
    );
    let mut raw = observation(1, 1_000_000_000).into_data();
    let Evidence::Known(pose) = &mut raw.pose else {
        unreachable!()
    };
    pose.frame_id = FrameId::from_bytes([9; 16]).unwrap();
    assert!(
        state
            .admit(
                &ObservationEnvelope::new(raw).unwrap(),
                stamp(1_000_000_000)
            )
            .is_err()
    );
    let mut wrong = stamp(2_000_000_000);
    wrong.epoch = ClockEpochId::from_bytes([9; 16]).unwrap();
    assert!(state.advance(wrong).is_err());
    assert_eq!(serde_json::to_vec(&state).unwrap(), original);
}

#[test]
fn serialized_state_is_revalidated_and_replay_is_deterministic() {
    let a = start()
        .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
        .unwrap();
    let b = start()
        .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(
        serde_json::from_slice::<PointSurvey>(&serde_json::to_vec(&a).unwrap()).unwrap(),
        a
    );
    let mut invalid = serde_json::to_value(&a).unwrap();
    invalid["phase"] = serde_json::json!("completed");
    assert!(serde_json::from_value::<PointSurvey>(invalid).is_err());
}

#[test]
fn cancelled_and_failed_points_preserve_partial_evidence() {
    let state = start()
        .admit(&observation(1, 500_000_000), stamp(500_000_000))
        .unwrap();
    let cancelled = state.cancel(stamp(600_000_000)).unwrap();
    assert_eq!(cancelled.progress().metrics[&PointMetric::Rssi], 1);
    assert!(cancelled.resume(stamp(700_000_000)).is_err());
    let failed = state
        .fail(stamp(600_000_000), text("adapter disconnected"))
        .unwrap();
    assert!(matches!(failed.phase(), PointPhase::Failed { .. }));
}

proptest! {
    #[test]
    fn increasing_elapsed_time_without_new_evidence_never_satisfies_sample_count(t in 1_u64..100_000_000_000) {
        let state=start().advance(stamp(t)).unwrap();
        prop_assert!(!state.progress().ready);
        prop_assert_eq!(state.progress().metrics[&PointMetric::Rssi],0);
    }
    #[test]
    fn dwell_sweep_matches_discrete_union_intersection_oracle(lengths in prop::collection::vec(0_u64..50, 1..50)) {
        let mut state = start();
        let mut now = 0_u64;
        let mut dwell_intervals = Vec::new();
        for (index, length) in lengths.into_iter().enumerate() {
            now += 10;
            let lower = now.saturating_sub(length);
            let mut raw = observation((index + 1) as u8, now).into_data();
            let Evidence::Known(dwell) = &mut raw.dwell else { unreachable!() };
            dwell.window = Evidence::Known(MonotonicWindow::new(stamp(lower), stamp(now)).unwrap());
            dwell_intervals.push((lower, now));
            state = state.admit(&ObservationEnvelope::new(raw).unwrap(), stamp(now)).unwrap();
            if index % 3 == 0 {
                state = state.pause(stamp(now)).unwrap().resume(stamp(now + 5)).unwrap();
                now += 5;
            }
        }
        let progress = state.progress();
        let oracle = (0..now).filter(|tick| dwell_intervals.iter().any(|(lo,hi)| tick >= lo && tick < hi)
            && progress.active_windows.iter().any(|window| *tick >= window.start().nanoseconds && *tick < window.end().nanoseconds)).count();
        prop_assert!((progress.dwell_seconds[0].get() - oracle as f64 / 1e9).abs() < 1e-18);
    }
}

#[test]
fn cache_origin_and_queue_delay_are_both_checked() {
    let first = start()
        .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
        .unwrap();
    let mut cached = observation(2, 1_000_000_000).into_data();
    let ObservationPayload::Scan(scan) = &mut cached.payload else {
        unreachable!()
    };
    scan.result_age = Evidence::Known(Seconds::new(0.25).unwrap());
    assert_eq!(
        first.admit(
            &ObservationEnvelope::new(cached).unwrap(),
            stamp(1_250_000_000)
        ),
        Err(SurveyError::DuplicateSourceSample)
    );
    assert_eq!(
        start().admit(&observation(1, 1_000_000_000), stamp(2_000_000_000)),
        Err(SurveyError::StaleScan)
    );
}

#[test]
fn actual_capture_time_is_never_reinterpreted_as_retrieval_time() {
    let state =
        PointSurvey::start(PointConfig::new(config()).unwrap(), stamp(1_000_000_000)).unwrap();
    let mut raw = observation(1, 1_100_000_000).into_data();
    let ObservationPayload::Scan(scan) = &mut raw.payload else {
        unreachable!()
    };
    scan.result_age = Evidence::Known(Seconds::new(0.2).unwrap());
    let accepted = state
        .admit(
            &ObservationEnvelope::new(raw.clone()).unwrap(),
            stamp(1_300_000_000),
        )
        .unwrap();
    assert_eq!(accepted.progress().metrics[&PointMetric::Rssi], 1);
    assert_eq!(
        serde_json::from_value::<PointSurvey>(serde_json::to_value(&accepted).unwrap()).unwrap(),
        accepted
    );
    assert_eq!(
        state.admit(
            &ObservationEnvelope::new(raw.clone()).unwrap(),
            stamp(1_200_000_000)
        ),
        Err(SurveyError::InconsistentScanAge)
    );
    raw.time.monotonic = unknown();
    assert_eq!(
        state.admit(
            &ObservationEnvelope::new(raw).unwrap(),
            stamp(1_300_000_000)
        ),
        Err(SurveyError::TimestampUnavailable)
    );
}

#[test]
fn admitted_observation_query_rejects_same_id_substitutions() {
    let original = observation(1, 500_000_000);
    let accepted = start().admit(&original, stamp(500_000_000)).unwrap();
    assert_eq!(accepted.validate_admitted_observation(&original), Ok(()));

    let mut changed_rssi = original.clone().into_data();
    let ObservationPayload::Scan(scan) = &mut changed_rssi.payload else {
        unreachable!()
    };
    scan.signal.rssi_dbm = Evidence::Known(Dbm::new(-61.).unwrap());
    assert_eq!(
        accepted.validate_admitted_observation(&ObservationEnvelope::new(changed_rssi).unwrap()),
        Err(SurveyError::InvalidSnapshot)
    );

    let mut changed_time = original.clone().into_data();
    changed_time.time.monotonic = Evidence::Known(stamp(400_000_000));
    assert_eq!(
        accepted.validate_admitted_observation(&ObservationEnvelope::new(changed_time).unwrap()),
        Err(SurveyError::InvalidSnapshot)
    );

    let mut changed_pose = original.into_data();
    let mut pose = pose();
    pose.position.x = CoordinateMeters::new(1.1).unwrap();
    changed_pose.pose = Evidence::Known(pose);
    assert_eq!(
        accepted.validate_admitted_observation(&ObservationEnvelope::new(changed_pose).unwrap()),
        Err(SurveyError::InvalidSnapshot)
    );
}

#[test]
fn monitor_frames_and_finite_snr_require_actual_supported_evidence() {
    let mut cfg = config();
    cfg.mode = CaptureMode::Frame;
    cfg.metrics
        .insert(PointMetric::Snr, NonZeroU32::new(1).unwrap());
    assert!(PointSurvey::start(PointConfig::new(cfg.clone()).unwrap(), stamp(0)).is_err());
    for capability in [Capability::MonitorFrames, Capability::NoiseDbm] {
        cfg.capabilities.entries.insert(
            capability,
            CapabilityState::Available {
                evidence: text("fixture"),
            },
        );
    }
    let start = PointSurvey::start(PointConfig::new(cfg).unwrap(), stamp(0)).unwrap();
    let mut raw = observation(1, 1_000_000_000).into_data();
    let ObservationPayload::Scan(mut scan) = raw.payload else {
        unreachable!()
    };
    scan.signal.noise_dbm = Evidence::Known(Dbm::new(-90.).unwrap());
    raw.payload = ObservationPayload::Frame(FrameMetadata {
        identity: scan.identity,
        signal: scan.signal,
        frame_type: unknown(),
        frame_subtype: unknown(),
        retry: unknown(),
        length_bytes: 128,
        phy_rate_mbps: unknown(),
        raw_information_elements: unknown(),
    });
    let state = start
        .admit(
            &ObservationEnvelope::new(raw.clone()).unwrap(),
            stamp(1_000_000_000),
        )
        .unwrap();
    assert_eq!(state.progress().metrics[&PointMetric::Snr], 1);
    let ObservationPayload::Frame(frame) = &mut raw.payload else {
        unreachable!()
    };
    frame.signal.rssi_dbm = Evidence::Known(Dbm::new(f64::MAX).unwrap());
    frame.signal.noise_dbm = Evidence::Known(Dbm::new(-f64::MAX).unwrap());
    assert_eq!(
        start
            .admit(
                &ObservationEnvelope::new(raw).unwrap(),
                stamp(1_000_000_000)
            )
            .unwrap()
            .progress()
            .metrics[&PointMetric::Snr],
        0
    );
}

#[test]
fn manual_pose_is_explicit_and_reported_pose_is_bounded() {
    let mut raw = observation(1, 1_000_000_000).into_data();
    raw.pose = unknown();
    let observation = ObservationEnvelope::new(raw.clone()).unwrap();
    assert_eq!(
        start().admit(&observation, stamp(1_000_000_000)),
        Err(SurveyError::PoseUnavailable)
    );
    let mut cfg = config();
    cfg.pose_policy = PosePolicy::ManualAnchor {
        maximum_reported_offset: Meters::new(0.5).unwrap(),
    };
    let state = PointSurvey::start(PointConfig::new(cfg).unwrap(), stamp(0))
        .unwrap()
        .admit(&observation, stamp(1_000_000_000))
        .unwrap();
    assert!(state.progress().manual_position_assumption);
    let mut distant = pose();
    distant.position.x = CoordinateMeters::new(2.).unwrap();
    raw.pose = Evidence::Known(distant);
    assert_eq!(
        start().admit(
            &ObservationEnvelope::new(raw.clone()).unwrap(),
            stamp(1_000_000_000)
        ),
        Err(SurveyError::PoseOutsidePoint)
    );
    let mut uncertain = pose();
    uncertain.covariance =
        Evidence::Known(PositionCovariance::new([1., 0., 0., 1., 0., 1.]).unwrap());
    raw.pose = Evidence::Known(uncertain);
    assert_eq!(
        start().admit(
            &ObservationEnvelope::new(raw).unwrap(),
            stamp(1_000_000_000)
        ),
        Err(SurveyError::PoseUncertain)
    );
}

#[test]
fn unknown_and_incomplete_channel_dwell_do_not_make_coverage() {
    for missing in [false, true] {
        let mut raw = observation(1, 1_000_000_000).into_data();
        if missing {
            raw.dwell = unknown();
        } else {
            raw.quality.push(QualityFlag::DroppedEvents);
        }
        let state = start()
            .admit(
                &ObservationEnvelope::new(raw).unwrap(),
                stamp(1_000_000_000),
            )
            .unwrap();
        assert_eq!(state.progress().dwell_seconds[0].get(), 0.);
        assert_eq!(state.progress().metrics[&PointMetric::Rssi], 1);
        assert!(!state.progress().ready);
    }
}

#[test]
fn target_filter_does_not_mix_distinct_bssids() {
    let mut cfg = config();
    cfg.target = Target::Bssid(MacAddress([9; 6]));
    let state = PointSurvey::start(PointConfig::new(cfg).unwrap(), stamp(0))
        .unwrap()
        .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
        .unwrap();
    assert_eq!(state.progress().metrics[&PointMetric::Rssi], 0);
    assert_eq!(state.progress().dwell_seconds[0].get(), 1.);
}

#[test]
fn invalid_configuration_and_future_schema_are_rejected() {
    let original = config();
    let mut cfg = original.clone();
    cfg.channels.push(cfg.channels[0].clone());
    assert!(PointConfig::new(cfg).is_err());
    let mut cfg = original.clone();
    cfg.collector_id = CollectorId::from_bytes([9; 16]).unwrap();
    assert!(PointConfig::new(cfg).is_err());
    let mut cfg = original.clone();
    cfg.metrics.clear();
    assert!(PointConfig::new(cfg).is_err());
    let mut cfg = original.clone();
    cfg.minimum_active_time = Seconds::new(f64::MAX).unwrap();
    assert!(PointConfig::new(cfg).is_err());
    let mut value = serde_json::to_value(PointConfig::new(original).unwrap()).unwrap();
    value["schema_version"] = serde_json::json!("99");
    assert!(serde_json::from_value::<PointConfig>(value).is_err());
    let duplicate = serde_json::to_string(&PointConfig::new(config()).unwrap())
        .unwrap()
        .replace("\"rssi\":2", "\"rssi\":2,\"rssi\":1");
    assert!(serde_json::from_str::<PointConfig>(&duplicate).is_err());
}

#[test]
fn malformed_snapshots_cannot_forge_coverage_or_temporal_consistency() {
    let state = start()
        .admit(&observation(1, 1_000_000_000), stamp(1_000_000_000))
        .unwrap();
    let original = serde_json::to_value(state).unwrap();
    for mutate in 0..7 {
        let mut value = original.clone();
        match mutate {
            0 => value["windows"][0]["start"] = serde_json::json!(2_000_000_000_u64),
            1 => value["records"][0]["captured"] = serde_json::json!(2_000_000_000_u64),
            2 => value["records"][0]["admitted"] = serde_json::json!(3_000_000_000_u64),
            3 => {
                let record = value["records"][0].clone();
                value["records"].as_array_mut().unwrap().push(record);
            }
            4 => value["records"][0]["quality"] = serde_json::json!(["dropped_events"]),
            5 => value["phase"] = serde_json::json!("paused"),
            6 => value["config"]["allow_synthetic"] = serde_json::json!(false),
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<PointSurvey>(value).is_err(),
            "mutation {mutate}"
        );
    }
}

#[test]
#[ignore = "release benchmark; run explicitly with --release --ignored --nocapture"]
fn bounded_point_benchmark() {
    let mut state = start();
    let started = std::time::Instant::now();
    for i in 1_u64..=4096 {
        let time = i * 1_000_000;
        let mut raw = observation(1, time).into_data();
        raw.id = ObservationId::from_bytes(u128::from(i).to_be_bytes()).unwrap();
        state = state
            .admit(&ObservationEnvelope::new(raw).unwrap(), stamp(time))
            .unwrap();
        if i % 4 == 0 && i < 4096 {
            state = state
                .pause(stamp(time))
                .unwrap()
                .resume(stamp(time + 1))
                .unwrap();
        }
    }
    let ingestion = started.elapsed();
    let started = std::time::Instant::now();
    let progress = state.progress();
    let progress_time = started.elapsed();
    assert_eq!(progress.metrics[&PointMetric::Rssi], 4096);
    assert!(progress.dwell_seconds[0].get() <= progress.active_seconds.get());
    eprintln!(
        "records=4096 windows=1024 admission_ms={:.3} progress_ms={:.3}",
        ingestion.as_secs_f64() * 1000.,
        progress_time.as_secs_f64() * 1000.
    );
}
