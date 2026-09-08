use kyberia_domain::identity::ProjectId;
use kyberia_project_store::{ArtifactEntry, ArtifactKind, Bundle, OpenMode, StoreError};
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

fn assert_sidecar_budget_error(result: kyberia_project_store::Result<Bundle>) {
    match result {
        Err(StoreError::Invalid(message)) => assert!(
            message == "metadata database and SQLite sidecars exceed 64 MiB read budget",
            "unexpected validation error: {message}"
        ),
        Ok(_) => panic!("oversized SQLite envelope was accepted"),
        Err(error) => panic!("unexpected error before budget check: {error}"),
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
fn current_schema_contains_every_complete_optional_table_group() {
    let (root, bundle) = fixture();
    drop(bundle);
    let db = Connection::open(root.join("project.sqlite")).unwrap();
    let mut statement = db
        .prepare(
            "SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap();
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        names,
        [
            "bundle_manifest",
            "observation_chunk_members",
            "observation_chunks",
            "operation_log_state",
            "project_operations",
            "survey_snapshot_history",
            "survey_snapshots",
        ]
    );
    let operation_state: (String, i64) = db
        .query_row(
            "SELECT project_id,project_revision FROM operation_log_state WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        operation_state.0,
        String::from(ProjectId::from_bytes([1; 16]).unwrap())
    );
    assert_eq!(operation_state.1, 0);
    drop(statement);
    drop(db);
    assert!(
        Bundle::open(&root, OpenMode::ReadOnly)
            .unwrap()
            .verify()
            .unwrap()
            .failures
            .is_empty()
    );
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
fn sqlite_sidecars_are_regular_and_share_the_database_read_budget() {
    const LIMIT: u64 = 64 * 1024 * 1024;
    for suffix in ["-wal", "-journal", "-shm"] {
        let (root, bundle) = fixture();
        drop(bundle);
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(root.join(format!("project.sqlite{suffix}")))
            .unwrap()
            .set_len(LIMIT)
            .unwrap();
        assert_sidecar_budget_error(Bundle::open(&root, OpenMode::ReadOnly));
    }
}

#[cfg(unix)]
#[test]
fn sqlite_sidecar_symlinks_are_rejected_before_sqlite_opens() {
    use std::os::unix::fs::symlink;

    let (root, bundle) = fixture();
    drop(bundle);
    symlink("project.sqlite", root.join("project.sqlite-wal")).unwrap();
    match Bundle::open(&root, OpenMode::ReadOnly) {
        Err(StoreError::Invalid(message)) => assert!(message.contains("nonsymlink SQLite sidecar")),
        Ok(_) => panic!("SQLite sidecar symlink was accepted"),
        Err(error) => panic!("unexpected error before sidecar type check: {error}"),
    }
}

#[test]
fn valid_oversized_wal_cannot_bypass_the_combined_budget() {
    const LIMIT: u64 = 64 * 1024 * 1024;
    let (root, bundle) = fixture();
    drop(bundle);
    let raw = Connection::open(root.join("project.sqlite")).unwrap();
    raw.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=OFF;
         PRAGMA wal_autocheckpoint=100000000;
         CREATE TEMP TABLE original(body BLOB);
         INSERT INTO original SELECT body FROM bundle_manifest;",
    )
    .unwrap();
    let padding = " ".repeat(1024 * 1024);
    for _ in 0..80 {
        raw.execute(
            "UPDATE bundle_manifest SET body=(SELECT body FROM original) || ?1",
            [&padding],
        )
        .unwrap();
        raw.execute(
            "UPDATE bundle_manifest SET body=(SELECT body FROM original)",
            [],
        )
        .unwrap();
        if fs::metadata(root.join("project.sqlite-wal")).unwrap().len() > LIMIT {
            break;
        }
    }
    let wal_bytes = fs::metadata(root.join("project.sqlite-wal")).unwrap().len();
    assert!(
        wal_bytes > LIMIT,
        "test failed to construct an oversized WAL"
    );
    assert_sidecar_budget_error(Bundle::open(&root, OpenMode::ReadOnly));
}

#[test]
fn bounded_valid_wal_remains_available_for_sqlite_recovery() {
    const LIMIT: u64 = 64 * 1024 * 1024;
    let (root, bundle) = fixture();
    let mut expected = bundle.manifest().unwrap();
    expected.revision = 1;
    expected.updated_utc_ms = 2;
    expected.validate().unwrap();
    let expected_body = serde_json::to_vec_pretty(&expected).unwrap();
    drop(bundle);
    let raw = Connection::open(root.join("project.sqlite")).unwrap();
    raw.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=100000000;")
        .unwrap();
    raw.execute(
        "UPDATE bundle_manifest SET revision=1, body=?1",
        [&expected_body],
    )
    .unwrap();
    let wal_bytes = fs::metadata(root.join("project.sqlite-wal")).unwrap().len();
    assert!(wal_bytes > 0 && wal_bytes < LIMIT);
    let opened = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(opened.manifest().unwrap(), expected);
}

#[test]
fn bounded_non_hot_rollback_sidecar_allows_readwrite_open() {
    let (root, bundle) = fixture();
    drop(bundle);
    // A zero header is explicitly not a hot journal according to SQLite's
    // recovery predicate; it is still part of the checked bundle envelope.
    fs::write(root.join("project.sqlite-journal"), [0_u8; 512]).unwrap();
    let writable = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
    assert_eq!(writable.manifest().unwrap().revision, 0);
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
