use kyberia_domain::{
    capability::{Capability, CapabilityDocument, CapabilityState, RawPayloadPolicy},
    evidence::{Evidence, SchemaVersion, UnknownReason},
    identity::{
        AdapterId, ClockEpochId, CollectorId, ContentHash, FloorId, FrameId, ObservationId,
        ProjectId, SessionId, SnapshotId, SourceId, Text,
    },
    observation::{
        CalibrationState, ObservationEnvelope, ObservationPayload, PrivacyState,
        RadioIdentityEvidence, ReceivedObservation, ScanObservation, SignalReading,
        SourceDescriptor, SourceKind, SourceResponseTiming,
    },
    spatial::{Point3, PoseReference},
    time::{CaptureTime, MonotonicTimestamp, MonotonicWindow},
    units::{CoordinateMeters, Dbm, Meters, Seconds},
};
use kyberia_observation_analysis::{RejectionReason, SelectionEvidencePlane, SelectionManifest};
use kyberia_project_store::{
    ArtifactEntry, ArtifactKind, Bundle, Cancellation, ObservationChunkProvenance, OpenMode,
};
use kyberia_spatial_analysis::{
    Config, Extrapolation, Grid, InputEvidencePlane, Method, MetricDefinition,
    MetricDefinitionBinding, Point2,
};
use kyberia_stored_analysis::{SnapshotInput, StoredAnalysisError, StoredRssiAnalysisRequest, run};
use kyberia_survey::{
    CaptureMode, PointConfig, PointConfigData, PointMetric, PointSurvey, PosePolicy, Target,
};
use sha2::Digest;
use std::{
    collections::BTreeMap,
    num::NonZeroU32,
    path::Path,
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
fn unknown<T>(reason: UnknownReason) -> Evidence<T> {
    Evidence::Unknown(reason)
}
fn retained_directory() -> std::path::PathBuf {
    tempfile::tempdir().unwrap().keep()
}
fn point_config() -> PointConfig {
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
            orientation: unknown(UnknownReason::NotObservable),
            method_version: text("manual/v1"),
        },
        map_calibration: unknown(UnknownReason::NotApplicable),
        source_id: SourceId::from_bytes(id(11)).unwrap(),
        collector_id,
        adapter_version: text("collector/1"),
        epoch: epoch(),
        capabilities: CapabilityDocument {
            schema_version: SchemaVersion::V1,
            collector_id,
            collector_version: text("collector/1"),
            probed_at: CaptureTime {
                wall: unknown(UnknownReason::ClockUnavailable),
                monotonic: Evidence::Known(stamp(0)),
                synchronization: unknown(UnknownReason::ClockUnavailable),
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
        minimum_active_time: Seconds::new(0.0).unwrap(),
        maximum_scan_age: Seconds::new(1.0).unwrap(),
        target: Target::AnyBssid,
        pose_policy: PosePolicy::RequireReported {
            maximum_offset: Meters::new(0.5).unwrap(),
            maximum_axis_stddev: Meters::new(0.5).unwrap(),
        },
        allow_synthetic: false,
        method_version: text("point/v1"),
    })
    .unwrap()
}
fn observation(id_byte: u8, captured: u64, rssi: f64) -> ObservationEnvelope {
    let config = point_config();
    let data = kyberia_domain::observation::EnvelopeData {
        schema_version: kyberia_domain::observation::ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes(id(id_byte)).unwrap(),
        session_id: config.data().session_id,
        source: SourceDescriptor {
            source_id: config.data().source_id,
            collector_id: config.data().collector_id,
            sensor_id: unknown(UnknownReason::NotApplicable),
            adapter_id: Evidence::Known(AdapterId::from_bytes(id(12)).unwrap()),
            kind: SourceKind::NativeApi,
            source_name: text("test-radio"),
            source_version: unknown(UnknownReason::SourceDidNotProvide),
            source_schema_version: text("test-wire/1"),
            adapter_name: text("kyberia-test"),
            adapter_version: config.data().adapter_version.clone(),
            parser_version: text("parser/1"),
            driver_version: unknown(UnknownReason::SourceDidNotProvide),
            os_version: unknown(UnknownReason::SourceDidNotProvide),
        },
        time: CaptureTime {
            wall: unknown(UnknownReason::NotMeasured),
            monotonic: Evidence::Known(stamp(captured)),
            synchronization: unknown(UnknownReason::ClockUnavailable),
        },
        pose: Evidence::Known(config.data().anchor.clone()),
        channel: unknown(UnknownReason::SourceDidNotProvide),
        dwell: unknown(UnknownReason::NotObservable),
        privacy: PrivacyState {
            policy_version: text("privacy/v1"),
            identifiers: kyberia_domain::observation::IdentifierPolicy::OwnedInfrastructure,
            payload: kyberia_domain::observation::PayloadRetention::Discarded,
        },
        quality: vec![],
        raw_source: unknown(UnknownReason::NotRetained),
        payload: ObservationPayload::Scan(ScanObservation {
            identity: RadioIdentityEvidence {
                physical_device: unknown(UnknownReason::NotAdvertised),
                radio: unknown(UnknownReason::NotAdvertised),
                bss: unknown(UnknownReason::NotAdvertised),
                bssid: Evidence::Known(kyberia_domain::identity::MacAddress([0, 1, 2, 3, 4, 5])),
                ess: unknown(UnknownReason::NotAdvertised),
                mld: unknown(UnknownReason::NotAdvertised),
                link_id: unknown(UnknownReason::NotApplicable),
                client: unknown(UnknownReason::NotApplicable),
                grouping_evidence: unknown(UnknownReason::NotAdvertised),
            },
            ssid: unknown(UnknownReason::NotAdvertised),
            signal: SignalReading {
                rssi_dbm: Evidence::Known(Dbm::new(rssi).unwrap()),
                noise_dbm: unknown(UnknownReason::NotObservable),
                chains: vec![],
                calibration: Evidence::Known(CalibrationState::Uncalibrated),
                measurement_method: text("test scan"),
            },
            information_elements: unknown(UnknownReason::NotRetained),
            result_age: Evidence::Known(Seconds::new(0.0).unwrap()),
        }),
    };
    ObservationEnvelope::new(data).unwrap()
}
fn response(returned: u64) -> SourceResponseTiming {
    SourceResponseTiming::new(
        CaptureTime {
            wall: unknown(UnknownReason::ClockUnavailable),
            monotonic: Evidence::Known(stamp(returned)),
            synchronization: unknown(UnknownReason::ClockUnavailable),
        },
        Evidence::Known(MonotonicWindow::new(stamp(returned - 40), stamp(returned - 20)).unwrap()),
    )
    .unwrap()
}
fn metric() -> MetricDefinitionBinding {
    let definition = MetricDefinition::signal_rssi().unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let artifact = kyberia_domain::analysis::VersionedArtifact {
        version: definition.version().clone(),
        sha256: ContentHash::from_sha256(sha2::Sha256::digest(&bytes).into()),
        byte_length: kyberia_domain::analysis::ExactU64::new(bytes.len() as u64),
        media_type: text(kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE),
    };
    definition.bind(artifact).unwrap()
}
fn grid() -> Grid {
    Grid {
        floor_id: floor_id(),
        frame_id: frame_id(),
        origin: Point2 {
            // The one cell is centered on the point anchor at (1, 2), making
            // the expected aggregate independently inspectable.
            x: CoordinateMeters::new(-1.5).unwrap(),
            y: CoordinateMeters::new(-0.5).unwrap(),
        },
        resolution: Meters::new(5.0).unwrap(),
        column_offset: 0,
        row_offset: 0,
        width: 1,
        height: 1,
    }
}
fn single_snapshot_request(
    revision: u64,
    observation_id: ObservationId,
    snapshot_id: SnapshotId,
) -> StoredRssiAnalysisRequest {
    let mut request = request(revision, vec![observation_id]);
    request.snapshots = vec![SnapshotInput {
        snapshot_id,
        floor_id: floor_id(),
    }];
    request
}
fn synthetic_observation(
    id_byte: u8,
    captured: u64,
    rssi: f64,
    synthetic_source: bool,
) -> ObservationEnvelope {
    let mut data = observation(id_byte, captured, rssi).into_data();
    if synthetic_source {
        data.source.kind = SourceKind::SyntheticFixture;
    }
    data.quality
        .push(kyberia_domain::observation::QualityFlag::SyntheticFixture);
    ObservationEnvelope::new(data).unwrap()
}
fn create_synthetic_fixture(path: &Path, synthetic_source: bool) -> (Bundle, ObservationId) {
    let mut bundle =
        Bundle::create(path, project_id(), "synthetic stored analysis".into(), 1).unwrap();
    let evidence = synthetic_observation(40, 150, -48.0, synthetic_source);
    let mut config = point_config().data().clone();
    config.allow_synthetic = true;
    let survey = PointSurvey::start(PointConfig::new(config).unwrap(), stamp(100))
        .unwrap()
        .admit(&evidence, stamp(200))
        .unwrap();
    bundle
        .save_survey_snapshot(SnapshotId::from_bytes(id(20)).unwrap(), &survey, 2)
        .unwrap();
    bundle
        .publish_observation_chunk(
            std::slice::from_ref(&evidence),
            ObservationChunkProvenance::new("test/synthetic-stored-analysis").unwrap(),
            3,
        )
        .unwrap();
    (bundle, evidence.data().id)
}
fn spatial_config() -> Config {
    Config {
        method: Method::PointValue,
        support_radius: Meters::new(5.0).unwrap(),
        minimum_locations: 1,
        maximum_neighbors: 4,
        extrapolation: Extrapolation::Disabled,
    }
}
fn request(revision: u64, observations: Vec<ObservationId>) -> StoredRssiAnalysisRequest {
    StoredRssiAnalysisRequest {
        project_id: project_id(),
        project_revision: revision,
        floor_id: floor_id(),
        frame_id: frame_id(),
        target_bssid: kyberia_domain::identity::MacAddress([0, 1, 2, 3, 4, 5]),
        observation_ids: observations,
        snapshots: vec![
            SnapshotInput {
                snapshot_id: SnapshotId::from_bytes(id(20)).unwrap(),
                floor_id: floor_id(),
            },
            SnapshotInput {
                snapshot_id: SnapshotId::from_bytes(id(21)).unwrap(),
                floor_id: floor_id(),
            },
        ],
        session_scope: None,
        source_scope: None,
        adapter_scope: None,
        allow_uncalibrated: true,
        metric: metric(),
        spatial_configuration: spatial_config(),
        grid: grid(),
    }
}
fn create_fixture(path: &Path) -> (Bundle, Vec<ObservationId>, String, String) {
    let mut bundle = Bundle::create(path, project_id(), "stored analysis".into(), 1).unwrap();
    let first = observation(30, 150, -55.0);
    let second = observation(31, 180, -65.0);
    let strict = PointSurvey::start(point_config(), stamp(100))
        .unwrap()
        .admit(&first, stamp(200))
        .unwrap();
    // The second observation is in the receipt survey; retain both evidence
    // planes in the same real bundle to exercise their distinct paths.
    let receipt = PointSurvey::start(point_config(), stamp(100))
        .unwrap()
        .associate_received(
            &ReceivedObservation::new(second.clone(), Evidence::Known(response(280))).unwrap(),
        )
        .unwrap()
        .0;
    bundle
        .save_survey_snapshot(SnapshotId::from_bytes(id(20)).unwrap(), &strict, 2)
        .unwrap();
    let second_record = bundle
        .save_survey_snapshot(SnapshotId::from_bytes(id(21)).unwrap(), &receipt, 3)
        .unwrap();
    let chunk = bundle
        .publish_observation_chunk(
            &[first, second],
            ObservationChunkProvenance::new("test/stored-analysis").unwrap(),
            4,
        )
        .unwrap();
    (
        bundle,
        vec![
            ObservationId::from_bytes(id(30)).unwrap(),
            ObservationId::from_bytes(id(31)).unwrap(),
        ],
        chunk.hash().to_owned(),
        second_record.artifact_hash,
    )
}

