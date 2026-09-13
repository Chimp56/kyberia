use kyberia_application::{
    Application, ApplicationError, CreateProject, OpenProject, ProjectQuery, ProjectQueryResult,
    ProjectState, SessionMode,
};
use kyberia_domain::identity::Text;
use kyberia_project_store::{Bundle, OpenMode};
use kyberia_resource_budget::CancellationHook;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

fn retained_directory() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
    fs::create_dir_all(&root).unwrap();
    loop {
        let candidate = root.join(format!(
            "application-project-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot retain test directory {candidate:?}: {error}"),
        }
    }
}

fn create_request(path: &Path, name: &str) -> CreateProject {
    CreateProject {
        path: path.to_path_buf(),
        name: Text::new(name).unwrap(),
        created_utc_ms: 1,
    }
}

fn current(
    session: &kyberia_application::ProjectSession,
) -> kyberia_application::CurrentProjectView {
    let ProjectQueryResult::CurrentSnapshot(view) =
        session.query(ProjectQuery::CurrentSnapshot).unwrap();
    view
}

#[test]
fn create_then_reopen_returns_the_same_canonical_baseline() {
    let root = retained_directory();
    let path = root.join("home.rfatlas");
    let app = Application;
    let created = app.create(create_request(&path, "Home")).unwrap();
    assert_eq!(created.mode(), SessionMode::ReadWrite);
    let first = current(&created);
    assert_eq!(first.state(), ProjectState::BaselineOnly);
    assert_eq!(first.manifest_name(), "Home");
    assert_eq!(first.project().unwrap().name().as_str(), "Home");
    assert_eq!(first.revision().unwrap().project_revision(), 0);
    assert_eq!(first.revision().unwrap().bundle_revision(), 1);

    drop(created);
    let reopened = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap();
    assert_eq!(reopened.mode(), SessionMode::ReadOnly);
    assert_eq!(current(&reopened), first);
}

#[test]
fn duplicate_create_is_structured_and_does_not_replace_the_existing_project() {
    let root = retained_directory();
    let path = root.join("protected.rfatlas");
    let app = Application;
    let created = app.create(create_request(&path, "Original")).unwrap();
    let before = current(&created);
    let error = app
        .create(create_request(&path, "Replacement"))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        kyberia_application::ErrorKind::ProjectAlreadyExists
    );
    assert_eq!(current(&created), before);
}

#[test]
fn missing_project_is_rejected_at_open() {
    let path = retained_directory().join("missing.rfatlas");
    let error = Application
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::MissingProject);
}

#[test]
fn corrupt_canonical_artifact_is_rejected_by_the_application_query() {
    let root = retained_directory();
    let path = root.join("corrupt.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Corruptible")).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let artifact = manifest["artifacts"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .to_owned();
    fs::write(
        path.join("artifacts").join(artifact),
        b"corrupt canonical bytes",
    )
    .unwrap();
    let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn unsupported_schema_is_rejected_even_when_read_only_metadata_is_supported_by_storage() {
    let root = retained_directory();
    let path = root.join("future.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Future")).unwrap();
    drop(session);

    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    manifest["schema_version"] = serde_json::Value::from(2_u64);
    db.execute(
        "UPDATE bundle_manifest SET body=?1",
        [serde_json::to_vec(&manifest).unwrap()],
    )
    .unwrap();
    db.execute_batch("PRAGMA user_version=2").unwrap();

    let error = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(
        error.kind(),
        kyberia_application::ErrorKind::UnsupportedVersion
    );
}

#[test]
fn query_view_is_immutable_and_reads_canonical_publication_after_reopen() {
    use kyberia_causal_materializer::materialize;
    use kyberia_operation_log::{OperationSet, ProjectVersion};
    use kyberia_project_store::MaterializationPublicationOutcome;

    let root = retained_directory();
    let path = root.join("published.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Canonical")).unwrap();
    let before = current(&session);
    let before_revision = before.revision().unwrap();
    let before_project = before.project().unwrap().clone();

    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let baseline = writer.materialization_baseline().unwrap().unwrap();
    let operations = OperationSet::empty(baseline.id());
    let materialized = materialize(&baseline, &operations).unwrap();
    assert!(matches!(
        writer
            .publish_materialized_project(
                &baseline,
                &operations,
                &materialized,
                ProjectVersion::new(0),
                2,
            )
            .unwrap(),
        MaterializationPublicationOutcome::Published(_)
    ));
    drop(writer);

    assert_eq!(before.project().unwrap(), &before_project);
    assert_eq!(before.revision().unwrap(), before_revision);
    let after = current(&session);
    assert_eq!(after.state(), ProjectState::MaterializedCurrent);
    assert_eq!(after.project().unwrap(), &before_project);
    assert_eq!(after.revision().unwrap().bundle_revision(), 2);
    assert_eq!(after.revision().unwrap().operation_revision(), Some(0));
    assert_ne!(after, before);
}

struct CancelImmediately;

impl CancellationHook for CancelImmediately {
    fn is_cancelled(&mut self) -> bool {
        true
    }
}

#[test]
fn cancellation_prevents_returning_a_query_result() {
    let root = retained_directory();
    let path = root.join("cancelled.rfatlas");
    let session = Application
        .create(create_request(&path, "Cancelled"))
        .unwrap();
    let mut cancellation = CancelImmediately;
    let error = session
        .query_with_cancel(ProjectQuery::CurrentSnapshot, &mut cancellation)
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::Cancelled);
}

#[test]
fn legacy_bundle_without_a_baseline_is_explicitly_absent() {
    let root = retained_directory();
    let path = root.join("legacy.rfatlas");
    let id = kyberia_domain::identity::ProjectId::from_bytes([7; 16]).unwrap();
    drop(Bundle::create(&path, id, "Legacy".into(), 1).unwrap());
    let session = Application
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap();
    let view = current(&session);
    assert_eq!(view.state(), ProjectState::LegacyAbsent);
    assert!(view.project().is_none());
    assert!(view.revision().is_none());
}

fn assert_application_error_is_structured(error: ApplicationError) {
    assert!(!error.message().is_empty());
}

#[test]
fn invalid_create_timestamp_is_rejected_before_reserving_a_directory() {
    let root = retained_directory();
    let path = root.join("invalid.rfatlas");
    let error = Application
        .create(CreateProject {
            path: path.clone(),
            name: Text::new("Invalid").unwrap(),
            created_utc_ms: -1,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::InvalidRequest);
    assert_application_error_is_structured(error);
    assert!(!path.exists());
}
