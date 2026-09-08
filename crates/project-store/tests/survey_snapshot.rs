use kyberia_domain::{
    capability::*, evidence::*, identity::*, observation::*, spatial::*, time::*, units::*,
};
use kyberia_project_store::{
    Bundle, MAX_SURVEY_SNAPSHOT_BYTES, OpenMode, StoreError, content_hash,
};
use kyberia_survey::*;
use rusqlite::Connection;
use std::{collections::BTreeMap, fs, num::NonZeroU32};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

const LEGACY: &str = include_str!("../../survey/tests/fixtures/point-v1.json");

fn fixture_state() -> PointSurvey {
    serde_json::from_str(LEGACY).unwrap()
}

fn fixture_state_for_session(byte: u8) -> PointSurvey {
    let mut wire: serde_json::Value = serde_json::from_str(LEGACY).unwrap();
    wire["config"]["session_id"] =
        serde_json::Value::String(SessionId::from_bytes([byte; 16]).unwrap().into());
    serde_json::from_value(wire).unwrap()
}

fn project(root: &std::path::Path) -> Bundle {
    Bundle::create(
        root,
        ProjectId::from_bytes([1; 16]).unwrap(),
        "Survey snapshots".into(),
        1,
    )
    .unwrap()
}

fn snapshot_id(byte: u8) -> SnapshotId {
    SnapshotId::from_bytes([byte; 16]).unwrap()
}

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