#[test]
fn real_bundle_receipt_and_strict_evidence_produce_deterministic_tile() {
    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, chunk_hash, snapshot_hash) = create_fixture(&path);
    let request = request(bundle.manifest().unwrap().revision, ids);
    let first = run(
        &bundle,
        request.clone(),
        &kyberia_project_store::NeverCancel,
    )
    .unwrap();
    let second = run(&bundle, request, &kyberia_project_store::NeverCancel).unwrap();
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.artifact(), second.artifact());
    assert_eq!(first.tile(), second.tile());
    assert_eq!(
        first.tile().cells[0].value,
        Evidence::Known(Dbm::new(-60.0).unwrap())
    );
    assert_eq!(
        first.tile().inputs.evidence_plane,
        InputEvidencePlane::Measured
    );
    let selection = SelectionManifest::from_canonical_bytes(first.selection_manifest()).unwrap();
    assert_eq!(selection.evidence_plane, SelectionEvidencePlane::Measured);
    assert_eq!(selection.selected.len(), 2);
    assert_eq!(first.document().source_chunk_hashes.len(), 1);
    assert_eq!(
        String::from(first.document().source_chunk_hashes[0]),
        chunk_hash
    );
    assert_eq!(
        String::from(first.document().snapshots[1].artifact_hash),
        snapshot_hash
    );
    assert_eq!(
        first.document().canonical_bytes().unwrap(),
        first.canonical_bytes()
    );
    let decoded = kyberia_stored_analysis::StoredRssiAnalysisDocument::from_canonical_bytes(
        first.canonical_bytes(),
    )
    .unwrap();
    assert_eq!(&decoded, first.document());
    let mut forged = first.document().clone();
    forged.source_chunk_hashes[0] = ContentHash::from_sha256([9; 32]);
    assert!(forged.canonical_bytes().is_err());
    assert_eq!(first.tile().cells.len(), 1);
}

