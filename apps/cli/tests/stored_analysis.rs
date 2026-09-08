use kyberia_domain::{
    capability::{Capability, CapabilityDocument, CapabilityState, RawPayloadPolicy},
    evidence::{Evidence, SchemaVersion, UnknownReason},
    identity::{
        AdapterId, ClockEpochId, CollectorId, ContentHash, FloorId, FrameId, MacAddress,
        ObservationId, ProjectId, SessionId, SnapshotId, SourceId, Text,
    },
    observation::{
        CalibrationState, EnvelopeData, IdentifierPolicy, ObservationEnvelope, ObservationPayload,
        ObservationSchemaVersion, PayloadRetention, PrivacyState, RadioIdentityEvidence,
        ScanObservation, SignalReading, SourceDescriptor, SourceKind,
    },
    spatial::{Point3, PoseReference},
    time::{CaptureTime, MonotonicTimestamp},
    units::{CoordinateMeters, Meters, Seconds},
};
use kyberia_project_store::{Bundle, ObservationChunkProvenance};
use kyberia_survey::{
    CaptureMode, PointConfig, PointConfigData, PointMetric, PointSurvey, PosePolicy, Target,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn id(byte: u8) -> [u8; 16] {
    [byte; 16]
}

fn project_id() -> ProjectId {
    ProjectId::from_bytes(id(1)).unwrap()
}

fn floor_id() -> FloorId {
    FloorId::from_bytes(id(2)).unwrap()
}

fn frame_id() -> FrameId {
    FrameId::from_bytes(id(3)).unwrap()
}

fn epoch() -> ClockEpochId {
    ClockEpochId::from_bytes(id(4)).unwrap()
}

fn stamp(nanoseconds: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds,
    }
}

fn text(value: &str) -> Text {
    Text::new(value).unwrap()
}

fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}

fn retained_directory() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(".trash")
        .join("test-runs");
    fs::create_dir_all(&root).unwrap();
    let process = std::process::id();
    loop {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let candidate = root.join(format!("stored-rssi-cli-{process}-{ordinal}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot retain test directory {candidate:?}: {error}"),
        }
    }
}

fn point_config(allow_synthetic: bool) -> PointConfig {
    let collector_id = CollectorId::from_bytes(id(7)).unwrap();
    PointConfig::new(PointConfigData {
        schema_version: SchemaVersion::V1,
        point_id: kyberia_survey::PointId::from_bytes(id(8)).unwrap(),
        session_id: SessionId::from_bytes(id(9)).unwrap(),
        anchor: PoseReference {
            pose_id: kyberia_domain::identity::PoseId::from_bytes(id(10)).unwrap(),
            frame_id: frame_id(),
            assignment_version: text("anchor/v1"),
            position: Point3 {
                x: CoordinateMeters::new(1.0).unwrap(),
                y: CoordinateMeters::new(2.0).unwrap(),
                z: CoordinateMeters::new(0.0).unwrap(),
            },
            covariance: Evidence::Known(
                kyberia_domain::spatial::PositionCovariance::new([0.01, 0.0, 0.0, 0.01, 0.0, 0.01])
                    .unwrap(),
            ),
            orientation: unknown(),
            method_version: text("manual/v1"),
        },
        map_calibration: unknown(),
        source_id: SourceId::from_bytes(id(11)).unwrap(),
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
        metrics: BTreeMap::from([(PointMetric::Rssi, std::num::NonZeroU32::new(1).unwrap())]),
        channels: vec![],
        minimum_active_time: Seconds::new(0.0).unwrap(),
        maximum_scan_age: Seconds::new(1.0).unwrap(),
        target: Target::AnyBssid,
        pose_policy: PosePolicy::RequireReported {
            maximum_offset: Meters::new(0.5).unwrap(),
            maximum_axis_stddev: Meters::new(0.5).unwrap(),
        },
        allow_synthetic,
        method_version: text("point/v1"),
    })
    .unwrap()
}

