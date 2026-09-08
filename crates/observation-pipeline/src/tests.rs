use super::*;
use kyberia_capture_adapter::macos::{
    MappingContext, ObservationMapping, SourceMapping, decode, normalize,
};
use kyberia_domain::{
    capability::{Capability, CapabilityDocument, CapabilityState, RawPayloadPolicy},
    evidence::{Evidence, SchemaVersion, UnknownReason},
    identity::{
        ClockEpochId, CollectorId, FrameId, ObservationId, ProjectId, SessionId, SnapshotId,
        SourceId, Text,
    },
    observation::{ObservationEnvelope, PayloadRetention, ReceivedObservation},
    spatial::{Point3, PoseReference, PositionCovariance},
    time::{CaptureTime, MonotonicTimestamp, UtcTimestamp},
    units::{CoordinateMeters, Meters, Seconds},
};
use kyberia_project_store::{
    Bundle, CaptureManifestRegistration, CapturePublicationStatus, ObservationChunkProvenance,
    OpenMode, StoreError,
};
use kyberia_survey::{
    CaptureMode, PointConfig, PointConfigData, PointId, PointMetric, PointSurvey, PosePolicy,
    Target,
};
use std::{cell::Cell, collections::BTreeMap, num::NonZeroU32, path::Path, rc::Rc};

const VALID: &[u8] = include_bytes!("../../../collectors/macos/fixtures/valid.ndjson");

fn text(value: &str) -> Text {
    Text::new(value).unwrap()
}

fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}

fn epoch() -> ClockEpochId {
    ClockEpochId::from_bytes([3; 16]).unwrap()
}

fn stamp(nanoseconds: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds,
    }
}

fn anchor() -> PoseReference {
    PoseReference {
        pose_id: kyberia_domain::identity::PoseId::from_bytes([13; 16]).unwrap(),
        frame_id: FrameId::from_bytes([14; 16]).unwrap(),
        assignment_version: text("manual-anchor/v1"),
        position: Point3 {
            x: CoordinateMeters::new(1.).unwrap(),
            y: CoordinateMeters::new(2.).unwrap(),
            z: CoordinateMeters::new(0.).unwrap(),
        },
        covariance: Evidence::Known(
            PositionCovariance::new([0.01, 0., 0., 0.01, 0., 0.01]).unwrap(),
        ),
        orientation: unknown(),
        method_version: text("manual/v1"),
    }
}