#[test]
fn synthetic_evidence_is_rejected_after_real_bundle_roundtrip() {
    for (synthetic_source, expected_reason) in [
        (true, RejectionReason::UnsupportedPayload),
        (false, RejectionReason::UnusableQuality),
    ] {
        let directory = retained_directory();
        let path = directory.join("bundle");
        let (bundle, observation_id) = create_synthetic_fixture(&path, synthetic_source);
        let revision = bundle.manifest().unwrap().revision;
        let result = run(
            &bundle,
            single_snapshot_request(
                revision,
                observation_id,
                SnapshotId::from_bytes(id(20)).unwrap(),
            ),
            &kyberia_project_store::NeverCancel,
        )
        .unwrap();

        let selection = SelectionManifest::from_canonical_bytes(result.selection_manifest())
            .expect("stored selection remains canonical");
        assert!(selection.selected.is_empty());
        assert_eq!(selection.rejected.len(), 1);
        assert_eq!(selection.rejected[0].reason, expected_reason);
        assert_eq!(selection.evidence_plane, SelectionEvidencePlane::Measured);
        assert_eq!(
            result.tile().inputs.evidence_plane,
            InputEvidencePlane::Measured
        );
        assert_eq!(
            result.tile().cells[0].value,
            Evidence::Unknown(UnknownReason::NotMeasured)
        );
        assert!(result.tile().cells[0].contributors.is_empty());
    }
}

