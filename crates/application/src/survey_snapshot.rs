//! Application-owned projections for durable point-survey snapshots.
//!
//! The bundle adapter remains private; callers exchange canonical survey
//! state and typed identities, never project-store records or storage errors.

use kyberia_domain::identity::{CollectorId, ContentHash, SessionId, SnapshotId, SourceId};
use kyberia_project_store::{
    LoadedSurveySnapshot, SurveySnapshotHistory, SurveySnapshotHistoryCursor,
    SurveySnapshotHistoryPage as StoreSurveySnapshotHistoryPage,
    SurveySnapshotHistoryPageLimits as StoreSurveySnapshotHistoryPageLimits, SurveySnapshotRecord,
};
use kyberia_survey::{
    PointId, PointSnapshotDecodeReceipt, PointSnapshotInputVersion, PointSnapshotSchemaVersion,
    PointSurvey,
};

use crate::error::{ApplicationError, ErrorKind};

/// Save one immutable, already admitted point-survey state.
#[derive(Clone, Debug, PartialEq)]
pub struct PointSurveySnapshotRequest {
    pub snapshot_id: SnapshotId,
    pub survey: PointSurvey,
    pub committed_utc_ms: i64,
    /// Optional optimistic concurrency check against the bundle revision.
    pub expected_bundle_revision: Option<u64>,
}

/// Application-owned identity and commit metadata for one saved snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointSurveySnapshotReceipt {
    snapshot_id: SnapshotId,
    session_id: SessionId,
    point_id: PointId,
    source_id: SourceId,
    collector_id: CollectorId,
    artifact_hash: ContentHash,
    input_schema_version: PointSnapshotInputVersion,
    output_schema_version: PointSnapshotSchemaVersion,
    decoder_version: &'static str,
    bundle_revision: u64,
    committed_utc_ms: i64,
}

impl PointSurveySnapshotReceipt {
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    pub const fn point_id(&self) -> PointId {
        self.point_id
    }
    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }
    pub const fn collector_id(&self) -> CollectorId {
        self.collector_id
    }
    pub const fn artifact_hash(&self) -> ContentHash {
        self.artifact_hash
    }
    pub const fn input_schema_version(&self) -> PointSnapshotInputVersion {
        self.input_schema_version
    }
    pub const fn output_schema_version(&self) -> PointSnapshotSchemaVersion {
        self.output_schema_version
    }
    pub const fn decoder_version(&self) -> &'static str {
        self.decoder_version
    }
    pub const fn bundle_revision(&self) -> u64 {
        self.bundle_revision
    }
    pub const fn committed_utc_ms(&self) -> i64 {
        self.committed_utc_ms
    }
}

/// A complete validated survey read back from the bundle.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedPointSurveySnapshot {
    receipt: PointSurveySnapshotReceipt,
    survey: PointSurvey,
    decode_receipt: PointSnapshotDecodeReceipt,
}

impl LoadedPointSurveySnapshot {
    pub const fn receipt(&self) -> &PointSurveySnapshotReceipt {
        &self.receipt
    }
    pub const fn survey(&self) -> &PointSurvey {
        &self.survey
    }
    pub const fn decode_receipt(&self) -> &PointSnapshotDecodeReceipt {
        &self.decode_receipt
    }
}

/// One validated entry from the append-only snapshot history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointSurveySnapshotHistoryEntry {
    receipt: PointSurveySnapshotReceipt,
}

impl PointSurveySnapshotHistoryEntry {
    pub const fn receipt(&self) -> &PointSurveySnapshotReceipt {
        &self.receipt
    }
}

/// Opaque continuation position for append-only point-survey history. It is
/// pinned to the project/filter/high-water revision from the first page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointSurveySnapshotHistoryCursor {
    pub(crate) inner: SurveySnapshotHistoryCursor,
}

/// Caller-selectable per-page ceilings. Values may be lowered from defaults;
/// values above the hard caps or zero are rejected as invalid requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointSurveySnapshotHistoryPageLimits {
    pub max_items: usize,
    pub max_artifact_bytes: u64,
    pub max_work_units: u64,
}