fn association_anchor() -> PoseReference {
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

fn association_config() -> PointConfig {
    let collector_id = CollectorId::from_bytes([4; 16]).unwrap();
    PointConfig::new(PointConfigData {
        schema_version: SchemaVersion::V1,
        point_id: PointId::from_bytes([5; 16]).unwrap(),
        session_id: SessionId::from_bytes([6; 16]).unwrap(),
        anchor: association_anchor(),
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
        metrics: BTreeMap::from([(PointMetric::Rssi, NonZeroU32::new(1).unwrap())]),
        channels: vec![],
        minimum_active_time: Seconds::new(0.).unwrap(),
        maximum_scan_age: Seconds::new(1.).unwrap(),
        target: Target::AnyBssid,
        pose_policy: PosePolicy::ManualAnchor {
            maximum_reported_offset: Meters::new(1.).unwrap(),
        },
        required_capabilities: vec![],
        allow_synthetic: false,
        method_version: text("point/v1"),
    })
    .unwrap()
}

fn association_envelope(id: u8) -> ObservationEnvelope {
    ObservationEnvelope::new(EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes([id; 16]).unwrap(),
        session_id: association_config().data().session_id,
        source: SourceDescriptor {
            source_id: association_config().data().source_id,
            collector_id: association_config().data().collector_id,
            sensor_id: unknown(),
            adapter_id: unknown(),
            kind: SourceKind::NativeApi,
            source_name: text("CoreWLAN"),
            source_version: unknown(),
            source_schema_version: text("macos-wire/1"),
            adapter_name: text("kyberia-macos"),
            adapter_version: text("collector/1"),
            parser_version: text("parser/1"),
            driver_version: unknown(),
            os_version: unknown(),
        },
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

fn associated_state() -> PointSurvey {
    let response = SourceResponseTiming::new(
        CaptureTime {
            wall: Evidence::Unknown(UnknownReason::ClockUnavailable),
            monotonic: Evidence::Known(stamp(150)),
            synchronization: Evidence::Unknown(UnknownReason::ClockUnavailable),
        },
        Evidence::Unknown(UnknownReason::SourceDidNotProvide),
    )
    .unwrap();
    let received =
        ReceivedObservation::new(association_envelope(1), Evidence::Known(response)).unwrap();
    PointSurvey::start(association_config(), stamp(100))
        .unwrap()
        .associate_received(&received)
        .unwrap()
        .0
}

fn database_envelope(path: &std::path::Path) -> Vec<(String, Vec<u8>, u64, u32)> {
    ["", "-wal", "-journal", "-shm"]
        .into_iter()
        .filter_map(|suffix| {
            let file = path.join(format!("project.sqlite{suffix}"));
            let metadata = fs::symlink_metadata(&file).ok()?;
            let mode = {
                #[cfg(unix)]
                {
                    metadata.mode()
                }
                #[cfg(not(unix))]
                {
                    0
                }
            };
            Some((
                suffix.to_string(),
                fs::read(file).unwrap(),
                metadata.len(),
                mode,
            ))
        })
        .collect()
}

fn envelope_signature(envelope: &[(String, Vec<u8>, u64, u32)]) -> Vec<(String, String, u64, u32)> {
    envelope
        .iter()
        .map(|(suffix, bytes, length, mode)| (suffix.clone(), content_hash(bytes), *length, *mode))
        .collect()
}

fn durable_envelope_signature(
    envelope: &[(String, Vec<u8>, u64, u32)],
) -> Vec<(String, String, u64, u32)> {
    envelope
        .iter()
        .filter(|(suffix, _, _, _)| suffix != "-shm")
        .map(|(suffix, bytes, length, mode)| (suffix.clone(), content_hash(bytes), *length, *mode))
        .collect()
}

#[test]
fn save_reopen_and_associations_roundtrip_with_receipt() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let state = associated_state();
    let session = state.config().data().session_id;
    let mut bundle = project(&path);
    let record = bundle
        .save_survey_snapshot(snapshot_id(9), &state, 2)
        .unwrap();
    assert_eq!(record.revision, 1);
    assert_eq!(record.session_id, session);
    assert_eq!(record.source_id, state.config().data().source_id);
    assert_eq!(record.collector_id, state.config().data().collector_id);
    assert_eq!(bundle.manifest().unwrap().revision, 1);
    let history = bundle.list_survey_snapshot_history(None).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].source_id, state.config().data().source_id);
    assert_eq!(history[0].collector_id, state.config().data().collector_id);
    let loaded = bundle
        .load_survey_snapshot_for_session(snapshot_id(9), Some(session))
        .unwrap();
    assert_eq!(loaded.survey, state);
    assert_eq!(loaded.survey.associations().len(), 1);
    assert!(!loaded.decode_receipt.migrated);
    drop(bundle);

    let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let loaded = reopened.load_survey_snapshot(snapshot_id(9)).unwrap();
    assert_eq!(loaded.survey, state);
    assert!(reopened.verify().unwrap().failures.is_empty());
}

#[test]
fn legacy_import_keeps_input_bytes_and_returns_migration_receipt() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let state = fixture_state();
    let mut bundle = project(&path);
    let record = bundle
        .import_survey_snapshot(snapshot_id(1), LEGACY.as_bytes(), 2, None)
        .unwrap();
    assert_eq!(
        record.input_schema_version,
        PointSnapshotInputVersion::LegacyUntaggedV1
    );
    let loaded = bundle.load_survey_snapshot(snapshot_id(1)).unwrap();
    assert!(loaded.decode_receipt.migrated);
    assert_eq!(loaded.survey, state);
    assert_eq!(
        bundle.read_artifact(&record.artifact_hash).unwrap(),
        LEGACY.as_bytes()
    );
}

#[test]
fn duplicate_identity_is_idempotent_and_revisions_preserve_history() {
    let dir = tempfile::tempdir().unwrap().keep();
    let mut bundle = project(&dir.join("project"));
    let state = fixture_state();
    let first = bundle
        .save_survey_snapshot(snapshot_id(1), &state, 2)
        .unwrap();
    let duplicate = bundle
        .save_survey_snapshot(snapshot_id(1), &state, 3)
        .unwrap();
    assert_eq!(duplicate, first);
    assert_eq!(bundle.manifest().unwrap().revision, 1);
    let second = bundle
        .save_survey_snapshot(snapshot_id(2), &state, 4)
        .unwrap();
    assert_eq!(second.revision, 2);
    let history = bundle.list_survey_snapshot_history(None).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].revision, 1);
    assert_eq!(history[1].revision, 2);
}

