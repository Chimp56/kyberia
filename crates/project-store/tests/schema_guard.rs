use kyberia_domain::identity::ProjectId;
use kyberia_project_store::{ArtifactEntry, ArtifactKind, Bundle, OpenMode};
use rusqlite::Connection;
use std::{fs, path::PathBuf};

fn fixture() -> (PathBuf, Bundle) {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let bundle = Bundle::create(
        &root,
        ProjectId::from_bytes([1; 16]).unwrap(),
        "Schema guard".into(),
        1,
    )
    .unwrap();
    (root, bundle)
}
fn entry() -> ArtifactEntry {
    ArtifactEntry {
        kind: ArtifactKind::MapSource,
        bytes: 8,
        media_type: "image/svg+xml".into(),
        provenance_id: "test:original-schema-guard".into(),
    }
}

#[test]
fn imported_trigger_cannot_suppress_successful_registration() {
    for trigger in [
        "CREATE TRIGGER hostile BEFORE UPDATE ON bundle_manifest BEGIN SELECT RAISE(IGNORE); END;",
        "CREATE TRIGGER hostile AFTER UPDATE ON bundle_manifest BEGIN UPDATE bundle_manifest SET revision=0; END;",
    ] {
        let (root, bundle) = fixture();
        drop(bundle);
        let db = Connection::open(root.join("project.sqlite")).unwrap();
        db.execute_batch(trigger).unwrap();
        drop(db);
        assert!(
            Bundle::open(&root, OpenMode::ReadWrite).is_err(),
            "hostile trigger accepted"
        );
        assert!(Bundle::open(&root, OpenMode::ReadOnly).is_err());
    }
}

#[test]
fn stale_handle_revalidates_schema_before_write_or_recovery() {
    let (root, mut bundle) = fixture();
    let projection = fs::read(root.join("manifest.json")).unwrap();
    let db = Connection::open(root.join("project.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER hostile BEFORE UPDATE ON bundle_manifest BEGIN SELECT RAISE(IGNORE); END;",
    )
    .unwrap();
    assert!(bundle.put_artifact(b"evidence", entry(), 2).is_err());
    assert!(bundle.recover_manifest().is_err());
    assert_eq!(fs::read(root.join("manifest.json")).unwrap(), projection);
    let revision: i64 = db
        .query_row("SELECT revision FROM bundle_manifest", [], |r| r.get(0))
        .unwrap();
    assert_eq!(revision, 0);
}

#[test]
fn imported_recursive_view_is_rejected_before_manifest_query() {
    let (root, bundle) = fixture();
    drop(bundle);
    let db = Connection::open(root.join("project.sqlite")).unwrap();
    db.execute_batch("ALTER TABLE bundle_manifest RENAME TO hidden_manifest; CREATE VIEW bundle_manifest AS WITH RECURSIVE count(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM count WHERE n<100000) SELECT singleton,revision,body FROM hidden_manifest WHERE (SELECT max(n) FROM count)=100000;").unwrap();
    assert!(Bundle::open(&root, OpenMode::ReadOnly).is_err());
}

#[test]
fn unexpected_objects_and_noncanonical_table_shapes_are_rejected() {
    for sql in [
        "CREATE TABLE unexpected(x)",
        "CREATE INDEX unexpected ON bundle_manifest(revision)",
        "CREATE VIEW unexpected AS SELECT 1",
        "ALTER TABLE bundle_manifest ADD COLUMN unexpected TEXT",
        "ALTER TABLE bundle_manifest RENAME TO old; CREATE TABLE bundle_manifest(singleton,revision,body); INSERT INTO bundle_manifest SELECT * FROM old; DROP TABLE old;",
        "ALTER TABLE bundle_manifest RENAME TO old; CREATE TABLE bundle_manifest(singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL CHECK(revision>=0), body BLOB NOT NULL, x GENERATED ALWAYS AS (length(body)) VIRTUAL);",
    ] {
        let (root, bundle) = fixture();
        drop(bundle);
        let db = Connection::open(root.join("project.sqlite")).unwrap();
        db.execute_batch(sql).unwrap();
        assert!(
            Bundle::open(&root, OpenMode::ReadOnly).is_err(),
            "accepted: {sql}"
        );
    }
}

#[test]
fn manifest_inventory_requires_exactly_one_singleton_row() {
    for mutation in [
        "DELETE FROM bundle_manifest",
        "PRAGMA ignore_check_constraints=ON; INSERT INTO bundle_manifest SELECT 2,revision,body FROM bundle_manifest WHERE singleton=1",
        "PRAGMA ignore_check_constraints=ON; UPDATE bundle_manifest SET singleton=2",
    ] {
        let (root, mut bundle) = fixture();
        let projection = fs::read(root.join("manifest.json")).unwrap();
        let db = Connection::open(root.join("project.sqlite")).unwrap();
        db.execute_batch(mutation).unwrap();
        assert!(
            bundle.manifest().is_err(),
            "invalid singleton inventory accepted: {mutation}"
        );
        assert!(bundle.put_artifact(b"evidence", entry(), 2).is_err());
        assert!(bundle.recover_manifest().is_err());
        assert_eq!(fs::read(root.join("manifest.json")).unwrap(), projection);
    }
}

#[test]
fn oversized_sparse_database_is_rejected_without_reading_its_contents() {
    let (root, bundle) = fixture();
    drop(bundle);
    fs::OpenOptions::new()
        .write(true)
        .open(root.join("project.sqlite"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    assert!(Bundle::open(&root, OpenMode::ReadOnly).is_err());
}

#[test]
fn valid_schema_preserves_commit_reopen_duplicate_and_recovery() {
    let (root, mut bundle) = fixture();
    let hash = bundle.put_artifact(b"evidence", entry(), 2).unwrap();
    assert_eq!(bundle.manifest().unwrap().revision, 1);
    assert_eq!(bundle.put_artifact(b"evidence", entry(), 3).unwrap(), hash);
    assert_eq!(bundle.manifest().unwrap().revision, 1);
    drop(bundle);
    let reopened = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
    assert_eq!(reopened.read_artifact(&hash).unwrap(), b"evidence");
    fs::write(root.join("manifest.json"), b"bad projection").unwrap();
    reopened.recover_manifest().unwrap();
    assert!(reopened.verify().unwrap().failures.is_empty());
}
