//! Inward-facing application commands and queries for canonical projects.
//!
//! The application boundary owns project-session lifecycle orchestration. Its
//! public API contains canonical domain values and application-owned views;
//! filesystem, SQLite, and materialization-publication adapter values remain
//! private to the production store adapter.

mod command;
mod error;
mod port;
mod query;
mod session;

pub use command::{Command, CommandResult, CreateProject, OpenProject, SessionMode};
pub use error::{ApplicationError, ErrorKind};
pub use port::ProjectStorePort;
pub use query::{
    CurrentProjectView, ProjectQuery, ProjectQueryResult, ProjectRevision, ProjectState,
};
pub use session::ProjectSession;

/// Stateless entry point for application use cases.
#[derive(Clone, Copy, Debug, Default)]
pub struct Application;

impl Application {
    /// Execute one typed lifecycle command.
    pub fn execute(&self, command: Command) -> Result<CommandResult, ApplicationError> {
        command::execute(command)
    }

    /// Create a project and return its writable session.
    pub fn create(&self, request: CreateProject) -> Result<ProjectSession, ApplicationError> {
        match self.execute(Command::CreateProject(request))? {
            CommandResult::ProjectSession(session) => Ok(session),
        }
    }

    /// Open an existing project and return its session.
    pub fn open(&self, request: OpenProject) -> Result<ProjectSession, ApplicationError> {
        match self.execute(Command::OpenProject(request))? {
            CommandResult::ProjectSession(session) => Ok(session),
        }
    }
}
