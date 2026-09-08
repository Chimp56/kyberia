//! Transactional directory bundles. SQLite owns the committed manifest; the
//! human-readable manifest is a recoverable projection, never a second truth.
mod bundle;
mod manifest;
mod sqlite_guard;
mod survey_snapshot;

pub use bundle::{Bundle, OpenMode, Verification};
pub use manifest::{ArtifactEntry, ArtifactKind, BundleManifest, content_hash};
pub use survey_snapshot::MAX_SURVEY_SNAPSHOT_BYTES;
pub use survey_snapshot::{LoadedSurveySnapshot, SurveySnapshotHistory, SurveySnapshotRecord};

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    Invalid(String),
    ReadOnly,
    Corrupt(String),
    UnsupportedVersion(u32),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "project I/O: {e}"),
            Self::Sql(e) => write!(f, "project database: {e}"),
            Self::Json(e) => write!(f, "project manifest JSON: {e}"),
            Self::Invalid(e) => write!(f, "invalid project input: {e}"),
            Self::ReadOnly => write!(f, "project is read-only"),
            Self::Corrupt(e) => write!(f, "project integrity failure: {e}"),
            Self::UnsupportedVersion(v) => write!(
                f,
                "unsupported project schema {v}; open read-only for metadata inspection"
            ),
        }
    }
}

impl std::error::Error for StoreError {}
impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
pub type Result<T> = std::result::Result<T, StoreError>;