fn config(allow_synthetic: bool) -> PointConfig {
    let collector_id = CollectorId::from_bytes([2; 16]).unwrap();
    PointConfig::new(PointConfigData {
        schema_version: SchemaVersion::V1,
        point_id: PointId::from_bytes([12; 16]).unwrap(),
        session_id: SessionId::from_bytes([1; 16]).unwrap(),
        anchor: anchor(),
        map_calibration: unknown(),
        source_id: SourceId::from_bytes([4; 16]).unwrap(),
        collector_id,
        adapter_version: text("0.1.0"),
        epoch: epoch(),
        capabilities: CapabilityDocument {
            schema_version: SchemaVersion::V1,
            collector_id,
            collector_version: text("0.1.0"),
            probed_at: CaptureTime {
                wall: unknown(),
                monotonic: Evidence::Known(stamp(2_000)),
                synchronization: unknown(),
            },
            entries: BTreeMap::from([(
                Capability::NearbyScan,
                CapabilityState::Available {
                    evidence: text("normalized native fixture"),
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
        allow_synthetic,
        method_version: text("point/v1"),
    })
    .unwrap()
}

fn normalized_capture(bytes: &[u8], redacted: bool) -> NormalizedCapture {
    let stream = decode(bytes).unwrap();
    let context = MappingContext {
        expected_process_session: stream.process_session().to_owned(),
        session_id: SessionId::from_bytes([1; 16]).unwrap(),
        collector_id: CollectorId::from_bytes([2; 16]).unwrap(),
        clock_epoch: epoch(),
        sources: stream
            .source_keys()
            .map(|key| {
                (
                    key.to_owned(),
                    SourceMapping {
                        source_id: SourceId::from_bytes([4; 16]).unwrap(),
                        sensor_id: unknown(),
                        adapter_id: unknown(),
                    },
                )
            })
            .collect(),
        observations: stream
            .observation_keys()
            .map(|key| {
                (
                    key.to_owned(),
                    ObservationMapping {
                        observation_id: ObservationId::from_bytes([8; 16]).unwrap(),
                        transmitter_radio: unknown(),
                        transmitter_bss: unknown(),
                        identity_evidence: unknown(),
                    },
                )
            })
            .collect(),
        privacy: kyberia_domain::observation::PrivacyState {
            policy_version: text("test-policy/v1"),
            identifiers: if redacted {
                kyberia_domain::observation::IdentifierPolicy::Redacted
            } else {
                kyberia_domain::observation::IdentifierPolicy::ExplicitResearchConsent
            },
            payload: PayloadRetention::Discarded,
        },
    };
    normalize(&stream, &context).unwrap()
}

fn batch() -> ReceivedObservationBatch {
    ReceivedObservationBatch::from_normalized_capture(normalized_capture(VALID, false)).unwrap()
}

fn request(snapshot_byte: u8) -> PipelineRequest {
    PipelineRequest::new(
        SnapshotId::from_bytes([snapshot_byte; 16]).unwrap(),
        text("native-macos-capture/v1"),
        2,
    )
    .unwrap()
}

fn project(path: &Path) -> Bundle {
    Bundle::create(
        path,
        ProjectId::from_bytes([50; 16]).unwrap(),
        "native observation pipeline".to_owned(),
        1,
    )
    .unwrap()
}

#[derive(Default)]
struct FakePort {
    publication: Option<PublicationProgress>,
    fail_after_chunk: bool,
    wrong_count: bool,
    wrong_snapshot: bool,
    cancel_after_chunk: Option<Rc<Cell<bool>>>,
}

impl CapturePersistencePort for FakePort {
    fn persist_capture(
        &mut self,
        request: CapturePersistenceRequest<'_>,
    ) -> Result<PublicationProgress, PortError> {
        let CapturePersistenceRequest {
            manifest,
            raw_records,
            observations,
            snapshot_id,
            cancel,
            ..
        } = request;
        if cancel.is_cancelled() {
            return Err(PortError::new(PortErrorKind::Cancelled, "cancelled"));
        }
        let manifest_receipt = CaptureManifestReceipt {
            hash: ContentHash::from_sha256(
                Sha256::digest(manifest.canonical_bytes().unwrap()).into(),
            ),
            observation_count: observations.len() as u64,
            raw_record_count: raw_records.len() as u64,
            project_revision: 1,
        };
        let mut progress = PublicationProgress {
            manifest: manifest_receipt,
            raw_record_count: raw_records.len() as u64,
            chunk: None,
            snapshot: None,
        };
        if !observations.is_empty() {
            progress.chunk = Some(
                ObservationChunkReceipt::new(
                    ContentHash::try_from("11".repeat(32)).unwrap(),
                    if self.wrong_count {
                        observations.len() as u64 + 1
                    } else {
                        observations.len() as u64
                    },
                    2,
                )
                .map_err(|error| *error)?,
            );
            if let Some(flag) = &self.cancel_after_chunk {
                flag.set(true);
            }
            if self.fail_after_chunk {
                return Err(
                    PortError::new(PortErrorKind::Io, "test failure after chunk")
                        .with_progress(progress),
                );
            }
        }
        if cancel.is_cancelled() {
            return Err(
                PortError::new(PortErrorKind::Cancelled, "cancelled after chunk")
                    .with_progress(progress),
            );
        }
        progress.snapshot = Some(SnapshotReceipt {
            snapshot_id: if self.wrong_snapshot {
                SnapshotId::from_bytes([99; 16]).unwrap()
            } else {
                snapshot_id
            },
            project_revision: 3,
        });
        self.publication = Some(progress.clone());
        Ok(progress)
    }
}

struct CancelAfter {
    calls: Cell<usize>,
    threshold: usize,
}

impl CancelAfter {
    const fn new(threshold: usize) -> Self {
        Self {
            calls: Cell::new(0),
            threshold,
        }
    }
}

impl Cancellation for CancelAfter {
    fn is_cancelled(&self) -> bool {
        let calls = self.calls.get() + 1;
        self.calls.set(calls);
        calls >= self.threshold
    }
}

fn retained_batch() -> ReceivedObservationBatch {
    let mut capture = normalized_capture(VALID, false);
    let (envelope, response) = capture.observations[0].clone().into_parts();
    let mut data = envelope.into_data();
    data.privacy.payload = PayloadRetention::Retained {
        authorization_reference: text("test-consent/v1"),
        retention_deadline: UtcTimestamp(2_000_000),
    };
    capture.observations[0] =
        ReceivedObservation::new(ObservationEnvelope::new(data).unwrap(), response).unwrap();
    ReceivedObservationBatch::from_normalized_capture(capture).unwrap()
}

fn two_observation_batch() -> ReceivedObservationBatch {
    let mut capture = normalized_capture(VALID, false);
    let (envelope, response) = capture.observations[0].clone().into_parts();
    let mut data = envelope.into_data();
    data.id = ObservationId::from_bytes([9; 16]).unwrap();
    capture
        .observations
        .push(ReceivedObservation::new(ObservationEnvelope::new(data).unwrap(), response).unwrap());
    capture.completion.observation_count = 2;
    ReceivedObservationBatch::from_normalized_capture(capture).unwrap()
}

fn stage_manifest_and_chunk(envelope: ObservationEnvelope) -> (tempfile::TempDir, Bundle, String) {
    let batch = batch();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let manifest = bundle
        .persist_capture_manifest(
            CaptureManifestRegistration::new(
                batch.manifest().clone(),
                text("native-macos-capture/v1"),
                2,
            )
            .unwrap(),
        )
        .unwrap();
    let descriptor = bundle
        .publish_observation_chunk(
            &[envelope],
            ObservationChunkProvenance::new("native-macos-capture/v1").unwrap(),
            3,
        )
        .unwrap();
    bundle
        .link_capture_chunk(
            &manifest.manifest_hash,
            descriptor.hash(),
            descriptor.row_count(),
        )
        .unwrap();
    (directory, bundle, manifest.manifest_hash)
}

#[test]
fn normalized_native_capture_associates_and_reopens_with_manifest_and_link() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(60), &NeverCancel).unwrap();

    assert_eq!(outcome.association_count, 1);
    assert_eq!(outcome.survey.associations().len(), 1);
    assert_eq!(outcome.publication.manifest().observation_count(), 1);
    assert_eq!(outcome.publication.manifest().raw_record_count(), 0);
    let association = &outcome.survey.associations()[0];
    assert!(matches!(
        association.time_basis(),
        kyberia_survey::PointAssociationTimeBasis::Receipt { .. }
    ));
    let envelope = batch.observations()[0].envelope().data();
    assert!(matches!(envelope.time.monotonic, Evidence::Unknown(_)));
    assert!(matches!(envelope.dwell, Evidence::Unknown(_)));
    assert_eq!(outcome.survey.progress().metrics[&PointMetric::Rssi], 0);

    let chunk = outcome.publication.chunk().unwrap();
    assert_eq!(chunk.row_count(), 1);
    let stored = bundle
        .read_observation_chunk(&String::from(chunk.hash()))
        .unwrap();
    assert_eq!(stored, vec![batch.observations()[0].envelope().clone()]);
    let manifest_hash = String::from(outcome.publication.manifest().hash());
    let capture = bundle.capture_publication(&manifest_hash).unwrap().unwrap();
    assert_eq!(capture.status, CapturePublicationStatus::Complete);
    assert_eq!(
        capture.chunk_hash.as_deref(),
        Some(String::from(chunk.hash()).as_str())
    );
    assert_eq!(
        capture.snapshot_id,
        Some(outcome.publication.snapshot().unwrap().snapshot_id())
    );
    let manifest_bytes = bundle.read_artifact(&manifest_hash).unwrap();
    let decoded_manifest: CaptureManifest = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(decoded_manifest, *batch.manifest());
    drop(bundle);

    let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(reopened.read_observations().unwrap().len(), 1);
    assert_eq!(
        reopened
            .load_survey_snapshot(outcome.publication.snapshot().unwrap().snapshot_id())
            .unwrap()
            .survey,
        outcome.survey
    );
    assert!(reopened.verify().unwrap().failures.is_empty());
}

#[test]
fn source_order_is_kept_for_association_while_chunk_rows_are_canonical() {
    let batch = two_observation_batch();
    assert_eq!(
        batch.manifest().observation_ids_in_source_order(),
        &batch
            .observations()
            .iter()
            .map(|observation| observation.envelope().data().id)
            .collect::<Vec<_>>()
    );
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(59), &NeverCancel).unwrap();
    let stored = bundle
        .read_observation_chunk(&String::from(outcome.publication.chunk().unwrap().hash()))
        .unwrap();
    assert_eq!(
        stored
            .iter()
            .map(|observation| observation.data().id)
            .collect::<Vec<_>>(),
        {
            let mut ids = batch
                .observations()
                .iter()
                .map(|observation| observation.envelope().data().id)
                .collect::<Vec<_>>();
            ids.sort();
            ids
        }
    );
    assert_eq!(outcome.association_count, 2);
}

#[test]
fn exact_retry_is_idempotent_for_chunk_snapshot_and_capture_link() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let first = ingest(&mut bundle, &survey, &batch, &request(61), &NeverCancel).unwrap();
    let revision = bundle.manifest().unwrap().revision;
    let second = ingest(&mut bundle, &survey, &batch, &request(61), &NeverCancel).unwrap();
    assert_eq!(first.publication, second.publication);
    assert_eq!(bundle.manifest().unwrap().revision, revision);
    assert_eq!(bundle.list_observation_chunks().unwrap().len(), 1);
    assert_eq!(bundle.list_survey_snapshot_history(None).unwrap().len(), 1);
}

#[test]
fn capture_manifest_hash_tampering_is_detected_on_publication_read() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(62), &NeverCancel).unwrap();
    let hash = String::from(outcome.publication.manifest().hash());
    let artifact_path = path.join("artifacts").join(&hash);
    let mut bytes = std::fs::read(&artifact_path).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&artifact_path, bytes).unwrap();
    assert!(matches!(
        bundle.capture_publication(&hash),
        Err(StoreError::Corrupt(_))
    ));
}

