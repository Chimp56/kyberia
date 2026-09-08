//! Transactional directory bundles. SQLite owns the committed manifest; the
//! human-readable manifest is a recoverable projection, never a second truth.
mod bundle;
mod manifest;
mod observation_chunks;
mod operation_log;
mod parquet_codec;
mod sqlite_guard;
mod survey_snapshot;

pub use bundle::{Bundle, OpenMode, Verification};
pub use manifest::{ArtifactEntry, ArtifactKind, BundleManifest, content_hash};
pub use observation_chunks::{
    Cancellation, MAX_OBSERVATION_CHUNK_BYTES, MAX_OBSERVATION_CHUNK_ROWS,
    MAX_OBSERVATION_QUERY_BYTES, MAX_OBSERVATION_QUERY_CHUNKS, MAX_OBSERVATION_QUERY_DECODED_ROWS,
    MAX_OBSERVATION_QUERY_IDS, MAX_OBSERVATION_ROW_BYTES, NATIVE_OBSERVATION_CHUNK_MEDIA_TYPE,
    NeverCancel, OBSERVATION_CHUNK_CODEC_VERSION, OBSERVATION_CHUNK_FORMAT_VERSION,
    OBSERVATION_SCHEMA_VERSION, ObservationChunkDescriptor, ObservationChunkProvenance,
    ObservationChunkStore, ObservationQueryReceipt, ObservationQueryResult,
    PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE, PARQUET_SCHEMA_FINGERPRINT,
};
pub use operation_log::{OperationAppendOutcome, OperationStoreState};
pub use survey_snapshot::MAX_SURVEY_SNAPSHOT_BYTES;
pub use survey_snapshot::{LoadedSurveySnapshot, SurveySnapshotHistory, SurveySnapshotRecord};

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    Invalid(String),
    ReadOnly,
    Cancelled,
    Corrupt(String),
    Operation(String),
    ChunkCodec(String),
    UnsupportedVersion(u32),
    UnsupportedChunkVersion(u32),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "project I/O: {e}"),
            Self::Sql(e) => write!(f, "project database: {e}"),
            Self::Json(e) => write!(f, "project manifest JSON: {e}"),
            Self::Invalid(e) => write!(f, "invalid project input: {e}"),
            Self::ReadOnly => write!(f, "project is read-only"),
            Self::Cancelled => write!(f, "operation cancelled before publication"),
            Self::Corrupt(e) => write!(f, "project integrity failure: {e}"),
            Self::Operation(e) => write!(f, "invalid project operation: {e}"),
            Self::ChunkCodec(e) => write!(f, "observation chunk codec: {e}"),
            Self::UnsupportedVersion(v) => write!(
                f,
                "unsupported project schema {v}; open read-only for metadata inspection"
            ),
            Self::UnsupportedChunkVersion(v) => {
                write!(f, "unsupported observation chunk format/schema version {v}")
            }
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
