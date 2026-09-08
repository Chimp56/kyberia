use bytes::Bytes;
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{
        AdapterId, ClockEpochId, CollectorId, ContentHash, ObservationId, SessionId, SourceId, Ssid,
    },
    observation::{
        CalibrationState, CaptureHealth, ChainSignal, EnvelopeData, FrameMetadata,
        IdentifierPolicy, ObservationEnvelope, ObservationPayload, ObservationSchemaVersion,
        RadioIdentityEvidence, ScanObservation, SignalReading, SourceDescriptor, SourceKind,
    },
    time::{CaptureTime, ClockModel, MonotonicTimestamp, UtcTimestamp, WallClockReading},
    units::{Dbm, Seconds},
};
use kyberia_project_store::{
    ArtifactEntry, ArtifactKind, Bundle, MAX_OBSERVATION_CHUNK_BYTES, MAX_OBSERVATION_CHUNK_ROWS,
    MAX_OBSERVATION_QUERY_IDS, ObservationChunkProvenance, OpenMode, StoreError, content_hash,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::{
    env, fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

fn text(value: &str) -> kyberia_domain::identity::Text {
    kyberia_domain::identity::Text::new(value).unwrap()
}

fn unknown<T>(reason: UnknownReason) -> Evidence<T> {
    Evidence::Unknown(reason)
}

fn observation(id: u8, utc_ns: i64, monotonic_ns: u64) -> ObservationEnvelope {
    let epoch = ClockEpochId::from_bytes([7; 16]).unwrap();
    let source_id = SourceId::from_bytes([3; 16]).unwrap();
    let session_id = SessionId::from_bytes([4; 16]).unwrap();
    ObservationEnvelope::new(EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes([id; 16]).unwrap(),
        session_id,
        source: SourceDescriptor {
            source_id,
            collector_id: CollectorId::from_bytes([5; 16]).unwrap(),
            sensor_id: unknown(UnknownReason::SourceDidNotProvide),
            adapter_id: Evidence::Known(AdapterId::from_bytes([6; 16]).unwrap()),
            kind: SourceKind::NativeApi,
            source_name: text("fixture-source"),
            source_version: unknown(UnknownReason::SourceDidNotProvide),
            source_schema_version: text("source-wire/2"),
            adapter_name: text("fixture-adapter"),
            adapter_version: text("adapter/1"),
            parser_version: text("parser/1"),
            driver_version: unknown(UnknownReason::NotApplicable),
            os_version: Evidence::Known(text("test-os")),
        },
        time: CaptureTime {
            wall: Evidence::Known(WallClockReading {
                time: UtcTimestamp(utc_ns),
                source: text("fixture-clock"),
                precision: Seconds::new(0.000000001).unwrap(),
                uncertainty: unknown(UnknownReason::ClockUnavailable),
            }),
            monotonic: Evidence::Known(MonotonicTimestamp {
                epoch,
                nanoseconds: monotonic_ns,
            }),
            synchronization: Evidence::Known(ClockModel {
                epoch,
                reference_monotonic_nanoseconds: monotonic_ns,
                reference_utc: UtcTimestamp(utc_ns),
                offset_to_reference: unknown(UnknownReason::NotMeasured),
                drift: unknown(UnknownReason::NotMeasured),
                error: unknown(UnknownReason::NotMeasured),
                method_version: text("clock-model/1"),
            }),
        },
        pose: unknown(UnknownReason::NotMeasured),
        channel: unknown(UnknownReason::SourceDidNotProvide),
        dwell: unknown(UnknownReason::NotObservable),
        privacy: kyberia_domain::observation::PrivacyState {
            policy_version: text("privacy/1"),
            identifiers: IdentifierPolicy::OwnedInfrastructure,
            payload: kyberia_domain::observation::PayloadRetention::Discarded,
        },
        quality: vec![],
        raw_source: Evidence::Known(kyberia_domain::evidence::ArtifactReference {
            sha256: ContentHash::from_sha256([9; 32]),
            media_type: text("application/octet-stream"),
            byte_length: 0,
        }),
        payload: ObservationPayload::Scan(ScanObservation {
            identity: RadioIdentityEvidence {
                physical_device: unknown(UnknownReason::NotObservable),
                radio: unknown(UnknownReason::NotObservable),
                bss: unknown(UnknownReason::NotObservable),
                bssid: Evidence::Known(kyberia_domain::identity::MacAddress([1, 2, 3, 4, 5, id])),
                ess: unknown(UnknownReason::NotAdvertised),
                mld: unknown(UnknownReason::NotApplicable),
                link_id: unknown(UnknownReason::NotApplicable),
                client: unknown(UnknownReason::NotApplicable),
                grouping_evidence: unknown(UnknownReason::NotMeasured),
            },
            ssid: unknown(UnknownReason::NotAdvertised),
            signal: SignalReading {
                rssi_dbm: Evidence::Known(Dbm::new(-55.25).unwrap()),
                noise_dbm: unknown(UnknownReason::NotObservable),
                chains: vec![],
                calibration: Evidence::Known(CalibrationState::Uncalibrated),
                measurement_method: text("native scan"),
            },
            information_elements: unknown(UnknownReason::NotRetained),
            result_age: unknown(UnknownReason::SourceDidNotProvide),
        }),
    })
    .unwrap()
}

fn bundle(root: &std::path::Path) -> Bundle {
    Bundle::create(
        root,
        kyberia_domain::identity::ProjectId::from_bytes([1; 16]).unwrap(),
        "chunk fixture".into(),
        1,
    )
    .unwrap()
}

fn payload_variants() -> Vec<ObservationEnvelope> {
    let scan = observation(3, 30, 30);
    let mut frame_data = scan.clone().into_data();
    let (identity, signal) = match &frame_data.payload {
        ObservationPayload::Scan(scan) => (scan.identity.clone(), scan.signal.clone()),
        _ => unreachable!(),
    };
    frame_data.id = ObservationId::from_bytes([4; 16]).unwrap();
    frame_data.payload = ObservationPayload::Frame(FrameMetadata {
        identity,
        signal,
        frame_type: Evidence::Known(1),
        frame_subtype: Evidence::Known(2),
        retry: Evidence::Known(false),
        length_bytes: 128,
        phy_rate_mbps: unknown(UnknownReason::NotAdvertised),
        raw_information_elements: unknown(UnknownReason::NotRetained),
    });
    let frame = ObservationEnvelope::new(frame_data).unwrap();

    let mut health_data = observation(5, 50, 50).into_data();
    health_data.payload = ObservationPayload::Health(CaptureHealth {
        dropped_events: Evidence::Known(7),
        queued_events: unknown(UnknownReason::NotMeasured),
        connected: Evidence::Known(true),
        diagnostic: text("healthy"),
    });
    let health = ObservationEnvelope::new(health_data).unwrap();

    let mut chains_data = observation(6, 60, 60).into_data();
    if let ObservationPayload::Scan(scan) = &mut chains_data.payload {
        scan.signal.chains.push(ChainSignal {
            chain_index: 0,
            rssi_dbm: Evidence::Known(Dbm::new(-56.0).unwrap()),
            noise_dbm: unknown(UnknownReason::NotObservable),
        });
    }
    let chains = ObservationEnvelope::new(chains_data).unwrap();
    vec![scan, frame, health, chains]
}

#[test]
fn complete_envelope_roundtrip_preserves_unknowns_and_clock_extremes() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let mut project = bundle(&root);
    let input = vec![
        observation(2, i64::MAX, u64::MAX),
        observation(1, i64::MIN, 0),
    ];
    let descriptor = project
        .publish_observation_chunk(
            &input,
            ObservationChunkProvenance::new("fixture:complete-envelope-v1").unwrap(),
            2,
        )
        .unwrap();
    let artifact = fs::read(root.join("artifacts").join(descriptor.hash())).unwrap();
    assert_eq!(artifact.get(..4), Some(&b"PAR1"[..]));
    assert_eq!(
        project
            .read_observation_chunk_parquet(descriptor.hash())
            .unwrap(),
        artifact
    );
    assert_eq!(
        artifact.get(artifact.len().saturating_sub(4)..),
        Some(&b"PAR1"[..])
    );
    assert_eq!(descriptor.row_count(), 2);
    assert_eq!(descriptor.first_observation_id(), input[1].data().id);
    assert_eq!(descriptor.last_observation_id(), input[0].data().id);
    assert_eq!(
        project.read_observation_chunk(descriptor.hash()).unwrap(),
        vec![input[1].clone(), input[0].clone()]
    );
    assert_eq!(project.list_observation_chunks().unwrap(), vec![descriptor]);
    assert!(project.verify().unwrap().failures.is_empty());
    drop(project);
    let reopened = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(reopened.read_observations().unwrap().len(), 2);
}

