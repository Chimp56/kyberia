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
        Arc, Mutex,
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
    pub job_id: String,
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
    session: Option<Arc<Mutex<ProjectSession>>>,
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

    pub fn session(&self) -> Option<Arc<Mutex<ProjectSession>>> {
        self.session.as_ref().map(Arc::clone)
    }

    pub fn set_session(&mut self, session: ProjectSession) {
        self.session = Some(Arc::new(Mutex::new(session)));
    }
}

fn lock_session_for_query(
    session: &Mutex<ProjectSession>,
) -> std::sync::MutexGuard<'_, ProjectSession> {
    // Current-project workers only borrow the session for an immutable query.
    // Recovering this owner after an unwinding query cannot expose a partial
    // session mutation; mutation paths must not use poison recovery.
    session
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        remediation: remediation.map(scrub_user_message),
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
    let words = message.split_whitespace().collect::<Vec<_>>();
    if words.iter().any(|word| {
        let trimmed = word.trim_matches(|character: char| ",.;:()[]{}\"'".contains(character));
        trimmed.starts_with("\\\\")
    }) {
        return "The desktop command failed while accessing a local project.".to_owned();
    }
    if words.iter().any(|word| is_absolute_path(word)) {
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
        if !state.jobs.is_empty() {
            return Err(error(
                "resource_limit",
                "Another project operation is already running.",
                Some("Wait for the current project operation to finish."),
                true,
            ));
        }
        return Ok(CurrentProjectResponse {
            schema: IPC_SCHEMA,
            state: "no_project",
            project: None,
            capabilities: capabilities(),
        });
    };
    let control = Arc::new(JobControl::new());
    let session = lock_session_for_query(session);
    response_for_with_cancel(&session, &mut JobCancellation::new(control))
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
    state.session = Some(Arc::new(Mutex::new(session)));
    current_project(state)
}

pub fn open_project_at(
    state: &mut DesktopState,
    path: PathBuf,
    mode: SessionMode,
) -> Result<CurrentProjectResponse, DesktopIpcError> {
    let session = state.application.open(OpenProject { path, mode })?;
    state.session = Some(Arc::new(Mutex::new(session)));
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

/// Move a project that was created by a cancelled operation into the owning
/// recovery bin. Keeping the bundle makes cancellation recoverable while
/// avoiding a half-admitted project at the path chosen by the application.
pub fn retain_cancelled_project(path: &std::path::Path) -> Result<PathBuf, DesktopIpcError> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let parent = path.parent().ok_or_else(|| {
        error(
            "storage",
            "The cancelled project could not be retained safely.",
            Some("Retry the operation and keep the application data directory writable."),
            true,
        )
    })?;
    let trash = parent.join(".trash");
    std::fs::create_dir_all(&trash).map_err(|value| {
        error(
            "storage",
            format!("Could not prepare cancellation recovery storage: {value}"),
            Some("Keep the application data directory writable and retry."),
            true,
        )
    })?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("cancelled-project.rfatlas");
    let destination = trash.join(format!("cancelled-{}-{name}", uuid::Uuid::new_v4()));
    std::fs::rename(path, &destination).map_err(|value| {
        error(
            "storage",
            format!("Could not retain the cancelled project bundle: {value}"),
            Some("Keep the application data directory writable and retry."),
            true,
        )
    })?;
    Ok(destination)
}