#[test]
fn read_only_capture_publication_is_inspectable_but_not_writable() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(63), &NeverCancel).unwrap();
    let hash = String::from(outcome.publication.manifest().hash());
    drop(bundle);

    let mut readonly = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(
        readonly.capture_publication(&hash).unwrap().unwrap().status,
        CapturePublicationStatus::Complete
    );
    let error = ingest(&mut readonly, &survey, &batch, &request(63), &NeverCancel).unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Storage(PortError {
            kind: PortErrorKind::ReadOnly,
            ..
        })
    ));
}

#[test]
fn terminal_empty_and_cancelled_captures_persist_capability_and_status() {
    for (byte, fixture, expected) in [
        (
            70,
            include_bytes!("../../../collectors/macos/fixtures/empty.ndjson").as_slice(),
            CaptureTerminalStatus::Ok,
        ),
        (
            71,
            include_bytes!("../../../collectors/macos/fixtures/error.ndjson").as_slice(),
            CaptureTerminalStatus::Error,
        ),
        (
            72,
            include_bytes!("../../../collectors/macos/fixtures/denied.ndjson").as_slice(),
            CaptureTerminalStatus::PermissionRequired,
        ),
    ] {
        let capture = normalized_capture(fixture, true);
        let status = capture.completion.status;
        assert_eq!(CaptureTerminalStatus::from(status), expected);
        let batch = ReceivedObservationBatch::from_normalized_capture(capture).unwrap();
        assert!(batch.observations().is_empty());
        assert!(batch.manifest().capabilities().as_known().is_some());
        let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("project");
        let mut bundle = project(&path);
        let outcome = ingest(&mut bundle, &survey, &batch, &request(byte), &NeverCancel).unwrap();
        assert!(outcome.publication.chunk().is_none());
        assert_eq!(
            bundle
                .capture_publication(&String::from(outcome.publication.manifest().hash()))
                .unwrap()
                .unwrap()
                .status,
            CapturePublicationStatus::Terminal
        );
        assert_eq!(outcome.survey, survey);
    }
}