#[test]
fn parquet_roundtrip_covers_payload_variants_nonempty_chains_and_is_deterministic() {
    let input = payload_variants();
    let first_dir = tempfile::tempdir().unwrap().keep().join("first");
    let second_dir = tempfile::tempdir().unwrap().keep().join("second");
    let mut first = bundle(&first_dir);
    let mut second = bundle(&second_dir);
    let first_descriptor = first
        .publish_observation_chunk(&input, "fixture:variants", 10)
        .unwrap();
    let mut reversed = input.clone();
    reversed.reverse();
    let second_descriptor = second
        .publish_observation_chunk(&reversed, "fixture:variants", 10)
        .unwrap();
    assert_eq!(first_descriptor.hash(), second_descriptor.hash());
    assert_eq!(
        first
            .read_observation_chunk(first_descriptor.hash())
            .unwrap(),
        {
            let mut expected = input;
            expected.sort_by_key(|observation| observation.data().id);
            expected
        }
    );
}

#[test]
fn disjoint_payload_chunks_expose_one_stable_v2_schema() {
    let variants = payload_variants();
    let scan = variants[0].clone();
    let health = variants[2].clone();
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let scan_descriptor = project
        .publish_observation_chunk(&[scan], "fixture:stable-scan", 10)
        .unwrap();
    let health_descriptor = project
        .publish_observation_chunk(&[health], "fixture:stable-health", 11)
        .unwrap();
    let schema = |hash: &str| {
        let bytes = fs::read(root.join("artifacts").join(hash)).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::from(bytes)).unwrap();
        reader
            .schema()
            .fields()
            .iter()
            .map(|field| (field.name().to_owned(), field.data_type().clone()))
            .collect::<Vec<_>>()
    };
    let scan_schema = schema(scan_descriptor.hash());
    let health_schema = schema(health_descriptor.hash());
    assert_eq!(scan_schema, health_schema);
    let columns = scan_schema
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(columns.contains("payload.kind"));
    assert!(columns.contains("payload.data.diagnostic"));
    assert!(columns.contains("source.source_id"));
}