#[test]
fn malformed_future_and_oversized_input_fail_without_revision_changes() {
    let dir = tempfile::tempdir().unwrap().keep();
    let mut bundle = project(&dir.join("project"));
    for (id, bytes) in [
        (1, b"not json".to_vec()),
        (2, br#"{"schema_version":"3"}"#.to_vec()),
    ] {
        assert!(
            bundle
                .import_survey_snapshot(snapshot_id(id), &bytes, 2, None)
                .is_err()
        );
        assert_eq!(bundle.manifest().unwrap().revision, 0);
    }
    let oversized = vec![b' '; (MAX_SURVEY_SNAPSHOT_BYTES + 1) as usize];
    assert!(matches!(
        bundle.import_survey_snapshot(snapshot_id(3), &oversized, 2, None),
        Err(StoreError::Corrupt(message)) if message.contains("8 MiB")
    ));
    assert_eq!(bundle.list_survey_snapshot_history(None).unwrap().len(), 0);
}

#[test]
fn checksum_missing_blob_and_wrong_session_fail_closed() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = project(&path);
    let state = fixture_state();
    let session = state.config().data().session_id;
    let record = bundle
        .save_survey_snapshot(snapshot_id(4), &state, 2)
        .unwrap();
    let other_session = SessionId::from_bytes([8; 16]).unwrap();
    assert!(matches!(
        bundle.load_survey_snapshot_for_session(snapshot_id(4), Some(other_session)),
        Err(StoreError::Invalid(message)) if message.contains("session mismatch")
    ));
    let artifact = path.join("artifacts").join(&record.artifact_hash);
    fs::write(&artifact, b"tampered").unwrap();
    assert!(matches!(
        bundle.load_survey_snapshot(snapshot_id(4)),
        Err(StoreError::Corrupt(message)) if message.contains("checksum")
    ));
    fs::remove_file(&artifact).unwrap();
    assert!(bundle.load_survey_snapshot(snapshot_id(4)).is_err());
    assert_eq!(bundle.manifest().unwrap().revision, 1);
    assert!(
        bundle
            .load_survey_snapshot_for_session(snapshot_id(4), Some(session))
            .is_err()
    );
}

#[test]
fn optimistic_revision_rejects_stale_handle_without_partial_metadata() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut first = project(&path);
    let mut second = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let state = fixture_state();
    first
        .save_survey_snapshot_if_revision(snapshot_id(5), &state, 2, Some(0))
        .unwrap();
    assert!(matches!(
        second.save_survey_snapshot_if_revision(snapshot_id(6), &state, 3, Some(0)),
        Err(StoreError::Invalid(message)) if message.contains("stale project revision")
    ));
    assert_eq!(second.manifest().unwrap().revision, 1);
    assert_eq!(second.list_survey_snapshot_history(None).unwrap().len(), 1);
    second
        .save_survey_snapshot_if_revision(snapshot_id(6), &state, 4, Some(1))
        .unwrap();
    assert_eq!(second.manifest().unwrap().revision, 2);
}

#[test]
fn projection_failure_rolls_back_snapshot_index_and_revision() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = project(&path);
    let state = fixture_state();
    let before = bundle.manifest().unwrap();
    fs::rename(path.join("manifest.json"), path.join("manifest.saved")).unwrap();
    fs::create_dir(path.join("manifest.json")).unwrap();
    assert!(
        bundle
            .save_survey_snapshot(snapshot_id(7), &state, 2)
            .is_err()
    );
    assert_eq!(bundle.manifest().unwrap(), before);
    assert!(
        bundle
            .list_survey_snapshot_history(None)
            .unwrap()
            .is_empty()
    );
    assert!(bundle.load_survey_snapshot(snapshot_id(7)).is_err());
}