#[test]
fn cancellation_after_empty_manifest_publication_is_recoverable() {
    let capture = normalized_capture(
        include_bytes!("../../../collectors/macos/fixtures/empty.ndjson"),
        true,
    );
    let batch = ReceivedObservationBatch::from_normalized_capture(capture).unwrap();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);

    // Calls 1 and 2 occur in ingest, call 3 precedes manifest publication,
    // and call 4 observes the manifest committed before the empty snapshot.
    let error = ingest(
        &mut bundle,
        &survey,
        &batch,
        &request(82),
        &CancelAfter::new(4),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Partial {
            progress,
            error: PortError {
                kind: PortErrorKind::Cancelled,
                ..
            }
        } if progress.snapshot().is_none()
    ));
    let manifest_hash =
        kyberia_project_store::content_hash(&batch.manifest().canonical_bytes().unwrap());
    assert_eq!(
        bundle
            .capture_publication(&manifest_hash)
            .unwrap()
            .unwrap()
            .status,
        CapturePublicationStatus::Terminal
    );
    assert!(
        bundle
            .list_survey_snapshot_history(None)
            .unwrap()
            .is_empty()
    );

    let outcome = ingest(&mut bundle, &survey, &batch, &request(82), &NeverCancel).unwrap();
    assert!(outcome.publication.snapshot().is_some());
    assert_eq!(bundle.list_survey_snapshot_history(None).unwrap().len(), 1);
}