fn observation(id_byte: u8, synthetic: bool) -> ObservationEnvelope {
    let config = point_config(synthetic);
    let mut data = EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes(id(id_byte)).unwrap(),
        session_id: config.data().session_id,
        source: SourceDescriptor {
            source_id: config.data().source_id,
            collector_id: config.data().collector_id,
            sensor_id: unknown(),
            adapter_id: Evidence::Known(AdapterId::from_bytes(id(12)).unwrap()),
            kind: if synthetic {
                SourceKind::SyntheticFixture
            } else {
                SourceKind::NativeApi
            },
            source_name: text("test-radio"),
            source_version: unknown(),
            source_schema_version: text("test-wire/1"),
            adapter_name: text("kyberia-test"),
            adapter_version: config.data().adapter_version.clone(),
            parser_version: text("parser/1"),
            driver_version: unknown(),
            os_version: unknown(),
        },
        time: CaptureTime {
            wall: unknown(),
            monotonic: Evidence::Known(stamp(150)),
            synchronization: unknown(),
        },
        pose: Evidence::Known(config.data().anchor.clone()),
        channel: unknown(),
        dwell: unknown(),
        privacy: PrivacyState {
            policy_version: text("privacy/v1"),
            identifiers: IdentifierPolicy::OwnedInfrastructure,
            payload: PayloadRetention::Discarded,
        },
        quality: vec![],
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
                rssi_dbm: Evidence::Known(kyberia_domain::units::Dbm::new(-55.0).unwrap()),
                noise_dbm: unknown(),
                chains: vec![],
                calibration: Evidence::Known(CalibrationState::Uncalibrated),
                measurement_method: text("test scan"),
            },
            information_elements: unknown(),
            result_age: Evidence::Known(Seconds::new(0.0).unwrap()),
        }),
    };
    if synthetic {
        data.quality
            .push(kyberia_domain::observation::QualityFlag::SyntheticFixture);
    }
    ObservationEnvelope::new(data).unwrap()
}

fn create_fixture(
    path: &Path,
    synthetic: bool,
) -> (ProjectId, FloorId, FrameId, ObservationId, SnapshotId, u64) {
    let mut bundle = Bundle::create(path, project_id(), "CLI stored RSSI".into(), 1).unwrap();
    let evidence = observation(30, synthetic);
    let survey = PointSurvey::start(point_config(synthetic), stamp(100))
        .unwrap()
        .admit(&evidence, stamp(200))
        .unwrap();
    let snapshot_id = SnapshotId::from_bytes(id(20)).unwrap();
    bundle
        .save_survey_snapshot(snapshot_id, &survey, 2)
        .unwrap();
    bundle
        .publish_observation_chunk(
            std::slice::from_ref(&evidence),
            ObservationChunkProvenance::new("test/cli-stored-rssi").unwrap(),
            3,
        )
        .unwrap();
    let revision = bundle.manifest().unwrap().revision;
    drop(bundle);
    (
        project_id(),
        floor_id(),
        frame_id(),
        evidence.data().id,
        snapshot_id,
        revision,
    )
}

fn request_json(
    project: ProjectId,
    floor: FloorId,
    frame: FrameId,
    observation: ObservationId,
    snapshot: SnapshotId,
    revision: u64,
    origin_x_m: f64,
) -> serde_json::Value {
    serde_json::json!({
        "schema": "kyberia.stored-rssi-analysis-request/1",
        "project_id": serde_json::to_value(project).unwrap(),
        "project_revision": revision.to_string(),
        "floor_id": serde_json::to_value(floor).unwrap(),
        "frame_id": serde_json::to_value(frame).unwrap(),
        "target_bssid": [0, 1, 2, 3, 4, 5],
        "observation_ids": [serde_json::to_value(observation).unwrap()],
        "snapshots": [{
            "snapshot_id": serde_json::to_value(snapshot).unwrap(),
            "floor_id": serde_json::to_value(floor).unwrap(),
        }],
        "session_scope": null,
        "source_scope": null,
        "adapter_scope": null,
        "allow_uncalibrated": true,
        "method": {"kind": "point_value"},
        "support_radius_m": 5.0,
        "minimum_locations": 1,
        "maximum_neighbors": 4,
        "extrapolation": {"kind": "disabled"},
        "grid": {
            "origin_x_m": origin_x_m,
            "origin_y_m": -0.5,
            "resolution_m": 5.0,
            "column_offset": 0,
            "row_offset": 0,
            "width": 1,
            "height": 1,
        },
    })
}

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn run_cli(project: &Path, request: &Path, destination: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kyberia"))
        .args([
            "analyze-stored-rssi",
            project.to_str().unwrap(),
            request.to_str().unwrap(),
            destination.to_str().unwrap(),
        ])
        .output()
        .unwrap()
}

