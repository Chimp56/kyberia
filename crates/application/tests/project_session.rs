use kyberia_application::{
    Application, ApplicationError, CreateProject, OpenProject, ProjectQuery, ProjectQueryResult,
    ProjectState, SessionMode,
};
use kyberia_domain::identity::Text;
use kyberia_project_store::{ArtifactEntry, ArtifactKind, Bundle, OpenMode};
use kyberia_resource_budget::{CancellationHook, ResourceBudget, ResourceLimits};
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
    assert_eq!(
        after.revision().unwrap().publication_bundle_revision(),
        Some(2)
    );
    assert_eq!(after.revision().unwrap().operation_revision(), Some(0));
    assert_ne!(after, before);

    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let ordinary_bytes = b"ordinary map source metadata";
    writer
        .put_artifact(
            ordinary_bytes,
            ArtifactEntry {
                kind: ArtifactKind::MapSource,
                bytes: ordinary_bytes.len() as u64,
                media_type: "application/octet-stream".into(),
                provenance_id: "application-test-map-source".into(),
            },
            3,
        )
        .unwrap();
    drop(writer);

    let after_artifact = current(&session);
    assert_eq!(after_artifact.revision().unwrap().bundle_revision(), 3);
    assert_eq!(
        after_artifact
            .revision()
            .unwrap()
            .publication_bundle_revision(),
        Some(2)
    );
}

