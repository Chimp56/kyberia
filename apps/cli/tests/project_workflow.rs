use std::process::Command;

fn unknown<T>() -> kyberia_domain::evidence::Evidence<T> {
    kyberia_domain::evidence::Evidence::Unknown(
        kyberia_domain::evidence::UnknownReason::SourceDidNotProvide,
    )
}

fn observation(id: u8) -> kyberia_domain::observation::ObservationEnvelope {
    use kyberia_domain::{
        evidence::Evidence,
        identity::{CollectorId, ObservationId, SessionId, SourceId, Text},
        observation::{
            CaptureHealth, EnvelopeData, IdentifierPolicy, ObservationEnvelope, ObservationPayload,
            ObservationSchemaVersion, PayloadRetention, PrivacyState, SourceDescriptor, SourceKind,
        },
        time::CaptureTime,
    };
    ObservationEnvelope::new(EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes([id; 16]).unwrap(),
        session_id: SessionId::from_bytes([2; 16]).unwrap(),
        source: SourceDescriptor {
            source_id: SourceId::from_bytes([3; 16]).unwrap(),
            collector_id: CollectorId::from_bytes([4; 16]).unwrap(),
            sensor_id: unknown(),
            adapter_id: unknown(),
            kind: SourceKind::NativeApi,
            source_name: Text::new("cli export fixture").unwrap(),
            source_version: unknown(),
            source_schema_version: Text::new("fixture/1").unwrap(),
            adapter_name: Text::new("fixture").unwrap(),
            adapter_version: Text::new("fixture/1").unwrap(),
            parser_version: Text::new("fixture/1").unwrap(),
            driver_version: unknown(),
            os_version: unknown(),
        },
        time: CaptureTime {
            wall: unknown(),
            monotonic: unknown(),
            synchronization: unknown(),
        },
        pose: unknown(),
        channel: unknown(),
        dwell: unknown(),
        privacy: PrivacyState {
            policy_version: Text::new("privacy/1").unwrap(),
            identifiers: IdentifierPolicy::Redacted,
            payload: PayloadRetention::Discarded,
        },
        quality: vec![],
        raw_source: unknown(),
        payload: ObservationPayload::Health(CaptureHealth {
            dropped_events: Evidence::Known(0),
            queued_events: unknown(),
            connected: Evidence::Known(true),
            diagnostic: Text::new("healthy").unwrap(),
        }),
    })
    .unwrap()
}

#[test]
fn executable_round_trip_and_failed_verification_exit_codes() {
    let dir = tempfile::tempdir().unwrap().keep();
    let project = dir.as_path().join("cli.rfatlas");
    let binary = env!("CARGO_BIN_EXE_kyberia");
    let output = Command::new(binary)
        .args(["new", project.to_str().unwrap(), "CLI home"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let output = Command::new(binary)
        .args(["inspect", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        created
    );
    let output = Command::new(binary)
        .args(["verify", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    std::fs::write(project.join("manifest.json"), b"broken projection").unwrap();
    let output = Command::new(binary)
        .args(["verify", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let output = Command::new(binary)
        .args(["recover-manifest", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = Command::new(binary)
        .args(["new", project.to_str().unwrap(), "Overwrite"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn projection_recovery_reports_remaining_evidence_corruption() {
    use kyberia_domain::identity::ProjectId;
    use kyberia_project_store::{ArtifactEntry, ArtifactKind, Bundle};
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("corrupt.rfatlas");
    let mut bundle = Bundle::create(
        &path,
        ProjectId::from_bytes([1; 16]).unwrap(),
        "Test".into(),
        1,
    )
    .unwrap();
    let hash = bundle
        .put_artifact(
            b"map",
            ArtifactEntry {
                kind: ArtifactKind::MapSource,
                bytes: 3,
                media_type: "application/octet-stream".into(),
                provenance_id: "test:original".into(),
            },
            2,
        )
        .unwrap();
    drop(bundle);
    std::fs::write(path.join("artifacts").join(hash), b"bad").unwrap();
    std::fs::write(path.join("manifest.json"), b"broken").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kyberia"))
        .args(["recover-manifest", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["projection_current"], true);
    assert_eq!(report["failures"].as_array().unwrap().len(), 1);
}

#[test]
fn normalized_parquet_export_is_verified_exact_and_non_overwriting() {
    use kyberia_domain::identity::ProjectId;
    use kyberia_project_store::{Bundle, ObservationChunkProvenance};

    let dir = tempfile::tempdir().unwrap().keep();
    let project = dir.join("export.rfatlas");
    let destination = dir.join("normalized-export");
    let mut bundle = Bundle::create(
        &project,
        ProjectId::from_bytes([1; 16]).unwrap(),
        "Export".into(),
        1,
    )
    .unwrap();
    let first = bundle
        .publish_observation_chunk(
            &[observation(6)],
            ObservationChunkProvenance::new("cli-export-fixture/first").unwrap(),
            2,
        )
        .unwrap();
    let second = bundle
        .publish_observation_chunk(
            &[observation(5)],
            ObservationChunkProvenance::new("cli-export-fixture/second").unwrap(),
            3,
        )
        .unwrap();
    assert!(
        first.hash() > second.hash(),
        "fixture must publish descending hashes to exercise canonical export ordering"
    );
    let expected = [&first, &second]
        .into_iter()
        .map(|descriptor| {
            (
                descriptor.hash().to_owned(),
                std::fs::read(project.join("artifacts").join(descriptor.hash())).unwrap(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    drop(bundle);

    let binary = env!("CARGO_BIN_EXE_kyberia");
    let output = Command::new(binary)
        .args([
            "export-observations-parquet",
            project.to_str().unwrap(),
            destination.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "kyberia.observation-parquet-export/1");
    assert_eq!(report["chunks"], 2);
    assert_eq!(report["rows"], 2);
    assert!(
        report["privacy_warning"]
            .as_str()
            .unwrap()
            .contains("before sharing")
    );
    for (hash, bytes) in &expected {
        assert_eq!(
            std::fs::read(destination.join(format!("{hash}.parquet"))).unwrap(),
            *bytes
        );
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(destination.join("manifest.json")).unwrap()).unwrap();
    assert!(!destination.join(".manifest.json.pending").exists());
    assert_eq!(manifest["schema"], report["schema"]);
    assert_eq!(manifest["privacy_warning"], report["privacy_warning"]);
    assert_eq!(manifest["observation_schema_version"], 2);
    assert_eq!(
        manifest["parquet_schema_fingerprint"],
        kyberia_project_store::PARQUET_SCHEMA_FINGERPRINT
    );
    let hashes = manifest["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| chunk["hash"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(hashes.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(manifest["chunks"][0]["row_count"], 1);
    assert_eq!(manifest["chunks"][1]["row_count"], 1);

    let retry = Command::new(binary)
        .args([
            "export-observations-parquet",
            project.to_str().unwrap(),
            destination.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(retry.status.code(), Some(2));

    std::fs::write(project.join("artifacts").join(first.hash()), b"corrupt").unwrap();
    let corrupt_destination = dir.join("corrupt-export");
    let corrupt = Command::new(binary)
        .args([
            "export-observations-parquet",
            project.to_str().unwrap(),
            corrupt_destination.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(corrupt.status.code(), Some(2));
    assert!(!corrupt_destination.exists());
}