#[test]
fn manifest_only_legacy_bundle_adds_snapshot_tables_transactionally() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let bundle = project(&path);
    drop(bundle);
    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute_batch("DROP TABLE survey_snapshot_history; DROP TABLE survey_snapshots;")
        .unwrap();
    drop(database);

    let mut upgraded = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    assert_eq!(upgraded.manifest().unwrap().revision, 0);
    upgraded
        .save_survey_snapshot(snapshot_id(8), &fixture_state(), 2)
        .unwrap();
    let history = upgraded.list_survey_snapshot_history(None).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].source_id,
        fixture_state().config().data().source_id
    );
    assert_eq!(
        history[0].collector_id,
        fixture_state().config().data().collector_id
    );
}

#[test]
fn future_manifest_only_rw_open_is_side_effect_free() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let bundle = project(&path);
    let mut future = bundle.manifest().unwrap();
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute_batch("DROP TABLE survey_snapshot_history; DROP TABLE survey_snapshots;")
        .unwrap();
    future.schema_version = 2;
    database
        .execute(
            "UPDATE bundle_manifest SET body=?1",
            [serde_json::to_vec(&future).unwrap()],
        )
        .unwrap();
    drop(database);
    let before = database_envelope(&path);
    assert!(matches!(
        Bundle::open(&path, OpenMode::ReadWrite),
        Err(StoreError::UnsupportedVersion(2))
    ));
    let after = database_envelope(&path);
    assert_eq!(envelope_signature(&after), envelope_signature(&before));
}

#[test]
fn oversized_future_manifest_value_is_bounded_before_writable_open() {
    const SQLITE_MANIFEST_LIMIT: i64 = 4 * 1024 * 1024;
    const DATABASE_LIMIT: u64 = 64 * 1024 * 1024;
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let bundle = project(&path);
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    // SQLite constructs the adversarial value internally, avoiding a large
    // Rust allocation. Keep the physical database below the bundle envelope
    // limit while exceeding the defensive manifest value limit.
    database
        .execute(
            "UPDATE bundle_manifest SET body=zeroblob(?1)",
            [SQLITE_MANIFEST_LIMIT + 1],
        )
        .unwrap();
    database.execute_batch("PRAGMA user_version=2;").unwrap();
    drop(database);
    assert!(fs::metadata(path.join("project.sqlite")).unwrap().len() < DATABASE_LIMIT);
    let before = database_envelope(&path);
    assert!(matches!(
        Bundle::open(&path, OpenMode::ReadWrite),
        Err(StoreError::Sql(_))
    ));
    assert_eq!(
        envelope_signature(&database_envelope(&path)),
        envelope_signature(&before)
    );
}

#[test]
fn future_manifest_wal_preflight_is_side_effect_free() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let bundle = project(&path);
    let mut future = bundle.manifest().unwrap();
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=100000000;",
        )
        .unwrap();
    future.schema_version = 2;
    database
        .execute(
            "UPDATE bundle_manifest SET body=?1",
            [serde_json::to_vec(&future).unwrap()],
        )
        .unwrap();
    let before = database_envelope(&path);
    assert!(matches!(
        Bundle::open(&path, OpenMode::ReadWrite),
        Err(StoreError::UnsupportedVersion(2))
    ));
    let after = database_envelope(&path);
    // SQLite may refresh volatile WAL-index lock state in -shm while opening
    // the read-only compatibility probe. Main and WAL content remain exact.
    assert_eq!(
        durable_envelope_signature(&after),
        durable_envelope_signature(&before)
    );
}

