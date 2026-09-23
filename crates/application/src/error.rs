use kyberia_project_store::{PublicationError, StoreError};
use std::{fmt, io, path::Path};

/// Stable categories exposed by the application boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    MissingProject,
    ProjectAlreadyExists,
    InvalidRequest,
    Prerequisite,
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
            Self::Prerequisite => "prerequisite",
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

/// The same store error has different application meaning depending on which
/// boundary operation observed it. In particular, only the requested root's
/// absence is `MissingProject`; missing files inside an existing root are
/// corruption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreContext {
    Create,
    Open,
    Query,
    Baseline,
    Mutation,
}

pub(crate) fn map_store_error(context: StoreContext, error: StoreError) -> ApplicationError {
    match error {
        StoreError::Io(error) => map_io(context, error),
        StoreError::Json(error) => ApplicationError::new(
            ErrorKind::CorruptProject,
            format!("invalid project JSON: {error}"),
        ),
        StoreError::Invalid(message) => map_invalid(context, message),
        StoreError::ReadOnly => {
            ApplicationError::new(ErrorKind::ReadOnly, "project session is read-only")
        }
        StoreError::Cancelled => {
            ApplicationError::new(ErrorKind::Cancelled, "project operation cancelled")
        }
        StoreError::Corrupt(message) => ApplicationError::new(ErrorKind::CorruptProject, message),
        StoreError::UnsupportedVersion(version) | StoreError::UnsupportedChunkVersion(version) => {
            ApplicationError::new(
                ErrorKind::UnsupportedVersion,
                format!("unsupported project schema or chunk version {version}"),
            )
        }
        StoreError::Operation(message) => map_content_error(context, message),
        StoreError::ChunkCodec(message) => map_content_error(context, message),
        StoreError::Materialization(error) => map_publication_error(context, error),
        StoreError::Sql(error) => map_sql_error(error),
    }
}

fn map_io(context: StoreContext, error: io::Error) -> ApplicationError {
    let kind = match error.kind() {
        io::ErrorKind::AlreadyExists if context == StoreContext::Create => {
            ErrorKind::ProjectAlreadyExists
        }
        io::ErrorKind::NotFound if matches!(context, StoreContext::Open | StoreContext::Query) => {
            ErrorKind::CorruptProject
        }
        _ => ErrorKind::Storage,
    };
    ApplicationError::new(kind, format!("project I/O: {error}"))
}

fn map_invalid(context: StoreContext, message: String) -> ApplicationError {
    if context == StoreContext::Mutation
        && (message.starts_with("stale operation project revision")
            || message.starts_with("stale project revision:"))
    {
        return ApplicationError::new(ErrorKind::Conflict, message);
    }
    if matches!(context, StoreContext::Open | StoreContext::Query) {
        ApplicationError::new(ErrorKind::CorruptProject, message)
    } else {
        // Store validation runs after application admission. Its invalid
        // values are a failed internal persistence operation, rather than a
        // new caller request.
        ApplicationError::new(ErrorKind::Storage, message)
    }
}

fn map_content_error(context: StoreContext, message: String) -> ApplicationError {
    if matches!(context, StoreContext::Open | StoreContext::Query) {
        ApplicationError::new(ErrorKind::CorruptProject, message)
    } else {
        ApplicationError::new(ErrorKind::Storage, message)
    }
}

fn map_publication_error(context: StoreContext, error: PublicationError) -> ApplicationError {
    let message = error.to_string();
    match error {
        PublicationError::ResourceLimit(_) => {
            ApplicationError::new(ErrorKind::ResourceLimit, message)
        }
        PublicationError::StaleOperationRevision { .. }
        | PublicationError::ConflictingCurrentPublication => {
            ApplicationError::new(ErrorKind::Conflict, message)
        }
        PublicationError::Corrupt(_) => ApplicationError::new(ErrorKind::CorruptProject, message),
        PublicationError::Invalid(_)
        | PublicationError::WrongProject
        | PublicationError::InputIdentityMismatch
        | PublicationError::BaselineNotRegistered
        | PublicationError::Identity(_) => {
            // These variants can only be raised by a store operation after
            // the application has admitted its request. Treat them as a bad
            // bundle or failed internal persistence, never as caller input.
            if matches!(context, StoreContext::Open | StoreContext::Query) {
                ApplicationError::new(ErrorKind::CorruptProject, message)
            } else {
                ApplicationError::new(ErrorKind::Storage, message)
            }
        }
        PublicationError::TestFault => ApplicationError::new(ErrorKind::Storage, message),
    }
}

fn map_sql_error(error: rusqlite::Error) -> ApplicationError {
    let kind = match error.sqlite_error_code() {
        Some(
            rusqlite::ErrorCode::OperationInterrupted
            | rusqlite::ErrorCode::OutOfMemory
            | rusqlite::ErrorCode::DiskFull
            | rusqlite::ErrorCode::TooBig,
        ) => ErrorKind::ResourceLimit,
        Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
            ErrorKind::CorruptProject
        }
        Some(_) | None => ErrorKind::Storage,
    };
    ApplicationError::new(kind, format!("project database: {error}"))
}

pub(crate) fn map_budget_error(
    error: kyberia_resource_budget::ResourceBudgetError,
) -> ApplicationError {
    match error {
        kyberia_resource_budget::ResourceBudgetError::Cancelled => ApplicationError::new(
            ErrorKind::Cancelled,
            "project query resource budget cancelled",
        ),
        kyberia_resource_budget::ResourceBudgetError::LimitExceeded(limit) => {
            ApplicationError::new(
                ErrorKind::ResourceLimit,
                format!("project query resource limit: {}", limit.kind().label()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_failure(code: i32) -> StoreError {
        StoreError::Sql(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            None,
        ))
    }

    #[test]
    fn typed_sql_codes_map_resource_storage_and_corruption_categories() {
        for code in [
            rusqlite::ffi::SQLITE_INTERRUPT,
            rusqlite::ffi::SQLITE_NOMEM,
            rusqlite::ffi::SQLITE_FULL,
            rusqlite::ffi::SQLITE_TOOBIG,
        ] {
            assert_eq!(
                map_store_error(StoreContext::Query, sqlite_failure(code)).kind(),
                ErrorKind::ResourceLimit
            );
        }
        for code in [
            rusqlite::ffi::SQLITE_BUSY,
            rusqlite::ffi::SQLITE_LOCKED,
            rusqlite::ffi::SQLITE_IOERR,
        ] {
            assert_eq!(
                map_store_error(StoreContext::Query, sqlite_failure(code)).kind(),
                ErrorKind::Storage
            );
        }
        for code in [rusqlite::ffi::SQLITE_CORRUPT, rusqlite::ffi::SQLITE_NOTADB] {
            assert_eq!(
                map_store_error(StoreContext::Query, sqlite_failure(code)).kind(),
                ErrorKind::CorruptProject
            );
        }
    }

    #[test]
    fn publication_corruption_message_never_changes_its_category() {
        let error = map_store_error(
            StoreContext::Query,
            StoreError::Materialization(PublicationError::Corrupt(
                "publication revision exceeds committed manifest resource limit".into(),
            )),
        );
        assert_eq!(error.kind(), ErrorKind::CorruptProject);
    }
}