#[test]
fn pyarrow_reads_complete_stable_schema_when_oracle_is_available() {
    let Some(python) = env::var_os("KYBERIA_PYARROW_PYTHON") else {
        eprintln!("SKIP: set KYBERIA_PYARROW_PYTHON to run the independent PyArrow oracle");
        return;
    };
    let python = Path::new(&python);
    if !python.is_file() {
        eprintln!(
            "SKIP: independent PyArrow oracle is unavailable at {}",
            python.display()
        );
        return;
    }
    let variants = payload_variants();
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let scan_descriptor = project
        .publish_observation_chunk(&[variants[0].clone()], "fixture:pyarrow-scan", 10)
        .unwrap();
    let health_descriptor = project
        .publish_observation_chunk(&[variants[2].clone()], "fixture:pyarrow-health", 11)
        .unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/observation_chunks_pyarrow_oracle.py");
    let scan_artifact = root.join("artifacts").join(scan_descriptor.hash());
    let health_artifact = root.join("artifacts").join(health_descriptor.hash());
    let output = Command::new(python)
        .arg(script)
        .arg("--expected-rows")
        .arg("1")
        .arg(&scan_artifact)
        .arg(&health_artifact)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "PyArrow oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let major = report["pyarrow_version"]
        .as_str()
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.parse::<u64>().ok())
        .expect("oracle reports a semantic PyArrow version");
    assert!(major >= 15, "unsupported PyArrow version: {stdout}");
    assert!(
        stdout.contains("\"pyarrow_supported_range\": \">=15\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"schema_equal\": true"), "{stdout}");
}

#[test]
fn generic_artifact_import_cannot_poison_observation_inventory() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let before = project.manifest().unwrap();
    let result = project.put_artifact(
        b"not a parquet chunk",
        ArtifactEntry {
            kind: ArtifactKind::NormalizedObservations,
            bytes: 19,
            media_type: "application/vnd.apache.parquet".into(),
            provenance_id: "fixture:generic-import".into(),
        },
        10,
    );
    assert!(
        matches!(result, Err(StoreError::Invalid(message)) if message.contains("publish_observation_chunk"))
    );
    assert_eq!(project.manifest().unwrap(), before);
    assert!(project.list_observation_chunks().unwrap().is_empty());
    assert!(
        !root
            .join("artifacts")
            .join(content_hash(b"not a parquet chunk"))
            .exists()
    );
}

#[test]
fn cancellation_after_durable_artifact_keeps_orphan_invisible() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let before = project.manifest().unwrap();
    let checks = AtomicUsize::new(0);
    let result = project.publish_observation_chunk_with_cancel(
        &[observation(16, 16, 16)],
        ObservationChunkProvenance::new("fixture:post-artifact-cancel").unwrap(),
        16,
        || checks.fetch_add(1, Ordering::SeqCst) >= 4,
    );
    assert!(matches!(result, Err(StoreError::Cancelled)));
    assert_eq!(checks.load(Ordering::SeqCst), 5);
    assert_eq!(project.manifest().unwrap(), before);
    assert!(project.list_observation_chunks().unwrap().is_empty());
    let artifacts = fs::read_dir(root.join("artifacts")).unwrap().count();
    assert_eq!(
        artifacts, 1,
        "the durable orphan is retained for explicit GC"
    );
}