#[test]
fn missing_project_database_inside_an_existing_root_is_corruption() {
    let root = retained_directory();
    let path = root.join("missing-database.rfatlas");
    let app = Application;
    drop(
        app.create(create_request(&path, "Missing database"))
            .unwrap(),
    );
    let retained = retained_directory().join("project.sqlite.moved");
    fs::rename(path.join("project.sqlite"), &retained).unwrap();

    let error = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn missing_artifacts_directory_inside_an_existing_root_is_corruption() {
    let root = retained_directory();
    let path = root.join("missing-artifacts-directory.rfatlas");
    let app = Application;
    drop(
        app.create(create_request(&path, "Missing artifacts directory"))
            .unwrap(),
    );
    let retained = retained_directory().join("artifacts.moved");
    fs::rename(path.join("artifacts"), retained).unwrap();

    let error = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn missing_declared_artifact_is_corruption_at_query_time() {
    let root = retained_directory();
    let path = root.join("missing-artifact.rfatlas");
    let app = Application;
    let session = app
        .create(create_request(&path, "Missing artifact"))
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let artifact = manifest["artifacts"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .to_owned();
    let retained = retained_directory().join("declared-artifact.moved");
    fs::rename(path.join("artifacts").join(artifact), retained).unwrap();

    let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn malformed_manifest_after_open_is_corruption_at_query_time() {
    let root = retained_directory();
    let path = root.join("malformed-manifest.rfatlas");
    let app = Application;
    let session = app
        .create(create_request(&path, "Malformed manifest"))
        .unwrap();
    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE bundle_manifest SET body=?1",
        [b"not-json".as_slice()],
    )
    .unwrap();

    let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn caller_budget_is_cumulative_and_reports_resource_limit() {
    let root = retained_directory();
    let path = root.join("budget.rfatlas");
    let session = Application.create(create_request(&path, "Budget")).unwrap();
    let mut budget = ResourceBudget::new(ResourceLimits::new(0, 0, 0, 0, 0, 0));

    let error = session
        .query_with_budget(ProjectQuery::CurrentSnapshot, &mut budget)
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::ResourceLimit);
    assert!(budget.usage().working_set_bytes() == 0);
}

#[test]
fn query_budget_is_shared_across_materialization_publication_history() {
    use kyberia_causal_materializer::materialize;
    use kyberia_domain::identity::{ActorDeviceId, ActorId, OperationId};
    use kyberia_operation_log::{
        CausalDepth, LogicalTimestamp, Mutation, Operation, OperationSet, ProjectVersion,
    };
    use kyberia_project_store::MaterializationPublicationOutcome;

    let root = retained_directory();
    let path = root.join("budget-history.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Budget history")).unwrap();
    let id = current(&session).project_id();
    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let baseline = writer.materialization_baseline().unwrap().unwrap();
    let empty = OperationSet::empty(id);
    let initial = materialize(&baseline, &empty).unwrap();
    assert!(matches!(
        writer
            .publish_materialized_project(&baseline, &empty, &initial, ProjectVersion::new(0), 2)
            .unwrap(),
        MaterializationPublicationOutcome::Published(_)
    ));

    let first = Operation::try_apply(
        OperationId::from_bytes([1; 16]).unwrap(),
        id,
        ActorId::from_bytes([21; 16]).unwrap(),
        ActorDeviceId::from_bytes([41; 16]).unwrap(),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::set_project_name(Text::new("Budget one").unwrap()),
        Mutation::set_project_name(Text::new("Budget history").unwrap()),
    )
    .unwrap();
    writer
        .append_operation_if_revision(first.clone(), Some(ProjectVersion::new(0)))
        .unwrap();
    let first_set = OperationSet::from_operations([first.clone()]).unwrap();
    let first_project = materialize(&baseline, &first_set).unwrap();
    writer
        .publish_materialized_project(
            &baseline,
            &first_set,
            &first_project,
            ProjectVersion::new(1),
            3,
        )
        .unwrap();

    let second = Operation::try_apply(
        OperationId::from_bytes([2; 16]).unwrap(),
        id,
        ActorId::from_bytes([22; 16]).unwrap(),
        ActorDeviceId::from_bytes([42; 16]).unwrap(),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id()],
        Mutation::set_project_name(Text::new("Budget two").unwrap()),
        Mutation::set_project_name(Text::new("Budget one").unwrap()),
    )
    .unwrap();
    writer
        .append_operation_if_revision(second.clone(), Some(ProjectVersion::new(1)))
        .unwrap();
    let second_set = OperationSet::from_operations([first, second]).unwrap();
    let second_project = materialize(&baseline, &second_set).unwrap();
    writer
        .publish_materialized_project(
            &baseline,
            &second_set,
            &second_project,
            ProjectVersion::new(2),
            4,
        )
        .unwrap();
    drop(writer);

    let mut unrestricted = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    ));
    session
        .query_with_budget(ProjectQuery::CurrentSnapshot, &mut unrestricted)
        .unwrap();
    let total_copy_bytes = unrestricted.usage().project_copy_bytes();
    assert!(total_copy_bytes > 1);

    let mut cumulative = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        total_copy_bytes - 1,
        usize::MAX,
    ));
    let error = session
        .query_with_budget(ProjectQuery::CurrentSnapshot, &mut cumulative)
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::ResourceLimit);
    assert!(cumulative.usage().project_copy_bytes() > 0);
    assert!(cumulative.usage().project_copy_bytes() < total_copy_bytes);
}

#[test]
fn already_open_session_revalidates_schema_and_required_features() {
    for (label, schema_version, required_features, database_version) in [
        ("logical-schema", 2_u64, serde_json::json!([]), 2_u64),
        (
            "logical-feature",
            1_u64,
            serde_json::json!(["future-materialization"]),
            1_u64,
        ),
    ] {
        let root = retained_directory();
        let path = root.join(format!("{label}.rfatlas"));
        let app = Application;
        let session = app.create(create_request(&path, label)).unwrap();
        let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
        manifest["schema_version"] = serde_json::Value::from(schema_version);
        manifest["required_features"] = required_features;
        db.execute(
            "UPDATE bundle_manifest SET body=?1",
            [serde_json::to_vec(&manifest).unwrap()],
        )
        .unwrap();
        db.execute_batch(&format!("PRAGMA user_version={database_version}"))
            .unwrap();

        let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
        assert_eq!(
            error.kind(),
            kyberia_application::ErrorKind::UnsupportedVersion
        );
    }
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
    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    db.execute_batch(
        "DROP TABLE materialized_project_state;
         DROP TABLE materialized_project_publications;
         DROP TABLE materialization_baselines;",
    )
    .unwrap();
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