impl Default for PointSurveySnapshotHistoryPageLimits {
    fn default() -> Self {
        Self {
            max_items: kyberia_project_store::MAX_SURVEY_SNAPSHOT_HISTORY_PAGE_ITEMS,
            max_artifact_bytes: kyberia_project_store::MAX_SURVEY_SNAPSHOT_HISTORY_PAGE_BYTES,
            max_work_units: kyberia_project_store::MAX_SURVEY_SNAPSHOT_HISTORY_PAGE_WORK_UNITS,
        }
    }
}

/// A fully validated page. Its entries have replay-validated artifacts; the
/// complete index/history and artifact-registry metadata inventory was also
/// checked, while artifact bytes outside this page were not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointSurveySnapshotHistoryPage {
    entries: Vec<PointSurveySnapshotHistoryEntry>,
    next_cursor: Option<PointSurveySnapshotHistoryCursor>,
}

impl PointSurveySnapshotHistoryPage {
    pub fn entries(&self) -> &[PointSurveySnapshotHistoryEntry] {
        &self.entries
    }

    pub fn next_cursor(&self) -> Option<&PointSurveySnapshotHistoryCursor> {
        self.next_cursor.as_ref()
    }
}

pub(crate) fn receipt_from_store(
    record: SurveySnapshotRecord,
) -> Result<PointSurveySnapshotReceipt, ApplicationError> {
    receipt(
        record.snapshot_id,
        record.session_id,
        record.point_id,
        record.source_id,
        record.collector_id,
        &record.artifact_hash,
        record.input_schema_version,
        record.output_schema_version,
        record.decoder_version,
        record.revision,
        record.created_utc_ms,
    )
}

pub(crate) fn loaded_from_store(
    loaded: LoadedSurveySnapshot,
) -> Result<LoadedPointSurveySnapshot, ApplicationError> {
    let receipt = receipt_from_store(loaded.record)?;
    Ok(LoadedPointSurveySnapshot {
        receipt,
        survey: loaded.survey,
        decode_receipt: loaded.decode_receipt,
    })
}

pub(crate) fn history_from_store(
    history: SurveySnapshotHistory,
) -> Result<PointSurveySnapshotHistoryEntry, ApplicationError> {
    let receipt = receipt(
        history.snapshot_id,
        history.session_id,
        history.point_id,
        history.source_id,
        history.collector_id,
        &history.artifact_hash,
        history.input_schema_version,
        history.output_schema_version,
        history.decoder_version,
        history.revision,
        history.committed_utc_ms,
    )?;
    Ok(PointSurveySnapshotHistoryEntry { receipt })
}

pub(crate) fn history_page_from_store(
    page: StoreSurveySnapshotHistoryPage,
) -> Result<PointSurveySnapshotHistoryPage, ApplicationError> {
    let entries = page
        .entries()
        .iter()
        .cloned()
        .map(history_from_store)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PointSurveySnapshotHistoryPage {
        entries,
        next_cursor: page
            .next_cursor()
            .cloned()
            .map(|inner| PointSurveySnapshotHistoryCursor { inner }),
    })
}

pub(crate) fn store_history_page_limits(
    limits: PointSurveySnapshotHistoryPageLimits,
) -> StoreSurveySnapshotHistoryPageLimits {
    StoreSurveySnapshotHistoryPageLimits {
        max_items: limits.max_items,
        max_artifact_bytes: limits.max_artifact_bytes,
        max_work_units: limits.max_work_units,
    }
}

#[allow(clippy::too_many_arguments)]
fn receipt(
    snapshot_id: SnapshotId,
    session_id: SessionId,
    point_id: PointId,
    source_id: SourceId,
    collector_id: CollectorId,
    artifact_hash: &str,
    input_schema_version: PointSnapshotInputVersion,
    output_schema_version: PointSnapshotSchemaVersion,
    decoder_version: &'static str,
    bundle_revision: u64,
    committed_utc_ms: i64,
) -> Result<PointSurveySnapshotReceipt, ApplicationError> {
    let artifact_hash = ContentHash::try_from(artifact_hash.to_owned()).map_err(|error| {
        ApplicationError::new(
            ErrorKind::CorruptProject,
            format!("invalid survey snapshot content hash: {error}"),
        )
    })?;
    Ok(PointSurveySnapshotReceipt {
        snapshot_id,
        session_id,
        point_id,
        source_id,
        collector_id,
        artifact_hash,
        input_schema_version,
        output_schema_version,
        decoder_version,
        bundle_revision,
        committed_utc_ms,
    })
}