#[test]
fn malformed_source_reference_and_unsupported_evidence_are_rejected_before_writes() {
    let mut capture = normalized_capture(VALID, false);
    capture.source_records[0].bytes[0] ^= 1;
    assert_eq!(
        ReceivedObservationBatch::from_normalized_capture(capture),
        Err(BatchError::SourceReferenceMismatch)
    );

    let batch = batch();
    let survey = PointSurvey::start(config(false), stamp(100)).unwrap();
    let mut port = FakePort::default();
    let error = ingest(&mut port, &survey, &batch, &request(73), &NeverCancel).unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Association {
            error: kyberia_survey::SurveyError::UnusableQuality,
            ..
        }
    ));
    assert!(port.publication.is_none());
}

#[test]
fn retained_raw_records_have_verified_artifact_closure() {
    let batch = retained_batch();
    assert_eq!(
        batch.manifest().raw_source_disposition(),
        RawSourceDisposition::Retained
    );
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(74), &NeverCancel).unwrap();
    assert_eq!(outcome.publication.manifest().raw_record_count(), 5);
    let manifest = bundle.manifest().unwrap();
    for source in batch.manifest().source_records() {
        let hash = String::from(source.reference().sha256);
        assert!(manifest.artifacts.contains_key(&hash));
        assert_eq!(
            bundle.read_artifact(&hash).unwrap().len() as u64,
            source.reference().byte_length
        );
    }
}

#[test]
fn cancellation_before_chunk_leaves_recoverable_manifest_without_association() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let cancel = CancelAfter::new(4);
    let error = ingest(&mut bundle, &survey, &batch, &request(80), &cancel).unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Partial { progress, error: PortError { kind: PortErrorKind::Cancelled, .. } }
            if progress.chunk().is_none() && progress.snapshot().is_none()
    ));
    let publication = bundle
        .capture_publication(
            &batch
                .manifest()
                .canonical_bytes()
                .map(|bytes| kyberia_project_store::content_hash(&bytes))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        publication.unwrap().status,
        CapturePublicationStatus::Manifest
    );
    assert!(bundle.list_observation_chunks().unwrap().is_empty());
    assert!(
        bundle
            .list_survey_snapshot_history(None)
            .unwrap()
            .is_empty()
    );
    let outcome = ingest(&mut bundle, &survey, &batch, &request(80), &NeverCancel).unwrap();
    assert_eq!(outcome.association_count, 1);
}

#[test]
fn cancellation_during_retained_raw_publication_is_recoverable() {
    let batch = retained_batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let cancel = CancelAfter::new(5);
    let error = ingest(&mut bundle, &survey, &batch, &request(81), &cancel).unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Partial { progress, error: PortError { kind: PortErrorKind::Cancelled, .. } }
            if progress.raw_record_count() == 1
                && progress.chunk().is_none()
                && progress.snapshot().is_none()
    ));
    let outcome = ingest(&mut bundle, &survey, &batch, &request(81), &NeverCancel).unwrap();
    assert_eq!(
        outcome.publication.raw_record_count(),
        batch.manifest().source_records().len() as u64
    );
}