#[test]
fn future_manifest_hot_journal_preflight_does_not_recover_writable() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let bundle = project(&path);
    let mut future = bundle.manifest().unwrap();
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute_batch("DROP TABLE survey_snapshot_history; DROP TABLE survey_snapshots;")
        .unwrap();
    future.schema_version = 2;
    database
        .execute(
            "UPDATE bundle_manifest SET body=?1",
            [serde_json::to_vec(&future).unwrap()],
        )
        .unwrap();
    drop(database);
    // An invalid rollback journal is intentionally treated as unreadable by
    // SQLite's read-only probe. The probe must fail before a writable open can
    // recover or migrate the bundle, and all database envelope bytes remain.
    fs::write(path.join("project.sqlite-journal"), b"future-journal-bytes").unwrap();
    let before = database_envelope(&path);
    assert!(matches!(
        Bundle::open(&path, OpenMode::ReadWrite),
        Err(StoreError::Sql(_))
    ));
    let after = database_envelope(&path);
    assert_eq!(envelope_signature(&after), envelope_signature(&before));
}

#[test]
fn snapshot_timestamp_must_not_regress_manifest_and_equal_is_allowed() {
    let dir = tempfile::tempdir().unwrap().keep();
    let mut bundle = project(&dir.join("project"));
    let state = fixture_state();
    bundle
        .save_survey_snapshot(snapshot_id(11), &state, 1)
        .unwrap();
    assert!(matches!(
        bundle.save_survey_snapshot(snapshot_id(12), &state, 0),
        Err(StoreError::Invalid(message)) if message.contains("precedes")
    ));
    assert_eq!(bundle.manifest().unwrap().revision, 1);
    assert_eq!(bundle.list_survey_snapshot_history(None).unwrap().len(), 1);
}

