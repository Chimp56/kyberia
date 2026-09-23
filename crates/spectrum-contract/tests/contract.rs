use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{
        CalibrationId, ClockEpochId, ContentHash, FrameId, PoseId, SensorId, SessionId, SourceId,
        Text,
    },
    spatial::{Orientation, Point3, PoseReference, PositionCovariance},
    time::{CaptureTime, MonotonicTimestamp, MonotonicWindow, UtcTimestamp, WallClockReading},
    units::{CoordinateMeters, Radians, Seconds},
};
use kyberia_spectrum_contract::{
    AcquisitionSettings, CALIBRATION_SCHEMA_V1, DetectorKind, FrequencyGrid, GainSetting,
    PowerUnit, ProcessingLimits, SWEEP_SCHEMA_V1, SignaturePattern, SignatureReason, SpectrumBin,
    SpectrumCalibrationProfile, SpectrumError, SpectrumEvent, SpectrumSourceKind,
    SpectrumSourceMetadata, SpectrumSweep, SpectrumSweepDocument, WindowFunction,
};

fn limits() -> ProcessingLimits {
    ProcessingLimits::conservative()
}

fn source_id() -> SourceId {
    SourceId::from_bytes([1; 16]).unwrap()
}

fn session_id() -> SessionId {
    SessionId::from_bytes([2; 16]).unwrap()
}

fn grid() -> FrequencyGrid {
    FrequencyGrid {
        start_hz: 2_400_000_000,
        stop_hz_exclusive: 2_408_000_000,
        bin_width_hz: 1_000_000,
        resolution_bandwidth_hz: 500_000,
        bin_count: 8,
    }
}

fn pose(sequence: u64) -> PoseReference {
    PoseReference {
        pose_id: PoseId::from_bytes([3; 16]).unwrap(),
        frame_id: FrameId::from_bytes([4; 16]).unwrap(),
        assignment_version: Text::new("manual-anchor/1").unwrap(),
        position: Point3 {
            x: CoordinateMeters::new(sequence as f64 / 10.0).unwrap(),
            y: CoordinateMeters::new(2.0).unwrap(),
            z: CoordinateMeters::new(1.2).unwrap(),
        },
        covariance: Evidence::Known(
            PositionCovariance::new([0.01, 0.0, 0.0, 0.01, 0.0, 0.02]).unwrap(),
        ),
        orientation: Evidence::Known(Orientation {
            yaw: Radians::new(0.0).unwrap(),
            pitch: Radians::new(0.0).unwrap(),
            roll: Radians::new(0.0).unwrap(),
        }),
        method_version: Text::new("pose-fixture/1").unwrap(),
    }
}

fn calibration() -> SpectrumCalibrationProfile {
    SpectrumCalibrationProfile {
        schema: CALIBRATION_SCHEMA_V1.to_owned(),
        calibration_id: CalibrationId::from_bytes([5; 16]).unwrap(),
        version: Text::new("profile-v1").unwrap(),
        profile_sha256: ContentHash::from_sha256([6; 32]),
        valid_start_hz: 2_300_000_000,
        valid_stop_hz_exclusive: 2_500_000_000,
        valid_from_utc: UtcTimestamp(0),
        valid_until_utc: UtcTimestamp(i64::MAX),
        reference_correction_milli_db: 1_250,
        antenna_factor_milli_db: 2_000,
        cable_loss_milli_db: 500,
        frequency_offset_correction_hz: -25,
        uncertainty_milli_db: 2_500,
        clipping_checked: Evidence::Known(true),
        dynamic_range_checked: Evidence::Known(true),
    }
}

fn bins(active: &[usize], clipped_bin: Option<usize>) -> Vec<SpectrumBin> {
    (0..8)
        .map(|index| {
            if active.contains(&index) {
                SpectrumBin::Observed {
                    power_milli_dbm: -50_000,
                    clipped: clipped_bin == Some(index),
                }
            } else {
                SpectrumBin::BelowDetectionThreshold {
                    threshold_milli_dbm: -80_000,
                }
            }
        })
        .collect()
}

fn sweep(sequence: u64, start_ns: u64, active: &[usize]) -> SpectrumSweep {
    sweep_with(sequence, start_ns, bins(active, None), true)
}

