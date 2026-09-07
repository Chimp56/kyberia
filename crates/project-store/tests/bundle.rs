use kyberia_domain::identity::ProjectId;
use kyberia_project_store::{ArtifactEntry, ArtifactKind, Bundle, OpenMode, StoreError};
use std::fs;

fn id() -> ProjectId {
    ProjectId::from_bytes([1; 16]).unwrap()
}
fn entry(bytes: &[u8]) -> ArtifactEntry {
    ArtifactEntry {
        kind: ArtifactKind::MapSource,
        bytes: bytes.len() as u64,
        media_type: "image/svg+xml".into(),
        provenance_id: "fixture:original-empty-map-v1".into(),
    }
}

#[test]
fn create_reopen_and_export_preserve_identity_and_artifact() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("home.rfatlas");
    let mut project = Bundle::create(&path, id(), "Home".into(), 10).unwrap();
    let map = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
    let hash = project.put_artifact(map, entry(map), 20).unwrap();
    let manifest = project.manifest().unwrap();
    assert_eq!(manifest.project_id, id());
    assert_eq!(manifest.revision, 1);
    assert!(project.verify().unwrap().failures.is_empty());
    drop(project);
    let mut project = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(project.manifest().unwrap(), manifest);
    assert_eq!(project.read_artifact(&hash).unwrap(), map);
    assert!(matches!(
        project.put_artifact(b"no", entry(b"no"), 30),
        Err(StoreError::ReadOnly)
    ));
    assert!(matches!(
        project.recover_manifest(),
        Err(StoreError::ReadOnly)
    ));
}

#[test]
fn create_does_not_overwrite_and_invalid_input_leaves_no_directory() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    assert!(Bundle::create(&path, id(), "\n".into(), 1).is_err());
    assert!(!path.exists());
    let project = Bundle::create(&path, id(), "Initial".into(), 1).unwrap();
    assert!(Bundle::create(&path, id(), "Replacement".into(), 2).is_err());
    assert_eq!(project.manifest().unwrap().name, "Initial");
}

#[test]
fn duplicate_import_does_not_advance_revision_or_duplicate_bytes() {
    let dir = tempfile::tempdir().unwrap().keep();
    let mut project =
        Bundle::create(&dir.as_path().join("project"), id(), "Test".into(), 1).unwrap();
    let hash = project.put_artifact(b"map", entry(b"map"), 2).unwrap();
    assert_eq!(
        hash,
        project.put_artifact(b"map", entry(b"map"), 3).unwrap()
    );
    assert_eq!(project.manifest().unwrap().revision, 1);
    assert_eq!(project.verify().unwrap().artifact_count, 1);
}

#[test]
fn checksum_corruption_and_missing_blobs_never_verify_as_success() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    let mut project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    let hash = project.put_artifact(b"map", entry(b"map"), 2).unwrap();
    fs::write(path.join("artifacts").join(&hash), b"bad").unwrap();
    assert!(project.read_artifact(&hash).is_err());
    assert_eq!(project.verify().unwrap().failures.len(), 1);
    fs::remove_file(path.join("artifacts").join(&hash)).unwrap();
    assert_eq!(project.verify().unwrap().failures.len(), 1);
}

#[test]
fn stale_projection_is_diagnosed_and_explicitly_recovered() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    let mut project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    let before = fs::read(path.join("manifest.json")).unwrap();
    project.put_artifact(b"map", entry(b"map"), 2).unwrap();
    fs::write(path.join("manifest.json"), before).unwrap();
    drop(project);
    let project = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(project.manifest().unwrap().revision, 1);
    assert!(!project.verify().unwrap().projection_current);
    drop(project);
    let project = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    project.recover_manifest().unwrap();
    assert!(project.verify().unwrap().failures.is_empty());
}

#[test]
fn malformed_manifest_and_traversal_are_rejected() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    let project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    assert!(project.read_artifact("../../etc/passwd").is_err());
    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    db.execute("UPDATE bundle_manifest SET body=?1", [b"not-json".to_vec()])
        .unwrap();
    assert!(Bundle::open(&path, OpenMode::ReadOnly).is_err());
}