#[test]
fn projection_is_stale_after_precommit_rollback_and_reopen_uses_sqlite_authority() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let before = project.manifest().unwrap();
    let descriptor = project
        .publish_observation_chunk(&[observation(17, 17, 17)], "fixture:projection-window", 10)
        .unwrap();
    let projected = fs::read(root.join("manifest.json")).unwrap();
    assert!(
        projected
            .windows(descriptor.hash().len())
            .any(|window| { window == descriptor.hash().as_bytes() })
    );

    // Emulate the SQLite rollback that would follow a process crash after the
    // projection rename and before transaction commit. The orphaned artifact
    // and projected manifest remain on disk, while SQLite returns to its last
    // committed state and remains authoritative after reopen.
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute("DELETE FROM observation_chunk_members", [])
        .unwrap();
    db.execute("DELETE FROM observation_chunks", []).unwrap();
    let before_bytes = serde_json::to_vec_pretty(&before).unwrap();
    db.execute(
        "UPDATE bundle_manifest SET revision=?1, body=?2 WHERE singleton=1",
        (i64::try_from(before.revision).unwrap(), before_bytes),
    )
    .unwrap();
    drop(db);
    drop(project);

    let reopened = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(reopened.manifest().unwrap(), before);
    assert!(reopened.list_observation_chunks().unwrap().is_empty());
    let verification = reopened.verify().unwrap();
    assert!(!verification.projection_current);
    assert!(
        verification
            .failures
            .iter()
            .any(|failure| failure.contains("manifest.json projection is stale"))
    );
}

#[test]
fn mixed_known_and_unknown_evidence_is_typed_and_lossless() {
    let mut known_data = observation(14, 14, 14).into_data();
    if let ObservationPayload::Scan(scan) = &mut known_data.payload {
        scan.identity.link_id = Evidence::Known(7);
        scan.ssid = Evidence::Known(Ssid::new(vec![0, 0xff, b'k']).unwrap());
        scan.signal.noise_dbm = Evidence::Known(Dbm::new(-91.5).unwrap());
        scan.signal.chains.push(ChainSignal {
            chain_index: 1,
            rssi_dbm: unknown(UnknownReason::NotObservable),
            noise_dbm: Evidence::Known(Dbm::new(-92.0).unwrap()),
        });
        scan.signal.chains.push(ChainSignal {
            chain_index: 2,
            rssi_dbm: Evidence::Known(Dbm::new(-57.0).unwrap()),
            noise_dbm: unknown(UnknownReason::NotObservable),
        });
    }
    let known = ObservationEnvelope::new(known_data).unwrap();
    let unknown = observation(15, 15, 15);
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let descriptor = project
        .publish_observation_chunk(
            &[unknown.clone(), known.clone()],
            "fixture:mixed-evidence",
            15,
        )
        .unwrap();
    assert_eq!(
        project.read_observation_chunk(descriptor.hash()).unwrap(),
        vec![known, unknown]
    );
}

#[test]
fn unknown_wall_clock_bounds_are_nullable_and_mixed_bounds_remain_exact() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let mut unknown_data = observation(7, 0, 70).into_data();
    unknown_data.time.wall = unknown(UnknownReason::ClockUnavailable);
    let unknown_only = ObservationEnvelope::new(unknown_data).unwrap();
    let unknown_descriptor = project
        .publish_observation_chunk(&[unknown_only], "fixture:unknown-wall", 10)
        .unwrap();
    assert_eq!(unknown_descriptor.known_utc_count(), 0);
    assert_eq!(unknown_descriptor.first_utc_ns(), None);
    assert_eq!(unknown_descriptor.last_utc_ns(), None);

    let mut mixed_data = observation(8, 123, 80).into_data();
    mixed_data.time.wall = unknown(UnknownReason::ClockUnavailable);
    let mixed_unknown = ObservationEnvelope::new(mixed_data).unwrap();
    let mixed_known = observation(9, i64::MIN, 90);
    let mixed_descriptor = project
        .publish_observation_chunk(&[mixed_unknown, mixed_known], "fixture:mixed-wall", 11)
        .unwrap();
    assert_eq!(mixed_descriptor.known_utc_count(), 1);
    assert_eq!(mixed_descriptor.first_utc_ns(), Some(i64::MIN));
    assert_eq!(mixed_descriptor.last_utc_ns(), Some(i64::MIN));
    assert!(project.verify().unwrap().failures.is_empty());
}

#[test]
fn descriptor_and_provenance_deserialization_recheck_invariants() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let descriptor = project
        .publish_observation_chunk(&[observation(10, 10, 10)], "fixture:serde", 10)
        .unwrap();
    let mut wire = serde_json::to_value(&descriptor).unwrap();
    wire["provenance_id"] = serde_json::Value::String(String::new());
    assert!(
        serde_json::from_value::<kyberia_project_store::ObservationChunkDescriptor>(wire).is_err()
    );
    let provenance = serde_json::json!({ "provenance_id": "" });
    assert!(serde_json::from_value::<ObservationChunkProvenance>(provenance).is_err());
}

#[test]
#[ignore = "bounded throughput benchmark; run explicitly for performance evidence"]
fn bounded_parquet_throughput_smoke() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let input = (1..=255)
        .map(|id| observation(id, i64::from(id), u64::from(id)))
        .collect::<Vec<_>>();
    let start = Instant::now();
    let descriptor = project
        .publish_observation_chunk(&input, "benchmark:bounded", 255)
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(descriptor.row_count(), 255);
    assert!(
        elapsed.as_secs() < 30,
        "bounded benchmark exceeded 30 seconds"
    );
}