#[test]
fn missing_extra_and_mismatched_history_fail_load_list_and_duplicate_save() {
    for mutation in ["missing", "extra", "mismatch", "source", "collector"] {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("project");
        let mut bundle = project(&path);
        let state = fixture_state();
        let record = bundle
            .save_survey_snapshot(snapshot_id(13), &state, 2)
            .unwrap();
        drop(bundle);

        let database = Connection::open(path.join("project.sqlite")).unwrap();
        match mutation {
            "missing" => {
                database
                    .execute(
                        "DELETE FROM survey_snapshot_history WHERE snapshot_id=?1",
                        [String::from(snapshot_id(13))],
                    )
                    .unwrap();
            }
            "extra" => {
                database
                    .execute(
                        "INSERT INTO survey_snapshot_history (revision,snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,committed_utc_ms) SELECT 2,snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,committed_utc_ms FROM survey_snapshot_history WHERE snapshot_id=?1",
                        [String::from(snapshot_id(13))],
                    )
                    .unwrap();
            }
            "mismatch" => {
                database
                    .execute(
                        "UPDATE survey_snapshot_history SET point_id=?1 WHERE snapshot_id=?2",
                        (
                            PointId::from_bytes([14; 16]).unwrap().database_key(),
                            String::from(snapshot_id(13)),
                        ),
                    )
                    .unwrap();
            }
            "source" => {
                database
                    .execute(
                        "UPDATE survey_snapshot_history SET source_id=?1 WHERE snapshot_id=?2",
                        (
                            String::from(SourceId::from_bytes([14; 16]).unwrap()),
                            String::from(snapshot_id(13)),
                        ),
                    )
                    .unwrap();
            }
            "collector" => {
                database
                    .execute(
                        "UPDATE survey_snapshot_history SET collector_id=?1 WHERE snapshot_id=?2",
                        (
                            String::from(CollectorId::from_bytes([14; 16]).unwrap()),
                            String::from(snapshot_id(13)),
                        ),
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(database);

        let mut reopened = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
        assert!(reopened.load_survey_snapshot(snapshot_id(13)).is_err());
        assert!(reopened.list_survey_snapshot_history(None).is_err());
        assert!(!reopened.verify().unwrap().failures.is_empty());
        assert!(
            reopened
                .save_survey_snapshot(snapshot_id(13), &state, 3)
                .is_err()
        );
        assert_eq!(reopened.manifest().unwrap().revision, record.revision);
    }
}

#[test]
fn load_rejects_unrelated_missing_or_extra_history() {
    for mutation in ["missing", "extra"] {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("project");
        let mut bundle = project(&path);
        let state = fixture_state();
        bundle
            .save_survey_snapshot(snapshot_id(17), &state, 2)
            .unwrap();
        if mutation == "missing" {
            bundle
                .save_survey_snapshot(snapshot_id(18), &state, 3)
                .unwrap();
        }
        drop(bundle);

        let database = Connection::open(path.join("project.sqlite")).unwrap();
        if mutation == "missing" {
            database
                .execute(
                    "DELETE FROM survey_snapshot_history WHERE snapshot_id=?1",
                    [String::from(snapshot_id(18))],
                )
                .unwrap();
        } else {
            database
                .execute(
                    "INSERT INTO survey_snapshot_history (revision,snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,committed_utc_ms) SELECT 2,?1,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,committed_utc_ms FROM survey_snapshot_history WHERE snapshot_id=?2",
                    (String::from(snapshot_id(18)), String::from(snapshot_id(17))),
                )
                .unwrap();
        }
        drop(database);

        let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(
            bundle.load_survey_snapshot(snapshot_id(17)).is_err(),
            "load concealed unrelated {mutation} history"
        );
    }
}

#[test]
fn snapshot_timestamps_must_remain_inside_manifest_interval_on_replay() {
    for (index_timestamp, history_timestamp) in [(0_i64, 0_i64), (3_i64, 3_i64)] {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("project");
        let mut bundle = project(&path);
        let record = bundle
            .save_survey_snapshot(snapshot_id(19), &fixture_state(), 2)
            .unwrap();
        drop(bundle);
        let database = Connection::open(path.join("project.sqlite")).unwrap();
        database
            .execute(
                "UPDATE survey_snapshots SET created_utc_ms=?1 WHERE snapshot_id=?2",
                (index_timestamp, String::from(record.snapshot_id)),
            )
            .unwrap();
        database
            .execute(
                "UPDATE survey_snapshot_history SET committed_utc_ms=?1 WHERE snapshot_id=?2",
                (history_timestamp, String::from(record.snapshot_id)),
            )
            .unwrap();
        drop(database);
        let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(bundle.load_survey_snapshot(record.snapshot_id).is_err());
        assert!(bundle.list_survey_snapshot_history(None).is_err());
        assert!(!bundle.verify().unwrap().failures.is_empty());
    }
}

#[test]
fn filtered_history_listing_replays_and_validates_excluded_sessions() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = project(&path);
    let first = fixture_state_for_session(20);
    let second = fixture_state_for_session(21);
    let first_record = bundle
        .save_survey_snapshot(snapshot_id(20), &first, 2)
        .unwrap();
    let second_record = bundle
        .save_survey_snapshot(snapshot_id(21), &second, 3)
        .unwrap();
    drop(bundle);
    fs::write(
        path.join("artifacts").join(&second_record.artifact_hash),
        b"corrupt",
    )
    .unwrap();
    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(
        bundle
            .list_survey_snapshot_history(Some(first_record.session_id))
            .is_err(),
        "filtered listing concealed another session's corrupt evidence"
    );
}

#[test]
fn history_query_is_bounded_before_allocation() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let bundle = project(&path);
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    let snapshot = String::from(snapshot_id(15));
    let project_id = String::from(ProjectId::from_bytes([1; 16]).unwrap());
    let session_id = String::from(SessionId::from_bytes([6; 16]).unwrap());
    let point_id = PointId::from_bytes([5; 16]).unwrap().database_key();
    let source_id = String::from(SourceId::from_bytes([7; 16]).unwrap());
    let collector_id = String::from(CollectorId::from_bytes([4; 16]).unwrap());
    let hash = "00".repeat(32);
    for revision in 1..=4097_i64 {
        database
            .execute(
                "INSERT INTO survey_snapshot_history (revision,snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,committed_utc_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                (
                    revision,
                    &snapshot,
                    &project_id,
                    &session_id,
                    &point_id,
                    &source_id,
                    &collector_id,
                    &hash,
                    "2",
                    "2",
                    "kyberia-point-snapshot/2.0.0",
                    "collector/1",
                    "survey_snapshot_commit/v1",
                    1_i64,
                ),
            )
            .unwrap();
    }
    drop(database);

    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        bundle.list_survey_snapshot_history(None),
        Err(StoreError::Corrupt(message)) if message.contains("exceeds resource limit")
    ));
}

