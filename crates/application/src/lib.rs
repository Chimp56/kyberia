//! Inward-facing application commands and queries for canonical projects.
//!
//! The application boundary owns project-session lifecycle orchestration. Its
//! public API contains canonical domain values and application-owned views;
//! filesystem, SQLite, and materialization-publication adapter values remain
//! private to the production store adapter.

mod command;
mod error;
mod map_asset;
mod map_mutation;
mod port;
mod query;
mod session;
mod survey_snapshot;

pub use command::{
    Command, CommandResult, CreateProject, CreateProjectWithInitialFloor, OpenProject, SessionMode,
};
pub use error::{ApplicationError, ErrorKind};
pub use kyberia_survey::{
    PointId, PointSnapshotDecodeReceipt, PointSnapshotInputVersion, PointSnapshotSchemaVersion,
    PointSurvey,
};
pub use map_asset::{
    AdmittedMapAsset, MAX_MAP_DIMENSION, MAX_MAP_PIXELS, MAX_MAP_SOURCE_BYTES, MAX_PNG_CHUNKS,
    MAX_PNG_METADATA_BYTES, PNG_MEDIA_TYPE, admit_map_asset, admit_map_asset_with_hints,
};
pub use map_mutation::{
    CalibrateMapIntent, CalibrateMapRequest, ImportMapIntent, ImportMapRequest, MapIntentAuthority,
    MapMutationOutcome, MapMutationReceipt, MapOperationContext,
};
pub use port::ProjectStorePort;
pub use query::{
    CurrentProjectView, ProjectQuery, ProjectQueryResult, ProjectRevision, ProjectState,
};
pub use session::ProjectSession;
pub use survey_snapshot::{
    LoadedPointSurveySnapshot, PointSurveySnapshotHistoryCursor, PointSurveySnapshotHistoryEntry,
    PointSurveySnapshotHistoryPage, PointSurveySnapshotHistoryPageLimits,
    PointSurveySnapshotReceipt, PointSurveySnapshotRequest,
};

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

    /// Create a project with the explicit canonical floor baseline needed for
    /// map import and calibration.
    pub fn create_with_initial_floor(
        &self,
        request: CreateProjectWithInitialFloor,
    ) -> Result<ProjectSession, ApplicationError> {
        match self.execute(Command::CreateProjectWithInitialFloor(Box::new(request)))? {
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