#[test]
fn mismatched_revision_floor_and_missing_ids_fail_before_output() {
    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, _, _) = create_fixture(&path);
    let revision = bundle.manifest().unwrap().revision;
    assert!(matches!(
        run(
            &bundle,
            request(revision - 1, ids.clone()),
            &kyberia_project_store::NeverCancel
        ),
        Err(StoredAnalysisError::InvalidRequest("project revision"))
    ));
    let mut wrong_project = request(revision, ids.clone());
    wrong_project.project_id = ProjectId::from_bytes(id(97)).unwrap();
    assert!(matches!(
        run(&bundle, wrong_project, &kyberia_project_store::NeverCancel),
        Err(StoredAnalysisError::InvalidRequest("project identity"))
    ));
    let mut wrong_floor = request(revision, ids.clone());
    wrong_floor.snapshots[0].floor_id = FloorId::from_bytes(id(99)).unwrap();
    assert!(matches!(
        run(&bundle, wrong_floor, &kyberia_project_store::NeverCancel),
        Err(StoredAnalysisError::InvalidRequest(
            "snapshot floor binding"
        ))
    ));
    let mut missing = ids;
    missing[0] = ObservationId::from_bytes(id(98)).unwrap();
    assert!(matches!(
        run(&bundle, request(revision, missing), &kyberia_project_store::NeverCancel),
        Err(StoredAnalysisError::Store(kyberia_project_store::StoreError::Invalid(message)))
            if message.contains("missing requested IDs")
    ));
}

