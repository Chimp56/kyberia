use kyberia_project_store::{PublicationError, StoreError};
use std::{fmt, io, path::Path};

/// Stable categories exposed by the application boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    MissingProject,
    ProjectAlreadyExists,
    InvalidRequest,
    CorruptProject,
    UnsupportedVersion,
    ResourceLimit,
    Cancelled,
    ReadOnly,
    Conflict,
    Storage,
}

/// Structured lifecycle/query failure with adapter details retained only as
/// diagnostic text. No SQLite or adapter error type crosses this boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationError {
    kind: ErrorKind,
    message: String,
}

impl ApplicationError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn missing_project(path: &Path) -> Self {
        Self::new(
            ErrorKind::MissingProject,
            format!("project does not exist: {}", path.display()),
        )
    }

    pub(crate) fn already_exists(path: &Path) -> Self {
        Self::new(
            ErrorKind::ProjectAlreadyExists,
            format!("project already exists: {}", path.display()),
        )
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for ApplicationError {}

impl ErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingProject => "missing_project",
            Self::ProjectAlreadyExists => "project_already_exists",
            Self::InvalidRequest => "invalid_request",
            Self::CorruptProject => "corrupt_project",
            Self::UnsupportedVersion => "unsupported_version",
            Self::ResourceLimit => "resource_limit",
            Self::Cancelled => "cancelled",
            Self::ReadOnly => "read_only",
            Self::Conflict => "conflict",
            Self::Storage => "storage",
        }
    }
}

impl From<StoreError> for ApplicationError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Io(error) => match error.kind() {
                io::ErrorKind::NotFound => Self::new(
                    ErrorKind::MissingProject,
                    "project path or required project file was not found",
                ),
                io::ErrorKind::AlreadyExists => Self::new(
                    ErrorKind::ProjectAlreadyExists,
                    "project path already exists",
                ),
                _ => Self::new(ErrorKind::Storage, format!("project I/O: {error}")),
            },
            StoreError::Json(error) => Self::new(
                ErrorKind::CorruptProject,
                format!("invalid project JSON: {error}"),
            ),
            StoreError::Invalid(message) => Self::new(ErrorKind::InvalidRequest, message),
            StoreError::ReadOnly => Self::new(ErrorKind::ReadOnly, "project session is read-only"),
            StoreError::Cancelled => Self::new(ErrorKind::Cancelled, "project operation cancelled"),
            StoreError::Corrupt(message) => Self::new(ErrorKind::CorruptProject, message),
            StoreError::UnsupportedVersion(version)
            | StoreError::UnsupportedChunkVersion(version) => Self::new(
                ErrorKind::UnsupportedVersion,
                format!("unsupported project schema or chunk version {version}"),
            ),
            StoreError::Operation(message) => Self::new(ErrorKind::CorruptProject, message),
            StoreError::ChunkCodec(message) => Self::new(ErrorKind::CorruptProject, message),
            StoreError::Materialization(error) => Self::from_publication(error),
            StoreError::Sql(error) => {
                Self::new(ErrorKind::Storage, format!("project database: {error}"))
            }
        }
    }
}

impl ApplicationError {
    fn from_publication(error: PublicationError) -> Self {
        let message = error.to_string();
        match error {
            PublicationError::ResourceLimit(_) => Self::new(ErrorKind::ResourceLimit, message),
            PublicationError::StaleOperationRevision { .. }
            | PublicationError::ConflictingCurrentPublication => {
                Self::new(ErrorKind::Conflict, message)
            }
            PublicationError::Corrupt(_) => Self::new(ErrorKind::CorruptProject, message),
            PublicationError::Invalid(_)
            | PublicationError::WrongProject
            | PublicationError::InputIdentityMismatch
            | PublicationError::BaselineNotRegistered
            | PublicationError::Identity(_) => Self::new(ErrorKind::InvalidRequest, message),
            PublicationError::TestFault => Self::new(ErrorKind::Storage, message),
        }
    }
}
