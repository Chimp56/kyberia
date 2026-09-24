use kyberia_application::{
    Application, ErrorKind, OpenProject, PointId, PointSnapshotInputVersion, PointSurvey,
    PointSurveySnapshotRequest, SessionMode,
};
use kyberia_domain::identity::{SessionId, Text};
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
            "application-point-survey-snapshot-{}-{}",
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

fn identity<T>(value: u8) -> T
where
    T: TryFrom<String>,
    <T as TryFrom<String>>::Error: std::fmt::Debug,
{
    T::try_from(format!("{value:02x}").repeat(16)).unwrap()
}

fn survey(session: u8) -> PointSurvey {
    let mut fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../survey/tests/fixtures/point-v1.json")).unwrap();
    fixture["config"]["session_id"] =
        serde_json::Value::String(format!("{session:02x}").repeat(16));
    serde_json::from_value(fixture).expect("fixture remains a valid completed point survey")
}

fn create_project(path: &Path) -> kyberia_application::ProjectSession {
    Application
        .create(kyberia_application::CreateProject {
            path: path.to_path_buf(),
            name: Text::new("Survey snapshots").unwrap(),
            created_utc_ms: 1,
        })
        .unwrap()
}

fn request(
    snapshot_id: u8,
    survey: PointSurvey,
    utc_ms: i64,
    revision: Option<u64>,
) -> PointSurveySnapshotRequest {
    PointSurveySnapshotRequest {
        snapshot_id: identity(snapshot_id),
        survey,
        committed_utc_ms: utc_ms,
        expected_bundle_revision: revision,
    }
}

#[test]
fn completed_survey_saves_reopens_and_loads_as_application_owned_typed_data() {
    let path = retained_directory().join("survey.rfatlas");
    let mut created = create_project(&path);
    let state = survey(1);
    assert_eq!(
        state.config().data().point_id,
        PointId::from_bytes([1; 16]).unwrap()
    );
    assert_eq!(state.config().data().session_id, identity(1));
    assert!(state.progress().ready);

    let receipt = created
        .save_point_survey_snapshot(request(40, state.clone(), 2, Some(1)))
        .unwrap();
    assert_eq!(receipt.snapshot_id(), identity(40));
    assert_eq!(receipt.session_id(), identity(1));
    assert_eq!(receipt.point_id(), PointId::from_bytes([1; 16]).unwrap());
    assert_eq!(receipt.bundle_revision(), 2);
    assert_eq!(receipt.committed_utc_ms(), 2);
    assert_eq!(
        receipt.input_schema_version(),
        PointSnapshotInputVersion::V2
    );
    assert_eq!(receipt.decoder_version(), "kyberia-point-snapshot/2.0.0");
    drop(created);

    let reopened = Application
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap();
    let loaded = reopened
        .load_point_survey_snapshot(identity(40), Some(identity(1)))
        .unwrap();
    assert_eq!(loaded.survey(), &state);
    assert_eq!(loaded.receipt(), &receipt);
    assert_eq!(
        loaded.decode_receipt().input_schema_version,
        PointSnapshotInputVersion::V2
    );
    assert!(!loaded.decode_receipt().migrated);
}

#[test]
fn read_only_session_rejects_snapshot_writes_before_store_access() {
    let path = retained_directory().join("readonly.rfatlas");
    drop(create_project(&path));
    let mut readonly = Application
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap();
    let error = readonly
        .save_point_survey_snapshot(request(41, survey(1), 2, None))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::ReadOnly);
}

#[test]
fn stale_expected_bundle_revision_maps_to_conflict_without_publishing() {
    let path = retained_directory().join("revision-conflict.rfatlas");
    let mut session = create_project(&path);
    let error = session
        .save_point_survey_snapshot(request(42, survey(1), 2, Some(0)))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Conflict);
    let kyberia_application::ProjectQueryResult::CurrentSnapshot(current) = session
        .query(kyberia_application::ProjectQuery::CurrentSnapshot)
        .unwrap();
    assert_eq!(current.revision().unwrap().bundle_revision(), 1);
}

#[test]
fn unknown_snapshot_and_expected_session_mismatch_are_invalid_requests() {
    let path = retained_directory().join("unknown-snapshot.rfatlas");
    let mut session = create_project(&path);
    let unknown = session
        .load_point_survey_snapshot(identity(43), None)
        .unwrap_err();
    assert_eq!(unknown.kind(), ErrorKind::InvalidRequest);

    session
        .save_point_survey_snapshot(request(44, survey(1), 2, Some(1)))
        .unwrap();
    let mismatch = session
        .load_point_survey_snapshot(identity(44), Some(identity(2)))
        .unwrap_err();
    assert_eq!(mismatch.kind(), ErrorKind::InvalidRequest);
}

#[test]
fn snapshot_history_is_validated_and_filtered_by_survey_session() {
    let path = retained_directory().join("history.rfatlas");
    let mut session = create_project(&path);
    let first = survey(1);
    let second = survey(2);
    let first_session: SessionId = first.config().data().session_id;
    let second_session = second.config().data().session_id;
    let first_receipt = session
        .save_point_survey_snapshot(request(45, first, 2, Some(1)))
        .unwrap();
    let second_receipt = session
        .save_point_survey_snapshot(request(46, second, 3, Some(2)))
        .unwrap();

    let first_history = session
        .list_point_survey_snapshot_history(Some(first_session))
        .unwrap();
    let second_history = session
        .list_point_survey_snapshot_history(Some(second_session))
        .unwrap();
    let all_history = session.list_point_survey_snapshot_history(None).unwrap();
    assert_eq!(first_history.len(), 1);
    assert_eq!(first_history[0].receipt(), &first_receipt);
    assert_eq!(second_history.len(), 1);
    assert_eq!(second_history[0].receipt(), &second_receipt);
    assert_eq!(all_history.len(), 2);
    assert_eq!(all_history[0].receipt().snapshot_id(), identity(45));
    assert_eq!(all_history[1].receipt().snapshot_id(), identity(46));
}
