use crate::{error::ApplicationError, port::BundleProjectStore, session::ProjectSession};
use kyberia_domain::identity::Text;
use std::path::PathBuf;

/// Whether a session may be used for future commands or only queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionMode {
    ReadOnly,
    ReadWrite,
}

/// Create a new canonical project bundle and register its immutable baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateProject {
    pub path: PathBuf,
    pub name: Text,
    pub created_utc_ms: i64,
}

/// Open one existing canonical project bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenProject {
    pub path: PathBuf,
    pub mode: SessionMode,
}

/// Mutating application lifecycle operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    CreateProject(CreateProject),
    OpenProject(OpenProject),
}

/// Successful command output. A session hides all storage adapter objects.
pub enum CommandResult {
    ProjectSession(ProjectSession),
}

pub(crate) fn execute(command: Command) -> Result<CommandResult, ApplicationError> {
    match command {
        Command::CreateProject(request) => {
            let CreateProject {
                path,
                name,
                created_utc_ms,
            } = request;
            if created_utc_ms < 0 {
                return Err(ApplicationError::new(
                    crate::ErrorKind::InvalidRequest,
                    "created_utc_ms must be nonnegative",
                ));
            }
            let project_id =
                kyberia_domain::identity::ProjectId::from_bytes(*uuid::Uuid::new_v4().as_bytes())
                    .map_err(|error| {
                    ApplicationError::new(
                        crate::ErrorKind::InvalidRequest,
                        format!("generated project identity was invalid: {error}"),
                    )
                })?;
            let mut store =
                BundleProjectStore::create(&path, project_id, name.clone().into(), created_utc_ms)?;
            store.register_baseline(
                &kyberia_domain::project::Project::new(project_id, name),
                created_utc_ms,
            )?;
            Ok(CommandResult::ProjectSession(ProjectSession::new(
                store,
                SessionMode::ReadWrite,
            )))
        }
        Command::OpenProject(request) => {
            let OpenProject { path, mode } = request;
            let store = BundleProjectStore::open(&path, mode)?;
            Ok(CommandResult::ProjectSession(ProjectSession::new(
                store, mode,
            )))
        }
    }
}