#[test]
fn cancellation_and_failure_after_chunk_are_explicit_and_retryable() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let flag = Rc::new(Cell::new(false));
    let mut port = FakePort {
        cancel_after_chunk: Some(flag.clone()),
        ..FakePort::default()
    };
    let error = ingest(&mut port, &survey, &batch, &request(75), &|| flag.get()).unwrap_err();
    assert!(matches!(error, PipelineError::Partial { .. }));
    assert!(flag.get());

    let mut failing = FakePort {
        fail_after_chunk: true,
        ..FakePort::default()
    };
    let error = ingest(&mut failing, &survey, &batch, &request(76), &NeverCancel).unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Partial {
            progress,
            error: PortError { kind: PortErrorKind::Io, .. }
        } if progress.chunk().is_some() && progress.snapshot().is_none()
    ));
}

#[test]
fn port_receipts_are_checked_for_counts_identity_and_revision() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let mut wrong_count = FakePort {
        wrong_count: true,
        ..FakePort::default()
    };
    let error = ingest(
        &mut wrong_count,
        &survey,
        &batch,
        &request(77),
        &NeverCancel,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Storage(PortError {
            kind: PortErrorKind::Corrupt,
            ..
        })
    ));

    let mut wrong_snapshot = FakePort {
        wrong_snapshot: true,
        ..FakePort::default()
    };
    let error = ingest(
        &mut wrong_snapshot,
        &survey,
        &batch,
        &request(78),
        &NeverCancel,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        PipelineError::Storage(PortError {
            kind: PortErrorKind::Corrupt,
            ..
        })
    ));
}

#[test]
fn future_wire_and_duplicate_observations_fail_closed() {
    let mut capture = normalized_capture(VALID, false);
    capture.completion.observation_count = 0;
    assert_eq!(
        ReceivedObservationBatch::from_normalized_capture(capture.clone()),
        Err(BatchError::CompletionMismatch)
    );
    capture.completion.observation_count = 2;
    capture.observations.push(capture.observations[0].clone());
    assert!(matches!(
        ReceivedObservationBatch::from_normalized_capture(capture),
        Err(BatchError::DuplicateObservation(_))
    ));
    let mut wire = serde_json::to_value(request(79)).unwrap();
    wire["schema_version"] = serde_json::json!("2");
    assert!(serde_json::from_value::<PipelineRequest>(wire).is_err());
}

#[test]
fn public_port_rejects_manifest_observation_mismatch_before_publication() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let req = request(60);
    let result = bundle.persist_capture(CapturePersistenceRequest {
        manifest: batch.manifest(),
        raw_records: &[],
        observations: &[],
        snapshot_id: req.snapshot_id(),
        survey: &survey,
        provenance_id: &req.provenance_id,
        published_utc_ms: req.published_utc_ms,
        cancel: &NeverCancel,
    });
    assert!(result.is_err());
    assert_eq!(bundle.manifest().unwrap().revision, 0);
}

#[test]
fn public_port_rejects_unassociated_survey_before_publication() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let req = request(83);
    let envelope = batch.observations()[0].envelope().clone();
    let result = bundle.persist_capture(CapturePersistenceRequest {
        manifest: batch.manifest(),
        raw_records: &[],
        observations: std::slice::from_ref(&envelope),
        snapshot_id: req.snapshot_id(),
        survey: &survey,
        provenance_id: &req.provenance_id,
        published_utc_ms: req.published_utc_ms,
        cancel: &NeverCancel,
    });
    assert!(matches!(
        result,
        Err(PortError {
            kind: PortErrorKind::Corrupt,
            ..
        })
    ));
    assert_eq!(bundle.manifest().unwrap().revision, 0);
}