#[test]
fn history_listing_rejects_tampered_artifact_metadata_and_bytes() {
    for tamper in ["media", "provenance", "bytes"] {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("project");
        let mut bundle = project(&path);
        let record = bundle
            .save_survey_snapshot(snapshot_id(16), &fixture_state(), 2)
            .unwrap();
        drop(bundle);

        if tamper == "bytes" {
            fs::write(
                path.join("artifacts").join(&record.artifact_hash),
                b"tampered",
            )
            .unwrap();
        } else {
            let database = Connection::open(path.join("project.sqlite")).unwrap();
            let body: Vec<u8> = database
                .query_row("SELECT body FROM bundle_manifest", [], |row| row.get(0))
                .unwrap();
            let mut manifest: kyberia_project_store::BundleManifest =
                serde_json::from_slice(&body).unwrap();
            if tamper == "media" {
                manifest
                    .artifacts
                    .get_mut(&record.artifact_hash)
                    .unwrap()
                    .media_type = "application/octet-stream".into();
            } else {
                manifest
                    .artifacts
                    .get_mut(&record.artifact_hash)
                    .unwrap()
                    .provenance_id = "tampered".into();
            }
            database
                .execute(
                    "UPDATE bundle_manifest SET body=?1",
                    [serde_json::to_vec(&manifest).unwrap()],
                )
                .unwrap();
        }
        let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(
            bundle.list_survey_snapshot_history(None).is_err(),
            "accepted {tamper}"
        );
    }
}

#[test]
fn corrupted_index_identity_and_future_index_schema_fail_closed() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = project(&path);
    let state = fixture_state();
    let record = bundle
        .save_survey_snapshot(snapshot_id(10), &state, 2)
        .unwrap();
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute(
            "UPDATE survey_snapshots SET project_id=?1 WHERE snapshot_id=?2",
            (
                "02020202020202020202020202020202",
                String::from(snapshot_id(10)),
            ),
        )
        .unwrap();
    drop(database);
    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        bundle.load_survey_snapshot(snapshot_id(10)),
        Err(StoreError::Corrupt(_))
    ));
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute(
            "UPDATE survey_snapshots SET project_id=?1,source_id=?2 WHERE snapshot_id=?3",
            (
                "01010101010101010101010101010101",
                String::from(SourceId::from_bytes([2; 16]).unwrap()),
                String::from(snapshot_id(10)),
            ),
        )
        .unwrap();
    drop(database);
    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        bundle.load_survey_snapshot(snapshot_id(10)),
        Err(StoreError::Corrupt(_))
    ));
    drop(bundle);

    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute(
            "UPDATE survey_snapshots SET source_id=?1,input_schema='3' WHERE snapshot_id=?2",
            (
                String::from(SourceId::from_bytes([1; 16]).unwrap()),
                String::from(snapshot_id(10)),
            ),
        )
        .unwrap();
    drop(database);
    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        bundle.load_survey_snapshot(snapshot_id(10)),
        Err(StoreError::UnsupportedVersion(3))
    ));
    assert_eq!(record.revision, 1);
}
