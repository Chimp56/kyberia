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
};

pub const IPC_SCHEMA: &str = "kyberia.desktop-ipc/1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBlankProjectRequest {
    pub schema: String,
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
    pub grant_id: String,
    pub mode: String,
    pub expected_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SchemaRequest {
    pub schema: String,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GrantKind {
    Open,
}

#[derive(Debug)]
struct NativeProjectGrant {
    path: PathBuf,
    display_name: String,
    kind: GrantKind,
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
    state.grants.insert(
        grant_id.clone(),
        NativeProjectGrant {
            path: canonical,
            display_name: display_name.clone(),
            kind: GrantKind::Open,
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

pub fn begin_job(state: &mut DesktopState) -> Result<(String, Arc<JobControl>), DesktopIpcError> {
    if !state.jobs.is_empty() {
        return Err(error(
            "resource_limit",
            "Another project operation is already running.",
            Some("Wait for the current project operation to finish."),
            true,
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let control = Arc::new(JobControl::new());
    state.jobs.insert(id.clone(), Arc::clone(&control));
    Ok((id, control))
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
        let (job_id, control) = begin_job(&mut state).expect("first job");
        assert_eq!(
            begin_job(&mut state).expect_err("second job").code,
            "resource_limit"
        );
        cancel_job(&state, &job_id).expect("cancel job");
        assert!(control.is_cancelled());
        finish_job(&mut state, &job_id);
        assert!(begin_job(&mut state).is_ok());
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