#[test]
fn capture_links_reject_same_count_chunk_with_different_observation_identity() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(84), &NeverCancel).unwrap();
    let (envelope, _) = batch.observations()[0].clone().into_parts();
    let mut data = envelope.into_data();
    data.id = ObservationId::from_bytes([44; 16]).unwrap();
    let unrelated = ObservationEnvelope::new(data).unwrap();
    let wrong_chunk = bundle
        .publish_observation_chunk(&[unrelated], "unrelated-chunk/v1", 4)
        .unwrap();
    let manifest_hash = String::from(outcome.publication.manifest().hash());
    assert!(matches!(
        bundle.link_capture_chunk(&manifest_hash, wrong_chunk.hash(), wrong_chunk.row_count()),
        Err(StoreError::Corrupt(_))
    ));
    assert_eq!(
        bundle
            .capture_publication(&manifest_hash)
            .unwrap()
            .unwrap()
            .chunk_hash
            .as_deref(),
        Some(String::from(outcome.publication.chunk().unwrap().hash()).as_str())
    );
}

#[test]
fn capture_links_reject_same_project_snapshot_without_manifest_associations() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(85), &NeverCancel).unwrap();
    let unrelated_survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let unrelated_snapshot = SnapshotId::from_bytes([86; 16]).unwrap();
    bundle
        .save_survey_snapshot(unrelated_snapshot, &unrelated_survey, 5)
        .unwrap();
    let manifest_hash = String::from(outcome.publication.manifest().hash());
    assert!(matches!(
        bundle.link_capture_snapshot(&manifest_hash, unrelated_snapshot, &unrelated_survey),
        Err(StoreError::Corrupt(_))
    ));
    assert_eq!(
        bundle
            .capture_publication(&manifest_hash)
            .unwrap()
            .unwrap()
            .snapshot_id,
        Some(outcome.publication.snapshot().unwrap().snapshot_id())
    );
}

#[test]
fn capture_snapshot_link_rejects_same_id_with_different_canonical_metadata() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let manifest = bundle
        .persist_capture_manifest(
            CaptureManifestRegistration::new(
                batch.manifest().clone(),
                text("native-macos-capture/v1"),
                2,
            )
            .unwrap(),
        )
        .unwrap();
    let descriptor = bundle
        .publish_observation_chunk(
            &[batch.observations()[0].envelope().clone()],
            ObservationChunkProvenance::new("native-macos-capture/v1").unwrap(),
            3,
        )
        .unwrap();
    bundle
        .link_capture_chunk(
            &manifest.manifest_hash,
            descriptor.hash(),
            descriptor.row_count(),
        )
        .unwrap();

    let (envelope, response) = batch.observations()[0].clone().into_parts();
    let mut data = envelope.into_data();
    data.time.wall = Evidence::Unknown(UnknownReason::SourceDidNotProvide);
    data.pose = Evidence::Unknown(UnknownReason::NotApplicable);
    data.raw_source = Evidence::Known(kyberia_domain::evidence::ArtifactReference {
        sha256: ContentHash::from_sha256([77; 32]),
        media_type: text("application/octet-stream"),
        byte_length: 77,
    });
    let different =
        ReceivedObservation::new(ObservationEnvelope::new(data).unwrap(), response).unwrap();
    let (different_survey, _) = survey.associate_received(&different).unwrap();
    let different_snapshot = SnapshotId::from_bytes([87; 16]).unwrap();
    bundle
        .save_survey_snapshot(different_snapshot, &different_survey, 5)
        .unwrap();

    assert!(matches!(
        bundle.link_capture_snapshot(&manifest.manifest_hash, different_snapshot, &different_survey),
        Err(StoreError::Corrupt(message))
            if message.contains("differs from its canonical observation")
    ));
    assert_eq!(
        bundle
            .capture_publication(&manifest.manifest_hash)
            .unwrap()
            .unwrap()
            .snapshot_id,
        None
    );
}

#[test]
fn capture_snapshot_link_rejects_canonical_source_identity_substitution() {
    let batch = batch();
    let mut data = batch.observations()[0].envelope().clone().into_data();
    data.source.collector_id = CollectorId::from_bytes([99; 16]).unwrap();
    data.source.adapter_version = text("collector/changed");
    let envelope = ObservationEnvelope::new(data).unwrap();
    let (_directory, mut bundle, manifest_hash) = stage_manifest_and_chunk(envelope);
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let (survey, _) = survey.associate_received(&batch.observations()[0]).unwrap();
    let snapshot_id = SnapshotId::from_bytes([88; 16]).unwrap();
    bundle
        .save_survey_snapshot(snapshot_id, &survey, 5)
        .unwrap();

    assert!(matches!(
        bundle.link_capture_snapshot(&manifest_hash, snapshot_id, &survey),
        Err(StoreError::Corrupt(message))
            if message.contains("survey source identity or mode")
    ));
}