#[test]
fn future_schema_is_read_only_when_compatible_metadata_is_available() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    let project = Bundle::create(&path, id(), "Future".into(), 1).unwrap();
    let mut manifest = project.manifest().unwrap();
    manifest.schema_version = 2;
    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE bundle_manifest SET body=?1",
        [serde_json::to_vec(&manifest).unwrap()],
    )
    .unwrap();
    db.execute_batch("PRAGMA user_version=2").unwrap();
    assert!(matches!(
        Bundle::open(&path, OpenMode::ReadWrite),
        Err(StoreError::UnsupportedVersion(2))
    ));
    let reader = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(reader.manifest().unwrap().name, "Future");
}

#[test]
fn malformed_artifact_declaration_cannot_mutate_committed_inventory() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    let mut project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    let before = project.manifest().unwrap();
    let mut bad = entry(b"wrong-length");
    bad.bytes = 1;
    assert!(project.put_artifact(b"wrong-length", bad, 2).is_err());
    assert_eq!(project.manifest().unwrap(), before);
    assert!(project.put_artifact(b"map", entry(b"map"), 0).is_err());
    assert_eq!(project.manifest().unwrap(), before);
}

#[cfg(unix)]
#[test]
fn symlink_assets_cannot_read_or_overwrite_outside_bundle() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    let mut project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    let hash = project.put_artifact(b"map", entry(b"map"), 2).unwrap();
    let outside = dir.as_path().join("outside");
    fs::write(&outside, b"map").unwrap();
    fs::remove_file(path.join("artifacts").join(&hash)).unwrap();
    symlink(&outside, path.join("artifacts").join(&hash)).unwrap();
    assert!(project.read_artifact(&hash).is_err());
    assert!(project.put_artifact(b"map", entry(b"map"), 2).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"map");
}

#[test]
fn two_writers_read_latest_committed_inventory() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.as_path().join("project");
    let mut first = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    let mut second = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    first.put_artifact(b"one", entry(b"one"), 2).unwrap();
    second.put_artifact(b"two", entry(b"two"), 3).unwrap();
    assert_eq!(first.manifest().unwrap().artifacts.len(), 2);
    assert_eq!(second.manifest().unwrap().revision, 2);
}

#[test]
fn stale_handle_rejects_corrupted_revision_and_schema_before_commit() {
    for corruption in [
        "UPDATE bundle_manifest SET revision=99",
        "PRAGMA user_version=99",
    ] {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("project");
        let mut project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
        let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
        db.execute_batch(corruption).unwrap();
        let before: Vec<u8> = db
            .query_row("SELECT body FROM bundle_manifest", [], |r| r.get(0))
            .unwrap();
        assert!(matches!(
            project.put_artifact(b"map", entry(b"map"), 2),
            Err(StoreError::Corrupt(_))
        ));
        assert!(project.recover_manifest().is_err());
        let after: Vec<u8> = db
            .query_row("SELECT body FROM bundle_manifest", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
    }
}

#[test]
fn projection_failure_rolls_back_artifact_registration() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    let before = project.manifest().unwrap();
    fs::rename(path.join("manifest.json"), path.join("original.json")).unwrap();
    fs::create_dir(path.join("manifest.json")).unwrap();
    assert!(project.put_artifact(b"map", entry(b"map"), 2).is_err());
    assert_eq!(project.manifest().unwrap(), before);
    drop(project);
    assert_eq!(
        Bundle::open(&path, OpenMode::ReadOnly)
            .unwrap()
            .manifest()
            .unwrap(),
        before
    );
}

#[test]
fn stale_handle_cannot_write_upgraded_compatible_manifest() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut project = Bundle::create(&path, id(), "Test".into(), 1).unwrap();
    let mut future = project.manifest().unwrap();
    future.schema_version = 2;
    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE bundle_manifest SET body=?1",
        [serde_json::to_vec(&future).unwrap()],
    )
    .unwrap();
    db.execute_batch("PRAGMA user_version=2").unwrap();
    assert!(matches!(
        project.put_artifact(b"map", entry(b"map"), 2),
        Err(StoreError::UnsupportedVersion(2))
    ));
    assert!(matches!(
        project.recover_manifest(),
        Err(StoreError::UnsupportedVersion(2))
    ));
    assert_eq!(project.manifest().unwrap(), future);
}
