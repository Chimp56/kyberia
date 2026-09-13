use kyberia_application::{
    Application, ApplicationError, CreateProject, OpenProject, ProjectQuery, ProjectQueryResult,
    ProjectSession, ProjectState, SessionMode,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const IPC_SCHEMA: &str = "kyberia.desktop-ipc/1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBlankProjectRequest {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub schema: String,
    pub path: String,
    pub name: String,
    pub created_utc_ms: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectRequest {
    pub schema: String,
    pub path: String,
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct SchemaRequest {
    pub schema: String,
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
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentProjectResponse {
    pub schema: &'static str,
    pub state: &'static str,
    pub project: Option<ProjectSummary>,
    pub capabilities: Vec<CapabilitySummary>,
}

pub struct DesktopState {
    application: Application,
    session: Option<ProjectSession>,
}

impl Default for DesktopState {
    fn default() -> Self {
        Self {
            application: Application,
            session: None,
        }
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
        message: message.into(),
        remediation: remediation.map(str::to_owned),
        retryable,
    }
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

fn response_for(
    session: Option<&ProjectSession>,
) -> Result<CurrentProjectResponse, DesktopIpcError> {
    let Some(session) = session else {
        return Ok(CurrentProjectResponse {
            schema: IPC_SCHEMA,
            state: "no_project",
            project: None,
            capabilities: capabilities(),
        });
    };
    let ProjectQueryResult::CurrentSnapshot(view) = session.query(ProjectQuery::CurrentSnapshot)?;
    let state = match view.state() {
        ProjectState::MaterializedCurrent => "materialized_current",
        ProjectState::BaselineOnly => "baseline_only",
        ProjectState::LegacyAbsent => "legacy_absent",
    };
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
    });
    Ok(CurrentProjectResponse {
        schema: IPC_SCHEMA,
        state,
        project,
        capabilities: capabilities(),
    })
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
    response_for(state.session.as_ref())
}

pub fn open_project_at(
    state: &mut DesktopState,
    path: PathBuf,
    mode: SessionMode,
) -> Result<CurrentProjectResponse, DesktopIpcError> {
    let session = state.application.open(OpenProject { path, mode })?;
    state.session = Some(session);
    response_for(state.session.as_ref())
}

pub fn current_project(state: &DesktopState) -> Result<CurrentProjectResponse, DesktopIpcError> {
    response_for(state.session.as_ref())
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
    Ok(app_data_dir.join(format!("rf-atlas-{}.rfatlas", uuid::Uuid::new_v4())))
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
        let result = require_schema("kyberia.desktop-ipc/0");
        assert_eq!(
            result.expect_err("version must be rejected").code,
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
            response.project.as_ref().map(|project| project.revision),
            Some(0)
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
    fn blank_paths_are_unique_and_scoped_to_app_data() {
        let root = retained_path("app-data")
            .parent()
            .expect("parent")
            .to_path_buf();
        let first = blank_project_path(root.clone()).expect("first path");
        let second = blank_project_path(root).expect("second path");
        assert_ne!(first, second);
        assert!(first.starts_with(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.trash")));
    }
}