#[test]
fn capture_snapshot_link_rejects_canonical_capture_mode_substitution() {
    let batch = batch();
    let data = batch.observations()[0].envelope().clone().into_data();
    let scan = match data.payload {
        kyberia_domain::observation::ObservationPayload::Scan(scan) => scan,
        _ => unreachable!(),
    };
    let frame = kyberia_domain::observation::FrameMetadata {
        identity: scan.identity,
        signal: scan.signal,
        frame_type: Evidence::Unknown(UnknownReason::NotApplicable),
        frame_subtype: Evidence::Unknown(UnknownReason::NotApplicable),
        retry: Evidence::Unknown(UnknownReason::NotApplicable),
        length_bytes: 0,
        phy_rate_mbps: Evidence::Unknown(UnknownReason::NotApplicable),
        raw_information_elements: Evidence::Unknown(UnknownReason::NotApplicable),
    };
    let mut data = batch.observations()[0].envelope().clone().into_data();
    data.payload = kyberia_domain::observation::ObservationPayload::Frame(frame);
    let envelope = ObservationEnvelope::new(data).unwrap();
    let (_directory, mut bundle, manifest_hash) = stage_manifest_and_chunk(envelope);
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let (survey, _) = survey.associate_received(&batch.observations()[0]).unwrap();
    let snapshot_id = SnapshotId::from_bytes([89; 16]).unwrap();
    bundle
        .save_survey_snapshot(snapshot_id, &survey, 5)
        .unwrap();

    assert!(matches!(
        bundle.link_capture_snapshot(&manifest_hash, snapshot_id, &survey),
        Err(StoreError::Corrupt(message))
            if message.contains("survey source identity or mode")
    ));
}

#[test]
fn capture_publication_readback_rejects_contradictory_snapshot_association() {
    let batch = batch();
    let survey = PointSurvey::start(config(true), stamp(100)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let mut bundle = project(&path);
    let outcome = ingest(&mut bundle, &survey, &batch, &request(90), &NeverCancel).unwrap();
    let manifest_hash = String::from(outcome.publication.manifest().hash());

    let (envelope, response) = batch.observations()[0].clone().into_parts();
    let mut data = envelope.into_data();
    data.time.wall = Evidence::Unknown(UnknownReason::SourceDidNotProvide);
    data.pose = Evidence::Unknown(UnknownReason::NotApplicable);
    let different =
        ReceivedObservation::new(ObservationEnvelope::new(data).unwrap(), response).unwrap();
    let (different_survey, _) = survey.associate_received(&different).unwrap();
    let different_snapshot = SnapshotId::from_bytes([91; 16]).unwrap();
    bundle
        .save_survey_snapshot(different_snapshot, &different_survey, 5)
        .unwrap();
    drop(bundle);

    let database = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute(
            "UPDATE capture_publications SET snapshot_id=?1 WHERE manifest_hash=?2",
            (String::from(different_snapshot), manifest_hash.as_str()),
        )
        .unwrap();
    drop(database);

    let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        reopened.capture_publication(&manifest_hash),
        Err(StoreError::Corrupt(message))
            if message.contains("differs from its canonical observation")
    ));
}

#[test]
fn raw_reference_metadata_must_match_the_referenced_source_record() {
    let mut capture = normalized_capture(VALID, false);
    let (envelope, response) = capture.observations[0].clone().into_parts();
    let mut data = envelope.into_data();
    let Evidence::Known(reference) = &mut data.raw_source else {
        panic!("fixture must have a raw reference");
    };
    reference.byte_length += 1;
    capture.observations[0] =
        ReceivedObservation::new(ObservationEnvelope::new(data).unwrap(), response).unwrap();
    assert!(matches!(
        ReceivedObservationBatch::from_normalized_capture(capture),
        Err(BatchError::SourceReferenceMismatch)
    ));
}