#[test]
fn duplicate_ids_roll_back_whole_publication_and_idempotent_retry_is_stable() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let mut project = bundle(&root);
    let first = observation(1, 10, 10);
    let second = observation(2, 20, 20);
    let descriptor = project
        .publish_observation_chunk(
            std::slice::from_ref(&first),
            ObservationChunkProvenance::new("fixture:duplicate-v1").unwrap(),
            2,
        )
        .unwrap();
    let before = project.manifest().unwrap();
    assert!(
        project
            .publish_observation_chunk(
                &[second.clone(), first.clone()],
                ObservationChunkProvenance::new("fixture:duplicate-v1").unwrap(),
                3,
            )
            .is_err()
    );
    assert_eq!(project.manifest().unwrap(), before);
    assert_eq!(project.read_observations().unwrap(), vec![first.clone()]);
    assert_eq!(
        project
            .publish_observation_chunk(
                &[first],
                ObservationChunkProvenance::new("fixture:duplicate-v1").unwrap(),
                3,
            )
            .unwrap(),
        descriptor
    );
}

#[test]
fn cancellation_and_corruption_are_fail_closed_and_artifacts_stay_invisible() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let mut project = bundle(&root);
    let input = vec![observation(1, 1, 1)];
    assert!(matches!(
        project.publish_observation_chunk_with_cancel(
            &input,
            ObservationChunkProvenance::new("fixture:cancelled").unwrap(),
            2,
            || true,
        ),
        Err(StoreError::Cancelled)
    ));
    assert!(project.list_observation_chunks().unwrap().is_empty());
    let descriptor = project
        .publish_observation_chunk(
            &input,
            ObservationChunkProvenance::new("fixture:corruption").unwrap(),
            2,
        )
        .unwrap();
    fs::write(root.join("artifacts").join(descriptor.hash()), b"corrupt").unwrap();
    assert!(project.read_observation_chunk(descriptor.hash()).is_err());
    assert!(!project.verify().unwrap().failures.is_empty());
}

#[test]
fn unsupported_media_and_missing_index_are_rejected() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let mut project = bundle(&root);
    let descriptor = project
        .publish_observation_chunk(
            &[observation(1, 1, 1)],
            ObservationChunkProvenance::new("fixture:index").unwrap(),
            2,
        )
        .unwrap();
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute(
        "DELETE FROM observation_chunk_members WHERE chunk_hash=?1",
        [descriptor.hash()],
    )
    .unwrap();
    assert!(project.read_observation_chunk(descriptor.hash()).is_err());
    assert!(project.list_observation_chunks().is_err());
}

#[test]
fn future_and_oversized_chunk_metadata_fail_closed_before_decode() {
    let future_root = tempfile::tempdir().unwrap().keep().join("future");
    let mut future = bundle(&future_root);
    let future_descriptor = future
        .publish_observation_chunk(&[observation(18, 18, 18)], "fixture:future", 10)
        .unwrap();
    let future_db = rusqlite::Connection::open(future_root.join("project.sqlite")).unwrap();
    future_db
        .execute(
            "UPDATE observation_chunks SET schema_version=99 WHERE chunk_hash=?1",
            [future_descriptor.hash()],
        )
        .unwrap();
    assert!(matches!(
        future.list_observation_chunks(),
        Err(StoreError::UnsupportedChunkVersion(99))
    ));

    let oversized_root = tempfile::tempdir().unwrap().keep().join("oversized");
    let mut oversized = bundle(&oversized_root);
    let oversized_descriptor = oversized
        .publish_observation_chunk(&[observation(19, 19, 19)], "fixture:oversized", 10)
        .unwrap();
    let oversized_db = rusqlite::Connection::open(oversized_root.join("project.sqlite")).unwrap();
    oversized_db
        .execute(
            "UPDATE observation_chunks SET bytes=?1 WHERE chunk_hash=?2",
            (
                i64::try_from(MAX_OBSERVATION_CHUNK_BYTES + 1).unwrap(),
                oversized_descriptor.hash(),
            ),
        )
        .unwrap();
    assert!(matches!(
        oversized.list_observation_chunks(),
        Err(StoreError::Corrupt(message)) if message.contains("byte length")
    ));
}

#[test]
fn observation_chunk_hash_traversal_is_rejected_before_filesystem_access() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let project = bundle(&root);
    assert!(matches!(
        project.read_observation_chunk("../outside"),
        Err(StoreError::Invalid(message)) if message.contains("SHA-256")
    ));
}

#[cfg(unix)]
#[test]
fn observation_chunk_symlink_is_rejected_before_reading_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let descriptor = project
        .publish_observation_chunk(&[observation(20, 20, 20)], "fixture:symlink", 10)
        .unwrap();
    let path = root.join("artifacts").join(descriptor.hash());
    fs::remove_file(&path).unwrap();
    symlink("/etc/passwd", &path).unwrap();
    assert!(project.list_observation_chunks().is_err());
}