fn sweep_with(
    sequence: u64,
    start_ns: u64,
    bin_values: Vec<SpectrumBin>,
    calibrated: bool,
) -> SpectrumSweep {
    let epoch = ClockEpochId::from_bytes([7; 16]).unwrap();
    let end_ns = start_ns + 50_000_000;
    let window = MonotonicWindow::new(
        MonotonicTimestamp {
            epoch,
            nanoseconds: start_ns,
        },
        MonotonicTimestamp {
            epoch,
            nanoseconds: end_ns,
        },
    )
    .unwrap();
    let captured_at = start_ns + 10_000_000;
    let time = CaptureTime {
        wall: Evidence::Known(WallClockReading {
            time: UtcTimestamp(10_000_000_000 + captured_at as i64),
            source: Text::new("fixture-clock").unwrap(),
            precision: Seconds::new(0.001).unwrap(),
            uncertainty: Evidence::Known(Seconds::new(0.002).unwrap()),
        }),
        monotonic: Evidence::Known(MonotonicTimestamp {
            epoch,
            nanoseconds: captured_at,
        }),
        synchronization: Evidence::Unknown(UnknownReason::ClockUnavailable),
    };
    let document = SpectrumSweepDocument {
        schema: SWEEP_SCHEMA_V1.to_owned(),
        session_id: session_id(),
        sequence,
        source: SpectrumSourceMetadata {
            kind: SpectrumSourceKind::ImportedTrace,
            source_id: source_id(),
            sensor_id: Evidence::Known(SensorId::from_bytes([8; 16]).unwrap()),
            adapter_name: Text::new("synthetic trace reader").unwrap(),
            adapter_version: Text::new("fixture-adapter/1").unwrap(),
            device_model: Evidence::Known(Text::new("synthetic analyzer").unwrap()),
            device_firmware: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        },
        grid: grid(),
        acquisition: AcquisitionSettings {
            detector: DetectorKind::Average,
            window: WindowFunction::Hann,
            gain: GainSetting::Manual {
                gain_milli_db: 10_000,
            },
            dwell_nanoseconds: 20_000_000,
            sweep_nanoseconds: 50_000_000,
        },
        power_unit: PowerUnit::DbmPerBin,
        time,
        capture_window: Evidence::Known(window),
        pose: Evidence::Known(pose(sequence)),
        calibration: if calibrated {
            Evidence::Known(calibration())
        } else {
            Evidence::Unknown(UnknownReason::NotMeasured)
        },
        bins: bin_values,
    };
    SpectrumSweep::new(document, limits()).unwrap()
}

fn persistent_series(active_bins: &[usize]) -> Vec<SpectrumSweep> {
    (0..5)
        .map(|sequence| sweep(sequence, sequence * 250_000_000, active_bins))
        .collect()
}

#[test]
fn sweep_round_trip_is_canonical_and_binds_source_time_pose_and_calibration() {
    let sample = sweep(3, 750_000_000, &[2]);
    let decoded = SpectrumSweep::from_canonical_bytes(sample.canonical_bytes(), limits()).unwrap();
    assert_eq!(sample, decoded);
    assert_eq!(sample.sha256(), decoded.sha256());
    let encoded = std::str::from_utf8(sample.canonical_bytes()).unwrap();
    assert!(!encoded.contains("spectrum_device"));
    assert!(encoded.contains("imported_trace"));
    assert!(encoded.contains("position"));
    assert!(encoded.contains("monotonic"));
    assert!(encoded.contains("profile_sha256"));
}

#[test]
fn rejects_unknown_fields_unsupported_versions_and_noncanonical_encoding() {
    let sample = sweep(0, 0, &[1]);
    let mut value: serde_json::Value = serde_json::from_slice(sample.canonical_bytes()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        SpectrumSweep::from_canonical_bytes(&bytes, limits()),
        Err(SpectrumError::Serialization)
    ));

    let mut document = sample.document().clone();
    document.schema = "kyberia.spectrum-sweep/2".to_owned();
    assert!(matches!(
        SpectrumSweep::new(document, limits()),
        Err(SpectrumError::UnsupportedSchema)
    ));

    let pretty = serde_json::to_vec_pretty(sample.document()).unwrap();
    assert!(matches!(
        SpectrumSweep::from_canonical_bytes(&pretty, limits()),
        Err(SpectrumError::NonCanonical)
    ));
}