pub fn cancel_created_project(
    path: &std::path::Path,
    control: &JobControl,
) -> Result<(), DesktopIpcError> {
    if !control.is_cancelled() {
        return Ok(());
    }
    let _retained_path = retain_cancelled_project(path)?;
    Err(cancelled_error())
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

/// Release a create job and retain a bundle when its worker unwinds after the
/// application has created it. The active session is intentionally untouched;
/// callers publish a new session only after this boundary returns success.
pub fn finish_created_project_job<T, E>(
    state: &mut DesktopState,
    job_id: &str,
    created_path: &std::path::Path,
    joined: Result<T, E>,
) -> Result<T, DesktopIpcError> {
    match finish_joined_job(state, job_id, joined) {
        Ok(value) => Ok(value),
        Err(join_error) => {
            let _retained_path = retain_cancelled_project(created_path)?;
            Err(join_error)
        }
    }
}

async fn current_project_job_with<F>(
    state: &Mutex<DesktopState>,
    job_id: String,
    worker: F,
) -> Result<CurrentProjectResponse, DesktopIpcError>
where
    F: FnOnce(
            Arc<Mutex<ProjectSession>>,
            Arc<JobControl>,
        ) -> Result<CurrentProjectResponse, DesktopIpcError>
        + Send
        + 'static,
{
    let (control, session) = {
        let mut guard = state.lock().map_err(|_| {
            error(
                "storage",
                "The desktop project session is unavailable.",
                Some("Restart RF Atlas."),
                true,
            )
        })?;
        let Some(session) = guard.session() else {
            if !guard.jobs.is_empty() {
                return Err(error(
                    "resource_limit",
                    "Another project operation is already running.",
                    Some("Wait for the current project operation to finish."),
                    true,
                ));
            }
            return Ok(no_project_response());
        };
        let control = begin_job(&mut guard, &job_id)?;
        (control, session)
    };
    let joined = tauri::async_runtime::spawn_blocking(move || worker(session, control)).await;
    let mut guard = state.lock().map_err(|_| {
        error(
            "storage",
            "The desktop project session is unavailable.",
            Some("Restart RF Atlas."),
            true,
        )
    })?;
    finish_joined_job(&mut guard, &job_id, joined)?
}

pub async fn current_project_job(
    state: &Mutex<DesktopState>,
    job_id: String,
) -> Result<CurrentProjectResponse, DesktopIpcError> {
    current_project_job_with(state, job_id, |session, control| {
        let mut cancellation = JobCancellation::new(control);
        cancellation.control().set_progress(25);
        let session = lock_session_for_query(&session);
        let response = response_for_with_cancel(&session, &mut cancellation)?;
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        cancellation.control().set_progress(100);
        Ok(response)
    })
    .await
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
    fn current_query_reports_busy_while_create_or_open_is_admitted() {
        let mut state = DesktopState::default();
        let job_id = uuid::Uuid::new_v4().to_string();
        begin_job(&mut state, &job_id).expect("admit operation");
        assert_eq!(
            current_project(&state)
                .expect_err("current query must not hide an active operation")
                .code,
            "resource_limit"
        );
        finish_job(&mut state, &job_id);
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
    fn post_create_worker_panic_retains_exact_bundle_and_preserves_session() {
        use std::panic::AssertUnwindSafe;

        let mut state = DesktopState::default();
        let active_id = create_project_at(
            &mut state,
            retained_path("active-before-create-panic"),
            "Active before create panic".into(),
            1_800_000_000_000,
        )
        .expect("active project")
        .project
        .expect("active project summary")
        .project_id;
        let created_path = retained_path("post-create-panic");
        let created_name = created_path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("created project name")
            .to_owned();
        let expected_manifest = Arc::new(Mutex::new(None::<Vec<u8>>));
        let manifest_slot = Arc::clone(&expected_manifest);
        let panic_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let name =
                kyberia_domain::identity::Text::new("Panic project".to_owned()).expect("name");
            let session = Application
                .create(CreateProject {
                    path: created_path.clone(),
                    name,
                    created_utc_ms: 1_800_000_000_001,
                })
                .expect("created bundle");
            *manifest_slot.lock().expect("manifest slot") =
                Some(std::fs::read(created_path.join("manifest.json")).expect("manifest"));
            drop(session);
            panic!("injected post-create panic");
        }));
        assert!(panic_result.is_err());
        let expected_manifest = expected_manifest
            .lock()
            .expect("manifest slot")
            .clone()
            .expect("manifest captured before panic");

        let job_id = uuid::Uuid::new_v4().to_string();
        begin_job(&mut state, &job_id).expect("admit create");
        let result =
            finish_created_project_job(&mut state, &job_id, &created_path, Err::<(), ()>(()));
        assert_eq!(result.expect_err("join failure").code, "storage");
        assert!(!created_path.exists());
        let entries = std::fs::read_dir(
            created_path
                .parent()
                .expect("created project parent")
                .join(".trash"),
        )
        .expect("recovery trash")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|value| value.ends_with(&created_name))
        })
        .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let retained = entries[0].path();
        assert!(
            retained
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.ends_with(&created_name))
        );
        assert_eq!(
            std::fs::read(retained.join("manifest.json")).expect("retained manifest"),
            expected_manifest
        );
        assert!(state.jobs.is_empty());
        assert_eq!(
            current_project(&state)
                .expect("old session remains")
                .project
                .expect("old project")
                .project_id,
            active_id
        );
    }

    #[test]
    fn current_project_join_failure_preserves_session_and_releases_admission() {
        let state = Mutex::new(DesktopState::default());
        let active_id = {
            let mut guard = state.lock().expect("state");
            create_project_at(
                &mut guard,
                retained_path("current-join-failure"),
                "Current before panic".into(),
                1_800_000_000_000,
            )
            .expect("active project")
            .project
            .expect("project")
            .project_id
        };
        let failed_id = uuid::Uuid::new_v4().to_string();
        let failure = tauri::async_runtime::block_on(current_project_job_with(
            &state,
            failed_id,
            |session, _control| {
                let _session = session
                    .lock()
                    .expect("unpoisoned session before injected panic");
                panic!("injected current-project worker failure")
            },
        ))
        .expect_err("join failure");
        assert_eq!(failure.code, "storage");
        assert!(failure.retryable);

        let mut guard = state.lock().expect("state after join failure");
        assert!(guard.jobs.is_empty());
        assert_eq!(
            current_project(&guard)
                .expect("prior session remains queryable")
                .project
                .expect("same project")
                .project_id,
            active_id
        );
        let next_id = uuid::Uuid::new_v4().to_string();
        assert!(begin_job(&mut guard, &next_id).is_ok());
    }

    #[test]
    fn cancellation_during_atomic_create_retains_bundle_and_keeps_session_recoverable() {
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
        let cancelled_name = cancelled_path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("cancelled project name")
            .to_owned();
        let worker_path = cancelled_path.clone();
        let job_id = uuid::Uuid::new_v4().to_string();
        let control = begin_job(&mut state, &job_id).expect("job");
        let task_control = Arc::clone(&control);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let worker = std::thread::spawn(move || {
            entered_tx.send(()).expect("entered");
            release_rx.recv().expect("release");
            let name =
                kyberia_domain::identity::Text::new("Cancelled project".to_owned()).expect("name");
            let session = Application
                .create(CreateProject {
                    path: worker_path.clone(),
                    name,
                    created_utc_ms: 1_800_000_000_001,
                })
                .map_err(DesktopIpcError::from)?;
            drop(session);
            let manifest = std::fs::read(worker_path.join("manifest.json")).expect("manifest");
            let cancellation = cancel_created_project(&worker_path, &task_control)
                .expect_err("cancelled after atomic work");
            Ok::<_, DesktopIpcError>((cancellation, manifest))
        });
        entered_rx.recv().expect("worker entered atomic step");
        cancel_job(&state, &job_id).expect("cancel");
        release_tx.send(()).expect("release worker");
        let (completed_path, expected_manifest) = worker
            .join()
            .expect("worker join")
            .expect("worker completes cancellation");
        assert_eq!(completed_path.code, "cancelled");
        finish_job(&mut state, &job_id);

        assert!(!cancelled_path.exists());
        let entries = cancelled_path
            .parent()
            .expect("cancelled project parent")
            .join(".trash")
            .read_dir()
            .expect("recovery trash")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|value| value.ends_with(&cancelled_name))
            })
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        let retained = entries[0].path();
        assert!(
            retained
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.ends_with(&cancelled_name))
        );
        assert_eq!(
            std::fs::read(retained.join("manifest.json")).expect("retained manifest"),
            expected_manifest
        );

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

    #[test]
    fn renderer_errors_scrub_unc_paths_and_remediation() {
        let value = error(
            "storage",
            r#"could not open \\server\share\rf atlas.rfatlas"#,
            Some(r#"Check \\server\share\rf atlas.rfatlas"#),
            true,
        );
        assert!(!value.message.contains("server"));
        assert!(!value.remediation.expect("remediation").contains("server"));
    }
}