#[test]
fn indexed_observation_selection_is_sorted_and_reopens_exactly() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let mut project = bundle(&root);
    let input = vec![
        observation(3, 30, 30),
        observation(1, 10, 10),
        observation(2, 20, 20),
    ];
    project
        .publish_observation_chunk(&input, "fixture:indexed-query", 30)
        .unwrap();
    let selected = project
        .read_observations_by_id(&[
            ObservationId::from_bytes([3; 16]).unwrap(),
            ObservationId::from_bytes([1; 16]).unwrap(),
        ])
        .unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|observation| observation.data().id)
            .collect::<Vec<_>>(),
        vec![
            ObservationId::from_bytes([1; 16]).unwrap(),
            ObservationId::from_bytes([3; 16]).unwrap(),
        ]
    );
    drop(project);
    let reopened = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(
        reopened
            .read_observations_by_id(&[
                ObservationId::from_bytes([3; 16]).unwrap(),
                ObservationId::from_bytes([1; 16]).unwrap(),
            ])
            .unwrap(),
        selected
    );
}

#[test]
fn indexed_selection_receipt_is_stable_and_lists_exact_verified_chunks() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let first = project
        .publish_observation_chunk(
            &[observation(1, 1, 1), observation(2, 2, 2)],
            "fixture:receipt-first",
            2,
        )
        .unwrap();
    let second = project
        .publish_observation_chunk(&[observation(3, 3, 3)], "fixture:receipt-second", 3)
        .unwrap();
    let unselected = project
        .publish_observation_chunk(&[observation(4, 4, 4)], "fixture:receipt-unselected", 4)
        .unwrap();
    fs::write(root.join("artifacts").join(unselected.hash()), b"corrupt").unwrap();
    let mut expected_chunks = vec![first, second];
    expected_chunks.sort_by(|left, right| left.hash().cmp(right.hash()));
    let ids = vec![
        ObservationId::from_bytes([3; 16]).unwrap(),
        ObservationId::from_bytes([1; 16]).unwrap(),
    ];
    let result = project.read_observation_selection_by_id(&ids).unwrap();
    let repeat = project.read_observation_selection_by_id(&ids).unwrap();
    assert_eq!(result, repeat);
    assert_eq!(
        project.read_observations_by_id(&ids).unwrap(),
        result.observations()
    );
    assert_eq!(result.receipt().project_revision(), 3);
    assert_eq!(result.receipt().selected_chunks(), expected_chunks);
    assert_eq!(
        result
            .observations()
            .iter()
            .map(|observation| observation.data().id)
            .collect::<Vec<_>>(),
        vec![
            ObservationId::from_bytes([1; 16]).unwrap(),
            ObservationId::from_bytes([3; 16]).unwrap(),
        ]
    );
    let empty = project.read_observation_selection_by_id(&[]).unwrap();
    assert!(empty.observations().is_empty());
    assert!(empty.receipt().selected_chunks().is_empty());
    assert_eq!(empty.receipt().project_revision(), 3);
}

#[test]
fn indexed_selection_rejects_a_project_revision_change_during_verification() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    project
        .publish_observation_chunk(&[observation(1, 1, 1)], "fixture:receipt-revision", 2)
        .unwrap();
    let writer_root = root.clone();
    let checks = AtomicUsize::new(0);
    let bumped = AtomicUsize::new(0);
    let bump_revision = || {
        let check = checks.fetch_add(1, Ordering::SeqCst);
        if check == 3 {
            let mut writer = Bundle::open(&writer_root, OpenMode::ReadWrite).unwrap();
            writer
                .put_artifact(
                    &[7, 8, 9],
                    ArtifactEntry {
                        kind: ArtifactKind::Annotation,
                        bytes: 3,
                        media_type: "application/octet-stream".into(),
                        provenance_id: "fixture:revision-bump".into(),
                    },
                    3,
                )
                .unwrap();
            bumped.store(1, Ordering::SeqCst);
        }
        false
    };
    let result = project.read_observation_selection_by_id_with_cancel(
        &[ObservationId::from_bytes([1; 16]).unwrap()],
        &bump_revision,
    );
    assert!(matches!(
        result,
        Err(StoreError::Invalid(message)) if message.contains("project revision changed")
    ));
    assert_eq!(bumped.load(Ordering::SeqCst), 1);
}