#[test]
fn rejects_malformed_grid_bin_shape_and_out_of_range_calibration() {
    let sample = sweep(0, 0, &[1]);
    let mut document = sample.document().clone();
    document.grid.bin_width_hz = 3;
    assert!(matches!(
        SpectrumSweep::new(document, limits()),
        Err(SpectrumError::Invalid("frequency bin geometry"))
    ));

    let mut document = sample.document().clone();
    document.bins.pop();
    assert!(matches!(
        SpectrumSweep::new(document, limits()),
        Err(SpectrumError::Invalid("sweep bin count"))
    ));

    let mut document = sample.document().clone();
    document.acquisition.sweep_nanoseconds = 40_000_000;
    assert!(matches!(
        SpectrumSweep::new(document, limits()),
        Err(SpectrumError::Invalid("capture duration mismatch"))
    ));

    let mut document = sample.document().clone();
    if let Evidence::Known(profile) = &mut document.calibration {
        profile.valid_stop_hz_exclusive = 2_407_000_000;
    }
    assert!(matches!(
        SpectrumSweep::new(document, limits()),
        Err(SpectrumError::Invalid("calibration profile"))
    ));
}

#[test]
fn zero_power_below_detection_and_not_observed_are_distinct_evidence() {
    let mut document = sweep(0, 0, &[1]).document().clone();
    document.bins[0] = SpectrumBin::Observed {
        power_milli_dbm: 0,
        clipped: false,
    };
    document.bins[1] = SpectrumBin::BelowDetectionThreshold {
        threshold_milli_dbm: -90_000,
    };
    document.bins[2] = SpectrumBin::NotObserved {
        reason: UnknownReason::UnsupportedCapability,
    };
    let validated = SpectrumSweep::new(document, limits()).unwrap();
    let decoded =
        SpectrumSweep::from_canonical_bytes(validated.canonical_bytes(), limits()).unwrap();
    assert_eq!(validated, decoded);
    assert!(matches!(
        &decoded.document().bins[0],
        SpectrumBin::Observed {
            power_milli_dbm: 0,
            ..
        }
    ));
    assert!(matches!(
        &decoded.document().bins[1],
        SpectrumBin::BelowDetectionThreshold { .. }
    ));
    assert!(matches!(
        &decoded.document().bins[2],
        SpectrumBin::NotObserved { .. }
    ));
}

#[test]
fn signatures_are_deterministic_pattern_evidence_not_emitter_labels() {
    let narrow = persistent_series(&[3]);
    let event = SpectrumEvent::from_sweeps(&narrow, -60_000, limits()).unwrap();
    assert_eq!(
        event.assessment().pattern,
        SignaturePattern::NarrowbandPersistentPattern
    );
    assert_eq!(
        event.assessment().reason,
        SignatureReason::PersistentNarrowbandEnergy
    );
    assert_eq!(event.assessment().confidence_parts_per_million, 1_000_000);

    let decoded =
        SpectrumEvent::from_canonical_bytes(event.canonical_bytes(), &narrow, -60_000, limits())
            .unwrap();
    assert_eq!(event, decoded);

    let wide = persistent_series(&[1, 2, 3, 4]);
    let wide_event = SpectrumEvent::from_sweeps(&wide, -60_000, limits()).unwrap();
    assert_eq!(
        wide_event.assessment().pattern,
        SignaturePattern::WidebandPersistentPattern
    );
    assert!(!wide_event.canonical_bytes().is_empty());
}

#[test]
fn shuffled_arrival_has_identical_event_identity_and_bytes() {
    let forward = persistent_series(&[2]);
    let mut reverse = forward.clone();
    reverse.reverse();
    let a = SpectrumEvent::from_sweeps(&forward, -60_000, limits()).unwrap();
    let b = SpectrumEvent::from_sweeps(&reverse, -60_000, limits()).unwrap();
    assert_eq!(a.identity(), b.identity());
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
}

