use std::process::Command;

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