#[test]
fn indexed_observation_selection_reports_missing_duplicate_and_cancelled_requests() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    project
        .publish_observation_chunk(&[observation(1, 1, 1)], "fixture:indexed-errors", 2)
        .unwrap();
    let missing = ObservationId::from_bytes([99; 16]).unwrap();
    assert!(matches!(
        project.read_observations_by_id(&[
            ObservationId::from_bytes([1; 16]).unwrap(),
            missing,
        ]),
        Err(StoreError::Invalid(message)) if message.contains("missing requested IDs") && message.contains(&String::from(missing))
    ));
    assert!(matches!(
        project.read_observations_by_id(&[
            ObservationId::from_bytes([1; 16]).unwrap(),
            ObservationId::from_bytes([1; 16]).unwrap(),
        ]),
        Err(StoreError::Invalid(message)) if message.contains("duplicate ID")
    ));
    assert!(matches!(
        project.read_observations_by_id_with_cancel(
            &[ObservationId::from_bytes([1; 16]).unwrap()],
            &|| true,
        ),
        Err(StoreError::Cancelled)
    ));
    assert!(matches!(
        project.read_observations_by_id_with_cancel(&[], &|| true),
        Err(StoreError::Cancelled)
    ));
    let final_checks = AtomicUsize::new(0);
    let cancel_after_final_manifest = || final_checks.fetch_add(1, Ordering::SeqCst) >= 4;
    assert!(matches!(
        project.read_observation_selection_by_id_with_cancel(
            &[ObservationId::from_bytes([1; 16]).unwrap()],
            &cancel_after_final_manifest,
        ),
        Err(StoreError::Cancelled)
    ));
    let empty_checks = AtomicUsize::new(0);
    let cancel_after_empty_manifest = || empty_checks.fetch_add(1, Ordering::SeqCst) >= 1;
    assert!(matches!(
        project.read_observation_selection_by_id_with_cancel(&[], &cancel_after_empty_manifest),
        Err(StoreError::Cancelled)
    ));
}

#[test]
fn indexed_query_cancellation_after_a_selected_decode_never_returns_partial() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    project
        .publish_observation_chunk(&[observation(1, 1, 1)], "fixture:cancel-one", 2)
        .unwrap();
    project
        .publish_observation_chunk(&[observation(2, 2, 2)], "fixture:cancel-two", 2)
        .unwrap();
    let checks = AtomicUsize::new(0);
    let cancel_after_first_decode = || checks.fetch_add(1, Ordering::SeqCst) >= 5;
    let result = project.read_observations_by_id_with_cancel(
        &[
            ObservationId::from_bytes([1; 16]).unwrap(),
            ObservationId::from_bytes([2; 16]).unwrap(),
        ],
        &cancel_after_first_decode,
    );
    assert!(matches!(result, Err(StoreError::Cancelled)));
    assert!(checks.load(Ordering::SeqCst) >= 5);
}

#[test]
fn selected_index_and_provenance_tampering_fail_closed() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let _descriptor = project
        .publish_observation_chunk(
            &[observation(1, 1, 1), observation(2, 2, 2)],
            "fixture:indexed-tamper",
            2,
        )
        .unwrap();
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE observation_chunk_members SET ordinal=1 WHERE observation_id=?1",
        [String::from(ObservationId::from_bytes([1; 16]).unwrap())],
    )
    .unwrap();
    assert!(matches!(
        project.read_observations_by_id(&[ObservationId::from_bytes([1; 16]).unwrap()]),
        Err(StoreError::Corrupt(_))
    ));
    drop(db);

    let root = tempfile::tempdir().unwrap().keep().join("provenance");
    let mut project = bundle(&root);
    let descriptor = project
        .publish_observation_chunk(&[observation(1, 1, 1)], "fixture:indexed-provenance", 2)
        .unwrap();
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE observation_chunks SET provenance_id=?1 WHERE chunk_hash=?2",
        ("tampered-provenance", descriptor.hash()),
    )
    .unwrap();
    assert!(matches!(
        project.read_observations_by_id(&[ObservationId::from_bytes([1; 16]).unwrap()]),
        Err(StoreError::Corrupt(message)) if message.contains("manifest entry mismatch")
    ));
}

#[test]
fn selected_future_publication_revision_is_rejected() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let descriptor = project
        .publish_observation_chunk(&[observation(1, 1, 1)], "fixture:future-revision", 2)
        .unwrap();
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE observation_chunks SET revision=revision+1 WHERE chunk_hash=?1",
        [descriptor.hash()],
    )
    .unwrap();
    assert!(matches!(
        project.read_observations_by_id(&[ObservationId::from_bytes([1; 16]).unwrap()]),
        Err(StoreError::Corrupt(message)) if message.contains("ahead of the manifest")
    ));
}

#[test]
fn indexed_query_ignores_unrelated_corrupt_chunks_at_scale() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let selected = project
        .publish_observation_chunk(&[observation(1, 1, 1)], "fixture:selected", 2)
        .unwrap();
    let mut unrelated = Vec::new();
    for id in 2..=129_u8 {
        unrelated.push(
            project
                .publish_observation_chunk(
                    &[observation(id, i64::from(id), u64::from(id))],
                    format!("fixture:unrelated-{id}"),
                    i64::from(id),
                )
                .unwrap(),
        );
    }
    for descriptor in unrelated {
        fs::write(root.join("artifacts").join(descriptor.hash()), b"corrupt").unwrap();
    }
    let result = project
        .read_observations_by_id(&[ObservationId::from_bytes([1; 16]).unwrap()])
        .unwrap();
    assert_eq!(result, vec![observation(1, 1, 1)]);
    assert!(project.read_observation_chunk(selected.hash()).is_err());
}