#[test]
fn real_bundle_reports_known_and_unknown_cells_with_canonical_hash() {
    let root = retained_directory();
    let project_path = root.join("bundle.rfatlas");
    let (project, floor, frame, observation, snapshot, revision) =
        create_fixture(&project_path, false);
    let request_path = root.join("request.json");
    write_json(
        &request_path,
        &request_json(project, floor, frame, observation, snapshot, revision, -1.5),
    );
    let destination = root.join("known-output");
    let output = run_cli(&project_path, &request_path, &destination);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "kyberia.stored-rssi-analysis-cli/1");
    assert_eq!(report["selected_observations"], 1);
    assert_eq!(report["rejected_observations"], 0);
    assert_eq!(report["cells"]["known"], 1);
    assert_eq!(report["cells"]["unknown"], 0);
    assert!(report.get("observation_ids").is_none());
    let canonical = fs::read(destination.join("analysis.json")).unwrap();
    assert_eq!(report["artifact"]["byte_length"], canonical.len() as u64);
    assert_eq!(
        report["artifact"]["sha256"],
        serde_json::to_value(ContentHash::from_sha256(Sha256::digest(&canonical).into())).unwrap()
    );
    kyberia_stored_analysis::StoredRssiAnalysisDocument::from_canonical_bytes(&canonical).unwrap();
    assert_eq!(
        fs::read(destination.join(".analysis.json.pending")).unwrap(),
        canonical
    );

    let gap_request_path = root.join("gap-request.json");
    write_json(
        &gap_request_path,
        &request_json(
            project,
            floor,
            frame,
            observation,
            snapshot,
            revision,
            1_000.0,
        ),
    );
    let gap_output = root.join("gap-output");
    let gap = run_cli(&project_path, &gap_request_path, &gap_output);
    assert!(gap.status.success());
    let gap_report: serde_json::Value = serde_json::from_slice(&gap.stdout).unwrap();
    assert_eq!(gap_report["cells"]["known"], 0);
    assert_eq!(gap_report["cells"]["unknown"], 1);
}

#[test]
fn malformed_oversize_mismatch_and_existing_output_fail_before_publication() {
    let root = retained_directory();
    let project_path = root.join("bundle.rfatlas");
    let (project, floor, frame, observation, snapshot, revision) =
        create_fixture(&project_path, false);
    let request_path = root.join("request.json");
    let valid = request_json(project, floor, frame, observation, snapshot, revision, -1.5);
    write_json(&request_path, &valid);

    let malformed = root.join("malformed.json");
    fs::write(&malformed, b"{").unwrap();
    let malformed_output = root.join("malformed-output");
    assert_eq!(
        run_cli(&project_path, &malformed, &malformed_output)
            .status
            .code(),
        Some(2)
    );
    assert!(!malformed_output.exists());

    let oversize = root.join("oversize.json");
    fs::write(&oversize, vec![b' '; 1_048_577]).unwrap();
    let oversize_output = root.join("oversize-output");
    assert_eq!(
        run_cli(&project_path, &oversize, &oversize_output)
            .status
            .code(),
        Some(2)
    );
    assert!(!oversize_output.exists());

    let mut wrong_revision = valid.clone();
    wrong_revision["project_revision"] = serde_json::json!("999");
    write_json(&request_path, &wrong_revision);
    let mismatch_output = root.join("mismatch-output");
    assert_eq!(
        run_cli(&project_path, &request_path, &mismatch_output)
            .status
            .code(),
        Some(2)
    );
    assert!(!mismatch_output.exists());

    write_json(&request_path, &valid);
    let existing = root.join("existing-output");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("sentinel"), b"keep me").unwrap();
    assert_eq!(
        run_cli(&project_path, &request_path, &existing)
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(existing.join("sentinel")).unwrap(), b"keep me");
    assert!(!existing.join("analysis.json").exists());
}

#[test]
fn synthetic_evidence_is_retained_as_unknown_without_false_measurement() {
    let root = retained_directory();
    let project_path = root.join("synthetic.rfatlas");
    let (project, floor, frame, observation, snapshot, revision) =
        create_fixture(&project_path, true);
    let request_path = root.join("request.json");
    write_json(
        &request_path,
        &request_json(project, floor, frame, observation, snapshot, revision, -1.5),
    );
    let destination = root.join("output");
    let output = run_cli(&project_path, &request_path, &destination);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["selected_observations"], 0);
    assert_eq!(report["rejected_observations"], 1);
    assert_eq!(report["cells"]["known"], 0);
    assert_eq!(report["cells"]["unknown"], 1);
}

#[cfg(unix)]
#[test]
fn fifo_request_is_rejected_without_blocking_or_creating_output() {
    let root = retained_directory();
    let project_path = root.join("bundle.rfatlas");
    let (project, floor, frame, observation, snapshot, revision) =
        create_fixture(&project_path, false);
    let _ = (project, floor, frame, observation, snapshot, revision);
    let fifo = root.join("request.fifo");
    assert!(
        Command::new("mkfifo")
            .args(["-m", "600", fifo.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let destination = root.join("fifo-output");
    let output = run_cli(&project_path, &fifo, &destination);
    assert_eq!(output.status.code(), Some(2));
    assert!(!destination.exists());
}
