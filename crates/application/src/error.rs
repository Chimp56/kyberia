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
        StoreError::Corrupt(message) => {
            if is_resource_limit(&message) {
                ApplicationError::new(ErrorKind::ResourceLimit, message)
            } else {
                ApplicationError::new(ErrorKind::CorruptProject, message)
            }
        }
        StoreError::UnsupportedVersion(version) | StoreError::UnsupportedChunkVersion(version) => {
            ApplicationError::new(
                ErrorKind::UnsupportedVersion,
                format!("unsupported project schema or chunk version {version}"),
            )
        }
        StoreError::Operation(message) => map_content_error(context, message),
        StoreError::ChunkCodec(message) => map_content_error(context, message),
        StoreError::Materialization(error) => map_publication_error(context, error),
        StoreError::Sql(error) => ApplicationError::new(
            // SQL failures while opening or querying an admitted bundle mean
            // its contents/schema cannot be trusted. Creation failures remain
            // an adapter/storage failure because no project was admitted yet.
            if matches!(context, StoreContext::Open | StoreContext::Query) {
                ErrorKind::CorruptProject
            } else {
                ErrorKind::Storage
            },
            format!("project database: {error}"),
        ),
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
    if is_resource_limit(&message) {
        ApplicationError::new(ErrorKind::ResourceLimit, message)
    } else {
        // Store validation runs after application admission. Its invalid
        // values are malformed or unsupported persisted contents at this
        // boundary, rather than a new caller request.
        let _ = context;
        ApplicationError::new(ErrorKind::CorruptProject, message)
    }
}

fn map_content_error(context: StoreContext, message: String) -> ApplicationError {
    if is_resource_limit(&message) {
        ApplicationError::new(ErrorKind::ResourceLimit, message)
    } else if matches!(context, StoreContext::Open | StoreContext::Query) {
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
        PublicationError::Corrupt(_) => {
            if is_resource_limit(&message) {
                ApplicationError::new(ErrorKind::ResourceLimit, message)
            } else {
                ApplicationError::new(ErrorKind::CorruptProject, message)
            }
        }
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

fn is_resource_limit(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "budget",
        "resource limit",
        "exceeds",
        "oversized",
        "too many",
        "limit",
    ]
    .iter()
    .any(|marker| message.contains(marker))
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