#[test]
fn indexed_query_rejects_selection_and_selected_decode_budgets() {
    let root = tempfile::tempdir().unwrap().keep().join("project");
    let mut project = bundle(&root);
    let descriptor = project
        .publish_observation_chunk(&[observation(1, 1, 1)], "fixture:query-budget", 2)
        .unwrap();
    let too_many = (0..=MAX_OBSERVATION_QUERY_IDS)
        .map(|value| ObservationId::from_bytes((value as u128 + 1).to_be_bytes()).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        project.read_observations_by_id(&too_many),
        Err(StoreError::Invalid(message)) if message.contains("selection limit")
    ));
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE observation_chunks SET bytes=?1 WHERE chunk_hash=?2",
        (
            i64::try_from(MAX_OBSERVATION_CHUNK_BYTES + 1).unwrap(),
            descriptor.hash(),
        ),
    )
    .unwrap();
    assert!(matches!(
        project.read_observations_by_id(&[ObservationId::from_bytes([1; 16]).unwrap()]),
        Err(StoreError::Corrupt(message)) if message.contains("byte length")
    ));

    let rows_root = tempfile::tempdir().unwrap().keep().join("rows");
    let mut rows_project = bundle(&rows_root);
    let mut row_ids = Vec::new();
    let mut row_descriptors = Vec::new();
    for id in 1..=5_u8 {
        row_ids.push(ObservationId::from_bytes([id; 16]).unwrap());
        row_descriptors.push(
            rows_project
                .publish_observation_chunk(
                    &[observation(id, i64::from(id), u64::from(id))],
                    format!("fixture:query-rows-{id}"),
                    i64::from(id),
                )
                .unwrap(),
        );
    }
    let rows_db = rusqlite::Connection::open(rows_root.join("project.sqlite")).unwrap();
    for descriptor in &row_descriptors {
        rows_db
            .execute(
                "UPDATE observation_chunks SET row_count=?1 WHERE chunk_hash=?2",
                (MAX_OBSERVATION_CHUNK_ROWS as i64, descriptor.hash()),
            )
            .unwrap();
    }
    assert!(matches!(
        rows_project.read_observations_by_id(&row_ids),
        Err(StoreError::Invalid(message)) if message.contains("decode budget")
    ));

    let bytes_root = tempfile::tempdir().unwrap().keep().join("bytes");
    let mut bytes_project = bundle(&bytes_root);
    let mut byte_ids = Vec::new();
    let mut byte_descriptors = Vec::new();
    for id in 1..=2_u8 {
        byte_ids.push(ObservationId::from_bytes([id; 16]).unwrap());
        byte_descriptors.push(
            bytes_project
                .publish_observation_chunk(
                    &[observation(id, i64::from(id), u64::from(id))],
                    format!("fixture:query-bytes-{id}"),
                    i64::from(id),
                )
                .unwrap(),
        );
    }
    let bytes_db = rusqlite::Connection::open(bytes_root.join("project.sqlite")).unwrap();
    for descriptor in &byte_descriptors {
        bytes_db
            .execute(
                "UPDATE observation_chunks SET bytes=?1 WHERE chunk_hash=?2",
                (
                    i64::try_from(MAX_OBSERVATION_CHUNK_BYTES).unwrap(),
                    descriptor.hash(),
                ),
            )
            .unwrap();
    }
    assert!(matches!(
        bytes_project.read_observations_by_id(&byte_ids),
        Err(StoreError::Invalid(message)) if message.contains("selected byte budget")
    ));

    fn observation_with_query_id(value: u16) -> ObservationEnvelope {
        let mut data = observation(1, i64::from(value), u64::from(value)).into_data();
        data.id = ObservationId::from_bytes(u128::from(value).to_be_bytes()).unwrap();
        ObservationEnvelope::new(data).unwrap()
    }

    let chunks_root = tempfile::tempdir().unwrap().keep().join("chunks");
    let mut chunks_project = bundle(&chunks_root);
    let mut chunk_ids = Vec::new();
    for value in 1..=129_u16 {
        let envelope = observation_with_query_id(value);
        chunk_ids.push(envelope.data().id);
        chunks_project
            .publish_observation_chunk(
                &[envelope],
                format!("fixture:query-chunks-{value}"),
                i64::from(value),
            )
            .unwrap();
    }
    assert!(matches!(
        chunks_project.read_observations_by_id(&chunk_ids),
        Err(StoreError::Invalid(message)) if message.contains("selected chunk limit")
    ));
}

#[test]
fn old_bundle_gets_empty_chunk_schema_only_on_writable_open() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let project = bundle(&root);
    drop(project);
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute_batch("DROP TABLE observation_chunk_members; DROP TABLE observation_chunks;")
        .unwrap();
    drop(db);
    let reader = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert!(reader.list_observation_chunks().unwrap().is_empty());
    drop(reader);
    let writer = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
    assert!(writer.list_observation_chunks().unwrap().is_empty());
}
