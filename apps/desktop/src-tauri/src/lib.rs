use kyberia_application::{
    Application, ApplicationError, CreateProject, OpenProject, ProjectQuery, ProjectQueryResult,
    ProjectSession, ProjectState, SessionMode,
};
use kyberia_resource_budget::CancellationHook;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

pub const IPC_SCHEMA: &str = "kyberia.desktop-ipc/1";
const MAX_OPEN_GRANTS: usize = 8;
const OPEN_GRANT_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBlankProjectRequest {
    pub schema: String,
    pub job_id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectOpenProjectRequest {
    pub schema: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectGrantRequest {
    pub schema: String,
    pub job_id: String,
    pub grant_id: String,
    pub mode: String,
    pub expected_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaRequest {
    pub schema: String,
    pub job_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub schema: String,
    pub job_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopIpcError {
    pub schema: &'static str,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    pub retryable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySummary {
    pub id: &'static str,
    pub label: &'static str,
    pub state: &'static str,
    pub detail: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub project_id: String,
    pub name: String,
    pub state: &'static str,
    pub schema_version: &'static str,
    pub revision: u64,
    pub logical_time: u64,
    pub has_floor_plan: bool,
    pub calibrated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentProjectResponse {
    pub schema: &'static str,
    pub state: &'static str,
    pub project: Option<ProjectSummary>,
    pub capabilities: Vec<CapabilitySummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectSelection {
    pub grant_id: String,
    pub display_name: String,
    pub kind: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectSelectionResponse {
    pub schema: &'static str,
    pub selection: Option<OpenProjectSelection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatusResponse {
    pub schema: &'static str,
    pub job_id: String,
    pub state: &'static str,
    pub progress: u8,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCancelResponse {
    pub schema: &'static str,
    pub job_id: String,
    pub state: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GrantKind {
    Open,
}

#[derive(Debug)]
struct NativeProjectGrant {
    path: PathBuf,
    display_name: String,
    kind: GrantKind,
    issued_at: Instant,
}

#[derive(Debug)]
pub struct JobControl {
    cancelled: AtomicBool,
    progress: AtomicU8,
}

impl JobControl {
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            progress: AtomicU8::new(0),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn progress(&self) -> u8 {
        self.progress.load(Ordering::Acquire)
    }

    pub fn set_progress(&self, progress: u8) {
        self.progress.store(progress.min(100), Ordering::Release);
    }
}

impl Default for JobControl {
    fn default() -> Self {
        Self::new()
    }
}

pub struct JobCancellation {
    control: Arc<JobControl>,
}

impl JobCancellation {
    pub fn new(control: Arc<JobControl>) -> Self {
        Self { control }
    }

    pub fn control(&self) -> &Arc<JobControl> {
        &self.control
    }

    pub fn is_cancelled(&self) -> bool {
        self.control.is_cancelled()
    }
}

impl CancellationHook for JobCancellation {
    fn is_cancelled(&mut self) -> bool {
        self.control.is_cancelled()
    }
}

pub struct DesktopState {
    application: Application,
    session: Option<ProjectSession>,
    grants: HashMap<String, NativeProjectGrant>,
    pub(crate) jobs: HashMap<String, Arc<JobControl>>,
}

impl Default for DesktopState {
    fn default() -> Self {
        Self {
            application: Application,
            session: None,
            grants: HashMap::new(),
            jobs: HashMap::new(),
        }
    }
}

impl DesktopState {
    pub fn application(&self) -> Application {
        self.application
    }

    pub fn take_session(&mut self) -> Option<ProjectSession> {
        self.session.take()
    }

    pub fn set_session(&mut self, session: ProjectSession) {
        self.session = Some(session);
    }
}

pub fn error(
    code: impl Into<String>,
    message: impl Into<String>,
    remediation: Option<&str>,
    retryable: bool,
) -> DesktopIpcError {
    DesktopIpcError {
        schema: IPC_SCHEMA,
        code: code.into(),
        message: scrub_user_message(&message.into()),
        remediation: remediation.map(str::to_owned),
        retryable,
    }
}

pub fn cancelled_error() -> DesktopIpcError {
    error(
        "cancelled",
        "The desktop project operation was cancelled.",
        Some("Run the operation again when ready."),
        true,
    )
}

/// Application create/open are short atomic storage operations without an
/// internal cancellation seam. Poll immediately before and after that step so
/// cancellation never publishes its returned session into the desktop state.
pub fn run_atomic_project_step<T>(
    control: &JobControl,
    work: impl FnOnce() -> Result<T, DesktopIpcError>,
) -> Result<T, DesktopIpcError> {
    if control.is_cancelled() {
        return Err(cancelled_error());
    }
    let value = work()?;
    if control.is_cancelled() {
        return Err(cancelled_error());
    }
    Ok(value)
}

fn scrub_user_message(message: &str) -> String {
    if message.split_whitespace().any(is_absolute_path) {
        message
            .split_whitespace()
            .map(|word| {
                if is_absolute_path(word) {
                    "[local path]"
                } else {
                    word
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        message.to_owned()
    }
}

fn is_absolute_path(value: &str) -> bool {
    let value = value.trim_matches(|character: char| ",.;:()[]{}\"'".contains(character));
    value.starts_with('/')
        || value.starts_with('\\')
        || (value.as_bytes().get(1) == Some(&b':')
            && value
                .as_bytes()
                .get(2)
                .is_some_and(|byte| *byte == b'\\' || *byte == b'/'))
}

pub fn require_schema(schema: &str) -> Result<(), DesktopIpcError> {
    if schema == IPC_SCHEMA {
        Ok(())
    } else {
        Err(error(
            "unsupported_version",
            "The desktop command schema is not supported by this build.",
            Some("Update RF Atlas before sending project commands."),
            false,
        ))
    }
}

impl From<ApplicationError> for DesktopIpcError {
    fn from(value: ApplicationError) -> Self {
        let retryable = matches!(
            value.kind(),
            kyberia_application::ErrorKind::Storage | kyberia_application::ErrorKind::ResourceLimit
        );
        let remediation = match value.kind() {
            kyberia_application::ErrorKind::MissingProject => {
                Some("Choose an existing .rfatlas directory.")
            }
            kyberia_application::ErrorKind::ProjectAlreadyExists => {
                Some("Choose a new project location.")
            }
            kyberia_application::ErrorKind::UnsupportedVersion => {
                Some("Update RF Atlas before opening this project.")
            }
            kyberia_application::ErrorKind::CorruptProject => {
                Some("Keep the original bundle and use the project verification tools.")
            }
            _ => None,
        };
        error(
            value.kind().as_str(),
            value.message(),
            remediation,
            retryable,
        )
    }
}

fn capabilities() -> Vec<CapabilitySummary> {
    vec![
        CapabilitySummary {
            id: "nearby_scan",
            label: "Nearby network scan",
            state: "unavailable",
            detail: "No native collector is connected to this session.",
            remediation: Some("Pair a supported collector to inspect nearby networks."),
        },
        CapabilitySummary {
            id: "current_link",
            label: "Current link",
            state: "unavailable",
            detail: "Current-link telemetry is not exposed by this build.",
            remediation: Some("Use a platform collector when it is available."),
        },
        CapabilitySummary {
            id: "monitor_frames",
            label: "Passive frame capture",
            state: "unavailable",
            detail: "Passive capture is adapter and driver dependent.",
            remediation: Some("Pair a supported Linux or Windows monitor collector."),
        },
        CapabilitySummary {
            id: "active_probes",
            label: "Active probes",
            state: "unavailable",
            detail: "Gateway, LAN, and Internet probes are not connected.",
            remediation: Some(
                "Add an authenticated active agent before measuring latency or throughput.",
            ),
        },
    ]
}

fn state_name(state: ProjectState) -> &'static str {
    match state {
        ProjectState::MaterializedCurrent => "materialized_current",
        ProjectState::BaselineOnly => "baseline_only",
        ProjectState::LegacyAbsent => "legacy_absent",
    }
}

fn response_for_view(view: kyberia_application::CurrentProjectView) -> CurrentProjectResponse {
    let state = state_name(view.state());
    let project = view.project().map(|project| ProjectSummary {
        project_id: String::from(view.project_id()),
        name: project.name().as_str().to_owned(),
        state,
        schema_version: match project.schema_version() {
            kyberia_domain::project::ProjectSchemaVersion::V1 => "1",
            kyberia_domain::project::ProjectSchemaVersion::V2 => "2",
        },
        revision: project.revision(),
        logical_time: project.logical_time(),
        has_floor_plan: project.floors().next().is_some(),
        // The current application query exposes floor entities but no map
        // calibration projection. Keep scale unavailable until that evidence
        // crosses the versioned boundary explicitly.
        calibrated: false,
    });
    CurrentProjectResponse {
        schema: IPC_SCHEMA,
        state,
        project,
        capabilities: capabilities(),
    }
}

pub fn response_for_with_cancel(
    session: &ProjectSession,
    cancel: &mut impl CancellationHook,
) -> Result<CurrentProjectResponse, DesktopIpcError> {
    let ProjectQueryResult::CurrentSnapshot(view) =
        session.query_with_cancel(ProjectQuery::CurrentSnapshot, cancel)?;
    Ok(response_for_view(view))
}

pub fn current_project(state: &DesktopState) -> Result<CurrentProjectResponse, DesktopIpcError> {
    let Some(session) = state.session.as_ref() else {
        return Ok(CurrentProjectResponse {
            schema: IPC_SCHEMA,
            state: "no_project",
            project: None,
            capabilities: capabilities(),
        });
    };
    let control = Arc::new(JobControl::new());
    response_for_with_cancel(session, &mut JobCancellation::new(control))
}

pub fn no_project_response() -> CurrentProjectResponse {
    CurrentProjectResponse {
        schema: IPC_SCHEMA,
        state: "no_project",
        project: None,
        capabilities: capabilities(),
    }
}

pub fn create_project_at(
    state: &mut DesktopState,
    path: PathBuf,
    name: String,
    created_utc_ms: i64,
) -> Result<CurrentProjectResponse, DesktopIpcError> {
    let name = kyberia_domain::identity::Text::new(name).map_err(|value| {
        error(
            "invalid_request",
            value.to_string(),
            Some("Provide a non-empty project name."),
            false,
        )
    })?;
    let session = state.application.create(CreateProject {
        path,
        name,
        created_utc_ms,
    })?;
    state.session = Some(session);
    current_project(state)
}

pub fn open_project_at(
    state: &mut DesktopState,
    path: PathBuf,
    mode: SessionMode,
) -> Result<CurrentProjectResponse, DesktopIpcError> {
    let session = state.application.open(OpenProject { path, mode })?;
    state.session = Some(session);
    current_project(state)
}

pub fn blank_project_path(app_data_dir: PathBuf) -> Result<PathBuf, DesktopIpcError> {
    std::fs::create_dir_all(&app_data_dir).map_err(|value| {
        error(
            "storage",
            format!("Could not prepare the local project directory: {value}"),
            Some("Choose a writable application data directory."),
            true,
        )
    })?;
    let root = std::fs::canonicalize(&app_data_dir).map_err(|value| {
        error(
            "storage",
            format!("Could not verify the local project directory: {value}"),
            Some("Choose a writable application data directory."),
            true,
        )
    })?;
    Ok(root.join(format!("rf-atlas-{}.rfatlas", uuid::Uuid::new_v4())))
}

pub fn issue_open_grant(
    state: &mut DesktopState,
    selected: PathBuf,
) -> Result<OpenProjectSelectionResponse, DesktopIpcError> {
    prune_expired_grants(state, Instant::now());
    let metadata = std::fs::symlink_metadata(&selected).map_err(|_| {
        error(
            "missing_project",
            "The selected project directory could not be found.",
            Some("Choose an existing .rfatlas directory."),
            false,
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(error(
            "invalid_request",
            "Symbolic-link project roots are not accepted.",
            Some("Choose the canonical .rfatlas directory."),
            false,
        ));
    }
    let canonical = std::fs::canonicalize(&selected).map_err(|_| {
        error(
            "invalid_request",
            "The selected project directory could not be canonicalized.",
            Some("Choose an existing .rfatlas directory."),
            false,
        )
    })?;
    if !canonical.is_dir()
        || canonical.extension().and_then(|value| value.to_str()) != Some("rfatlas")
    {
        return Err(error(
            "invalid_request",
            "Choose a canonical .rfatlas project directory.",
            Some("Select a directory whose name ends in .rfatlas."),
            false,
        ));
    }
    let display_name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Project")
        .to_owned();
    let grant_id = uuid::Uuid::new_v4().to_string();
    if state.grants.len() >= MAX_OPEN_GRANTS
        && let Some(oldest) = state
            .grants
            .iter()
            .min_by_key(|(_, grant)| grant.issued_at)
            .map(|(id, _)| id.clone())
    {
        state.grants.remove(&oldest);
    }
    state.grants.insert(
        grant_id.clone(),
        NativeProjectGrant {
            path: canonical,
            display_name: display_name.clone(),
            kind: GrantKind::Open,
            issued_at: Instant::now(),
        },
    );
    Ok(OpenProjectSelectionResponse {
        schema: IPC_SCHEMA,
        selection: Some(OpenProjectSelection {
            grant_id,
            display_name,
            kind: "open",
        }),
    })
}

pub fn consume_open_grant(
    state: &mut DesktopState,
    grant_id: &str,
    expected_name: Option<&str>,
) -> Result<PathBuf, DesktopIpcError> {
    prune_expired_grants(state, Instant::now());
    let Some(grant) = state.grants.get(grant_id) else {
        return Err(error(
            "invalid_grant",
            "The project selection is invalid or has expired.",
            Some("Choose the project again."),
            false,
        ));
    };
    if grant.kind != GrantKind::Open || expected_name.is_some_and(|name| name != grant.display_name)
    {
        return Err(error(
            "invalid_grant",
            "The project selection does not match this open request.",
            Some("Choose the project again."),
            false,
        ));
    }
    let path = grant.path.clone();
    state.grants.remove(grant_id);
    Ok(path)
}

pub fn begin_open_job(
    state: &mut DesktopState,
    job_id: &str,
    grant_id: &str,
    expected_name: Option<&str>,
) -> Result<(Arc<JobControl>, PathBuf), DesktopIpcError> {
    let control = begin_job(state, job_id)?;
    match consume_open_grant(state, grant_id, expected_name) {
        Ok(path) => Ok((control, path)),
        Err(error) => {
            finish_job(state, job_id);
            Err(error)
        }
    }
}

fn prune_expired_grants(state: &mut DesktopState, now: Instant) {
    state.grants.retain(|_, grant| {
        now.checked_duration_since(grant.issued_at)
            .is_some_and(|age| age <= OPEN_GRANT_TTL)
    });
}

pub fn begin_job(
    state: &mut DesktopState,
    job_id: &str,
) -> Result<Arc<JobControl>, DesktopIpcError> {
    let parsed_id = uuid::Uuid::parse_str(job_id).ok();
    if parsed_id.as_ref().is_none_or(|value| {
        value.hyphenated().to_string() != job_id
            || value.get_version() != Some(uuid::Version::Random)
            || value.get_variant() != uuid::Variant::RFC4122
    }) {
        return Err(error(
            "invalid_request",
            "The project job identifier is malformed.",
            Some("Start the project operation again."),
            false,
        ));
    }
    if !state.jobs.is_empty() {
        return Err(error(
            "resource_limit",
            "Another project operation is already running.",
            Some("Wait for the current project operation to finish."),
            true,
        ));
    }
    let control = Arc::new(JobControl::new());
    state.jobs.insert(job_id.to_owned(), Arc::clone(&control));
    Ok(control)
}

pub fn finish_job(state: &mut DesktopState, job_id: &str) {
    state.jobs.remove(job_id);
}

pub fn cancel_job(state: &DesktopState, job_id: &str) -> Result<(), DesktopIpcError> {
    state
        .jobs
        .get(job_id)
        .map(|control| control.cancel())
        .ok_or_else(|| {
            error(
                "invalid_request",
                "That project operation is no longer running.",
                Some("Refresh the project state."),
                false,
            )
        })
}

pub fn job_control(state: &DesktopState, job_id: &str) -> Option<Arc<JobControl>> {
    state.jobs.get(job_id).cloned()
}

pub fn finish_joined_job<T, E>(
    state: &mut DesktopState,
    job_id: &str,
    joined: Result<T, E>,
) -> Result<T, DesktopIpcError> {
    finish_job(state, job_id);
    joined.map_err(|_| {
        error(
            "storage",
            "The desktop project operation stopped unexpectedly.",
            Some("Retry the project operation."),
            true,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retained_path(label: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.trash/test-runs");
        std::fs::create_dir_all(&root).expect("retained test root");
        root.join(format!("desktop-{label}-{}.rfatlas", uuid::Uuid::new_v4()))
    }

    #[test]
    fn command_schema_is_checked_before_application_work() {
        assert_eq!(
            require_schema("kyberia.desktop-ipc/0")
                .expect_err("version must be rejected")
                .code,
            "unsupported_version"
        );
    }

    #[test]
    fn create_mapping_returns_canonical_baseline_without_floor_data() {
        let mut state = DesktopState::default();
        let response = create_project_at(
            &mut state,
            retained_path("create"),
            "Test project".into(),
            1_800_000_000_000,
        )
        .expect("create project");
        assert_eq!(response.schema, IPC_SCHEMA);
        assert_eq!(response.state, "baseline_only");
        assert_eq!(
            response
                .project
                .as_ref()
                .map(|project| project.has_floor_plan),
            Some(false)
        );
        assert_eq!(
            response.project.as_ref().map(|project| project.calibrated),
            Some(false)
        );
    }

    #[test]
    fn query_mapping_reports_no_project_without_inventing_state() {
        let response = current_project(&DesktopState::default()).expect("query empty session");
        assert_eq!(response.state, "no_project");
        assert!(response.project.is_none());
        assert!(!response.capabilities.is_empty());
    }

    #[test]
    fn blank_paths_are_unique_and_canonicalized_under_app_data() {
        let root = retained_path("app-data")
            .parent()
            .expect("parent")
            .to_path_buf();
        let first = blank_project_path(root.clone()).expect("first path");
        let second = blank_project_path(root).expect("second path");
        assert_ne!(first, second);
        assert!(
            first.starts_with(
                std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.trash"))
                    .unwrap()
            )
        );
    }

    #[test]
    fn forged_reused_and_mismatched_grants_are_rejected() {
        let mut state = DesktopState::default();
        assert_eq!(
            consume_open_grant(&mut state, "forged", None)
                .expect_err("forged grant")
                .code,
            "invalid_grant"
        );
        let root = retained_path("grant");
        std::fs::create_dir_all(&root).expect("project root");
        let selected = issue_open_grant(&mut state, root.clone()).expect("grant");
        let grant_id = selected
            .selection
            .as_ref()
            .expect("selection")
            .grant_id
            .clone();
        assert_eq!(
            consume_open_grant(&mut state, &grant_id, Some("wrong.rfatlas"))
                .expect_err("mismatch")
                .code,
            "invalid_grant"
        );
        let expected = root.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            consume_open_grant(&mut state, &grant_id, Some(expected)).expect("consume"),
            std::fs::canonicalize(root).unwrap()
        );
        assert_eq!(
            consume_open_grant(&mut state, &grant_id, None)
                .expect_err("single use")
                .code,
            "invalid_grant"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_project_roots_are_rejected() {
        let target = retained_path("missing-target");
        let link = retained_path("symlink");
        std::os::unix::fs::symlink(target, &link).expect("symlink");
        let mut state = DesktopState::default();
        assert_eq!(
            issue_open_grant(&mut state, link)
                .expect_err("symlink")
                .code,
            "invalid_request"
        );
    }

    #[test]
    fn job_admission_is_bounded_and_cancellation_is_observable() {
        let mut state = DesktopState::default();
        let job_id = uuid::Uuid::new_v4().to_string();
        let control = begin_job(&mut state, &job_id).expect("first job");
        let second_id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            begin_job(&mut state, &second_id)
                .expect_err("second job")
                .code,
            "resource_limit"
        );
        cancel_job(&state, &job_id).expect("cancel job");
        assert!(control.is_cancelled());
        finish_job(&mut state, &job_id);
        assert!(begin_job(&mut state, &second_id).is_ok());
    }

    #[test]
    fn malformed_job_ids_fail_before_admission() {
        let mut state = DesktopState::default();
        assert_eq!(
            begin_job(&mut state, "not-a-job")
                .expect_err("malformed identifier")
                .code,
            "invalid_request"
        );
        assert!(state.jobs.is_empty());
    }

    #[test]
    fn join_failure_always_releases_admission() {
        let mut state = DesktopState::default();
        let first_id = uuid::Uuid::new_v4().to_string();
        begin_job(&mut state, &first_id).expect("first job");
        let result: Result<(), DesktopIpcError> =
            finish_joined_job(&mut state, &first_id, Err::<(), ()>(()));
        assert_eq!(result.expect_err("join failure").code, "storage");
        let second_id = uuid::Uuid::new_v4().to_string();
        assert!(begin_job(&mut state, &second_id).is_ok());
    }

    #[test]
    fn cancellation_during_atomic_create_keeps_session_and_admission_recoverable() {
        use std::sync::mpsc;

        let mut state = DesktopState::default();
        let active = create_project_at(
            &mut state,
            retained_path("active-before-cancel"),
            "Active project".into(),
            1_800_000_000_000,
        )
        .expect("active project");
        let active_id = active.project.expect("project").project_id;
        let cancelled_path = retained_path("cancelled-create");
        let worker_path = cancelled_path.clone();
        let job_id = uuid::Uuid::new_v4().to_string();
        let control = begin_job(&mut state, &job_id).expect("job");
        let task_control = Arc::clone(&control);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let worker = std::thread::spawn(move || {
            run_atomic_project_step(&task_control, || {
                entered_tx.send(()).expect("entered");
                release_rx.recv().expect("release");
                let name = kyberia_domain::identity::Text::new("Cancelled project".to_owned())
                    .expect("name");
                Application.create(CreateProject {
                    path: worker_path.clone(),
                    name,
                    created_utc_ms: 1_800_000_000_001,
                })?;
                Ok(worker_path)
            })
        });
        entered_rx.recv().expect("worker entered atomic step");
        cancel_job(&state, &job_id).expect("cancel");
        release_tx.send(()).expect("release worker");
        let completed_path = worker
            .join()
            .expect("worker join")
            .expect_err("cancelled after atomic work");
        assert_eq!(completed_path.code, "cancelled");
        finish_job(&mut state, &job_id);

        Application
            .open(OpenProject {
                path: cancelled_path,
                mode: SessionMode::ReadOnly,
            })
            .expect("atomic create left a complete canonical project");

        let current = current_project(&state).expect("current project");
        assert_eq!(
            current.project.expect("active project").project_id,
            active_id
        );
        let next_id = uuid::Uuid::new_v4().to_string();
        assert!(begin_job(&mut state, &next_id).is_ok());
    }

    #[test]
    fn open_grants_are_bounded_and_expire() {
        let mut state = DesktopState::default();
        let mut first_id = String::new();
        for index in 0..=MAX_OPEN_GRANTS {
            let root = retained_path(&format!("bounded-grant-{index}"));
            std::fs::create_dir_all(&root).expect("project root");
            let issued = issue_open_grant(&mut state, root).expect("grant");
            if index == 0 {
                first_id = issued.selection.expect("selection").grant_id;
            }
        }
        assert_eq!(state.grants.len(), MAX_OPEN_GRANTS);
        assert!(!state.grants.contains_key(&first_id));

        let expiring = state.grants.keys().next().expect("grant").clone();
        state.grants.get_mut(&expiring).expect("grant").issued_at =
            Instant::now() - OPEN_GRANT_TTL - Duration::from_secs(1);
        assert_eq!(
            consume_open_grant(&mut state, &expiring, None)
                .expect_err("expired")
                .code,
            "invalid_grant"
        );
    }

    #[test]
    fn busy_open_does_not_consume_a_valid_grant() {
        let mut state = DesktopState::default();
        let root = retained_path("busy-grant");
        std::fs::create_dir_all(&root).expect("project root");
        let selected = issue_open_grant(&mut state, root).expect("grant");
        let selection = selected.selection.expect("selection");
        let active_id = uuid::Uuid::new_v4().to_string();
        begin_job(&mut state, &active_id).expect("active job");

        let blocked_id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            begin_open_job(
                &mut state,
                &blocked_id,
                &selection.grant_id,
                Some(&selection.display_name),
            )
            .expect_err("busy")
            .code,
            "resource_limit"
        );
        finish_job(&mut state, &active_id);

        let retry_id = uuid::Uuid::new_v4().to_string();
        assert!(
            begin_open_job(
                &mut state,
                &retry_id,
                &selection.grant_id,
                Some(&selection.display_name),
            )
            .is_ok()
        );
    }

    #[test]
    fn renderer_errors_scrub_absolute_paths() {
        let value = error(
            "storage",
            "could not open /private/user/secret.rfatlas",
            None,
            true,
        );
        assert!(!value.message.contains("/private/user"));
        assert!(value.message.contains("[local path]"));
    }
}