#[test]
fn event_decode_rejects_fabricated_identity_and_derived_pattern() {
    let sweeps = persistent_series(&[2]);
    let event = SpectrumEvent::from_sweeps(&sweeps, -60_000, limits()).unwrap();
    let original = std::str::from_utf8(event.canonical_bytes()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(event.canonical_bytes()).unwrap();
    let old_identity = serde_json::to_string(&value["identity"]).unwrap();
    let old_pattern = serde_json::to_string(&value["assessment"]["pattern"]).unwrap();
    let identity_field = format!("\"identity\":{old_identity}");
    let forged_identity_field = format!("\"identity\":\"{}\"", "00".repeat(32));
    let forged = original
        .replacen(&identity_field, &forged_identity_field, 1)
        .into_bytes();
    assert!(matches!(
        SpectrumEvent::from_canonical_bytes(&forged, &sweeps, -60_000, limits()),
        Err(SpectrumError::EvidenceMismatch)
    ));

    let pattern_field = format!("\"pattern\":{old_pattern}");
    let forged_pattern_field = "\"pattern\":\"wideband_persistent_pattern\"";
    let forged = original
        .replacen(&pattern_field, forged_pattern_field, 1)
        .into_bytes();
    assert!(matches!(
        SpectrumEvent::from_canonical_bytes(&forged, &sweeps, -60_000, limits()),
        Err(SpectrumError::EvidenceMismatch)
    ));
}

#[test]
fn incomplete_hopping_or_uncalibrated_evidence_stays_unknown() {
    let hopping = (0..5)
        .map(|sequence| sweep(sequence, sequence * 250_000_000, &[sequence as usize]))
        .collect::<Vec<_>>();
    let event = SpectrumEvent::from_sweeps(&hopping, -60_000, limits()).unwrap();
    assert_eq!(event.assessment().pattern, SignaturePattern::Unknown);

    let uncalibrated = (0..5)
        .map(|sequence| sweep_with(sequence, sequence * 250_000_000, bins(&[2], None), false))
        .collect::<Vec<_>>();
    let event = SpectrumEvent::from_sweeps(&uncalibrated, -60_000, limits()).unwrap();
    assert_eq!(event.assessment().pattern, SignaturePattern::Unknown);
    assert_eq!(
        event.assessment().reason,
        SignatureReason::CalibrationUnavailable
    );
}

#[test]
fn sequence_gaps_excessive_cadence_and_clipping_cannot_claim_persistence() {
    let gap = vec![
        sweep(0, 0, &[2]),
        sweep(1, 250_000_000, &[2]),
        sweep(3, 500_000_000, &[2]),
        sweep(4, 750_000_000, &[2]),
        sweep(5, 1_000_000_000, &[2]),
    ];
    let event = SpectrumEvent::from_sweeps(&gap, -60_000, limits()).unwrap();
    assert_eq!(event.assessment().pattern, SignaturePattern::Unknown);
    assert_eq!(event.assessment().reason, SignatureReason::SequenceGap);

    let cadence = vec![
        sweep(0, 0, &[2]),
        sweep(1, 250_000_000, &[2]),
        sweep(2, 600_000_000, &[2]),
        sweep(3, 850_000_000, &[2]),
        sweep(4, 1_100_000_000, &[2]),
    ];
    let event = SpectrumEvent::from_sweeps(&cadence, -60_000, limits()).unwrap();
    assert_eq!(event.assessment().pattern, SignaturePattern::Unknown);
    assert_eq!(
        event.assessment().reason,
        SignatureReason::ExcessiveInterSweepGap
    );

    let clipped = (0..5)
        .map(|sequence| sweep_with(sequence, sequence * 250_000_000, bins(&[2], Some(2)), true))
        .collect::<Vec<_>>();
    let event = SpectrumEvent::from_sweeps(&clipped, -60_000, limits()).unwrap();
    assert_eq!(event.assessment().pattern, SignaturePattern::Unknown);
    assert_eq!(event.assessment().reason, SignatureReason::ClippedEvidence);
}

#[test]
fn explicit_unknown_position_and_time_are_retained_but_not_temporal_proof() {
    let mut document = sweep(0, 0, &[1]).document().clone();
    document.pose = Evidence::Unknown(UnknownReason::NotMeasured);
    document.capture_window = Evidence::Unknown(UnknownReason::ClockUnavailable);
    document.time.monotonic = Evidence::Unknown(UnknownReason::ClockUnavailable);
    document.calibration = Evidence::Unknown(UnknownReason::NotMeasured);
    let sample = SpectrumSweep::new(document, limits()).unwrap();
    let decoded = SpectrumSweep::from_canonical_bytes(sample.canonical_bytes(), limits()).unwrap();
    assert_eq!(sample, decoded);
    assert!(matches!(decoded.document().pose, Evidence::Unknown(_)));
    assert!(matches!(
        decoded.document().capture_window,
        Evidence::Unknown(_)
    ));
}

#[test]
fn limits_prevent_unbounded_sweep_and_signature_work() {
    let tiny = ProcessingLimits {
        max_bins_per_sweep: 7,
        ..limits()
    };
    let document = sweep(0, 0, &[1]).document().clone();
    assert!(matches!(
        SpectrumSweep::new(document, tiny),
        Err(SpectrumError::Invalid("frequency bin geometry"))
    ));

    let work_limited = ProcessingLimits {
        max_work_units: 39,
        ..limits()
    };
    assert!(matches!(
        SpectrumEvent::from_sweeps(&persistent_series(&[2]), -60_000, work_limited),
        Err(SpectrumError::ResourceLimit("signature work"))
    ));
}