#[test]
fn revision_change_during_query_is_rejected() {
    struct RevisionBump {
        calls: AtomicUsize,
        bump_at: usize,
        path: std::path::PathBuf,
    }
    impl Cancellation for RevisionBump {
        fn is_cancelled(&self) -> bool {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == self.bump_at {
                let mut writer = Bundle::open(&self.path, OpenMode::ReadWrite).unwrap();
                writer
                    .put_artifact(
                        b"revision bump",
                        ArtifactEntry {
                            kind: ArtifactKind::Annotation,
                            bytes: 13,
                            media_type: "application/test".into(),
                            provenance_id: "test/revision-bump".into(),
                        },
                        5,
                    )
                    .unwrap();
            }
            false
        }
    }
    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, _, _) = create_fixture(&path);
    let revision = bundle.manifest().unwrap().revision;
    // With two snapshots, the sixteenth callback is the query's initial
    // cancellation checkpoint. The second writer advances the manifest after
    // snapshot replay but before the indexed query receipt is accepted.
    let cancel = RevisionBump {
        calls: AtomicUsize::new(0),
        bump_at: 16,
        path,
    };
    assert!(matches!(
        run(&bundle, request(revision, ids), &cancel),
        Err(StoredAnalysisError::InvalidRequest(
            "query project revision"
        ))
    ));
}

#[test]
fn tampered_chunk_or_snapshot_is_rejected_and_cancellation_returns_no_result() {
    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, chunk_hash, _) = create_fixture(&path);
    let revision = bundle.manifest().unwrap().revision;
    std::fs::write(path.join("artifacts").join(&chunk_hash), b"tampered").unwrap();
    assert!(matches!(
        run(&bundle, request(revision, ids.clone()), &kyberia_project_store::NeverCancel),
        Err(StoredAnalysisError::Store(kyberia_project_store::StoreError::Corrupt(message)))
            if message.contains("checksum") || message.contains("Parquet")
    ));

    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, _, snapshot_hash) = create_fixture(&path);
    let revision = bundle.manifest().unwrap().revision;
    std::fs::write(path.join("artifacts").join(&snapshot_hash), b"tampered").unwrap();
    assert!(
        run(
            &bundle,
            request(revision, ids),
            &kyberia_project_store::NeverCancel
        )
        .is_err()
    );

    struct Cancel;
    impl Cancellation for Cancel {
        fn is_cancelled(&self) -> bool {
            true
        }
    }
    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, _, _) = create_fixture(&path);
    let revision = bundle.manifest().unwrap().revision;
    assert!(matches!(
        run(&bundle, request(revision, ids), &Cancel),
        Err(StoredAnalysisError::Cancelled)
    ));

    struct CancelAfter {
        calls: AtomicUsize,
        limit: usize,
    }
    impl Cancellation for CancelAfter {
        fn is_cancelled(&self) -> bool {
            self.calls.fetch_add(1, Ordering::SeqCst) + 1 >= self.limit
        }
    }
    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, _, _) = create_fixture(&path);
    let revision = bundle.manifest().unwrap().revision;
    let cancel_after_snapshot_decode = CancelAfter {
        calls: AtomicUsize::new(0),
        // The sixth check is in the cancellation-aware snapshot loader after
        // its bounded read and decoder, before it can publish a survey.
        limit: 6,
    };
    assert!(matches!(
        run(
            &bundle,
            request(revision, ids),
            &cancel_after_snapshot_decode,
        ),
        Err(StoredAnalysisError::Cancelled)
    ));

    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, _, _, _) = create_fixture(&path);
    assert!(matches!(
        bundle.load_survey_snapshot_with_cancel(
            SnapshotId::from_bytes(id(20)).unwrap(),
            None,
            &Cancel,
        ),
        Err(kyberia_project_store::StoreError::Cancelled)
    ));
    let cancel_after_decode = CancelAfter {
        calls: AtomicUsize::new(0),
        // Direct public-API coverage: entry, pre-read and post-read checks
        // pass; the post-decoder checkpoint cancels before commit/return.
        limit: 4,
    };
    assert!(matches!(
        bundle.load_survey_snapshot_with_cancel(
            SnapshotId::from_bytes(id(20)).unwrap(),
            None,
            &cancel_after_decode,
        ),
        Err(kyberia_project_store::StoreError::Cancelled)
    ));

    let directory = retained_directory();
    let path = directory.join("bundle");
    let (bundle, ids, _, _) = create_fixture(&path);
    let revision = bundle.manifest().unwrap().revision;
    let cancel_before_publication = CancelAfter {
        calls: AtomicUsize::new(0),
        // The final checkpoints occur after canonical output encoding and
        // after the last manifest read; the result must still be discarded.
        limit: 32,
    };
    assert!(matches!(
        run(&bundle, request(revision, ids), &cancel_before_publication,),
        Err(StoredAnalysisError::Cancelled)
    ));
}
