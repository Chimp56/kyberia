//! Transactional persistence for immutable point-survey snapshots.
//!
//! A survey snapshot is a content-addressed evidence artifact. SQLite stores
//! only the project/session identity, decoder metadata and immutable history
//! needed to locate it. The JSON bytes remain in the bundle artifact store;
//! decoding is performed by `kyberia-survey`, which validates state invariants
//! and reports whether a legacy wire representation was migrated.

use crate::bundle::{atomic_projection, bounded_read, load_manifest, regular};
use crate::manifest::{MAX_ARTIFACT_BYTES, validate_hash};
use crate::{
    ArtifactEntry, ArtifactKind, Bundle, Cancellation, NeverCancel, Result, StoreError,
    content_hash, sqlite_guard,
};
use kyberia_domain::identity::{CollectorId, ProjectId, SessionId, SnapshotId, SourceId, Text};
use kyberia_survey::{
    DecodedPointSurvey, PointId, PointSnapshotDecodeReceipt, PointSnapshotInputVersion,
    PointSnapshotSchemaVersion, PointSurvey,
};
use rusqlite::{OptionalExtension, TransactionBehavior};
use std::path::Path;

/// A deliberately smaller bound than the general artifact limit. A point
/// snapshot is metadata plus bounded admitted records, so accepting a large
/// JSON document would only increase parser/resource exposure.
pub const MAX_SURVEY_SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_SURVEY_SNAPSHOTS: u64 = 4096;
const SNAPSHOT_MEDIA_TYPE: &str = "application/vnd.kyberia.point-survey+json";
const SNAPSHOT_OPERATION: &str = "survey_snapshot_commit/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurveySnapshotRecord {
    pub snapshot_id: SnapshotId,
    pub project_id: ProjectId,
    pub session_id: SessionId,
    pub point_id: PointId,
    pub source_id: SourceId,
    pub collector_id: CollectorId,
    pub artifact_hash: String,
    pub input_schema_version: PointSnapshotInputVersion,
    pub output_schema_version: PointSnapshotSchemaVersion,
    pub decoder_version: &'static str,
    pub source_version: Text,
    pub created_utc_ms: i64,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedSurveySnapshot {
    pub record: SurveySnapshotRecord,
    pub survey: PointSurvey,
    pub decode_receipt: PointSnapshotDecodeReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurveySnapshotHistory {
    pub snapshot_id: SnapshotId,
    pub project_id: ProjectId,
    pub session_id: SessionId,
    pub point_id: PointId,
    pub source_id: SourceId,
    pub collector_id: CollectorId,
    pub artifact_hash: String,
    pub input_schema_version: PointSnapshotInputVersion,
    pub output_schema_version: PointSnapshotSchemaVersion,
    pub decoder_version: &'static str,
    pub source_version: Text,
    pub operation: Text,
    pub revision: u64,
    pub committed_utc_ms: i64,
}

fn snapshot_id_text(id: SnapshotId) -> String {
    id.into()
}

fn point_id_text(id: PointId) -> String {
    id.database_key()
}

fn parse_point_id(raw: String) -> Result<PointId> {
    PointId::from_database_key(&raw)
        .map_err(|_| StoreError::Corrupt("invalid point_id in snapshot index".into()))
}

fn source_id_text(id: SourceId) -> String {
    id.into()
}

fn collector_id_text(id: CollectorId) -> String {
    id.into()
}

fn parse_source_id(raw: String) -> Result<SourceId> {
    parse_id(raw, "source_id")
}

fn parse_collector_id(raw: String) -> Result<CollectorId> {
    parse_id(raw, "collector_id")
}

fn project_id_text(id: ProjectId) -> String {
    id.into()
}

fn session_id_text(id: SessionId) -> String {
    id.into()
}

fn parse_id<T>(raw: String, field: &'static str) -> Result<T>
where
    T: TryFrom<String>,
{
    T::try_from(raw).map_err(|_| StoreError::Corrupt(format!("invalid {field} in snapshot index")))
}

fn input_schema_text(version: PointSnapshotInputVersion) -> &'static str {
    match version {
        PointSnapshotInputVersion::LegacyUntaggedV1 => "legacy_untagged_v1",
        PointSnapshotInputVersion::V2 => "2",
    }
}

fn parse_input_schema(raw: String) -> Result<PointSnapshotInputVersion> {
    match raw.as_str() {
        "legacy_untagged_v1" => Ok(PointSnapshotInputVersion::LegacyUntaggedV1),
        "2" => Ok(PointSnapshotInputVersion::V2),
        unknown => Err(unknown.parse::<u32>().map_or_else(
            |_| StoreError::Corrupt("unknown survey snapshot input schema".into()),
            StoreError::UnsupportedVersion,
        )),
    }
}

fn parse_output_schema(raw: &str) -> Result<PointSnapshotSchemaVersion> {
    match raw {
        "2" => Ok(PointSnapshotSchemaVersion::V2),
        unknown => Err(unknown.parse::<u32>().map_or_else(
            |_| StoreError::Corrupt("unknown survey snapshot output schema".into()),
            StoreError::UnsupportedVersion,
        )),
    }
}

fn parse_decoder_version(raw: &str) -> Result<&'static str> {
    match raw {
        "kyberia-point-snapshot/2.0.0" => Ok("kyberia-point-snapshot/2.0.0"),
        _ => Err(StoreError::UnsupportedVersion(2)),
    }
}

fn source_version(survey: &PointSurvey) -> Result<Text> {
    Text::new(survey.config().data().adapter_version.as_str())
        .map_err(|_| StoreError::Corrupt("snapshot source version is invalid".into()))
}

fn encode_current(survey: &PointSurvey) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(survey)?;
    if bytes.len() as u64 > MAX_SURVEY_SNAPSHOT_BYTES {
        return Err(StoreError::Invalid(
            "survey snapshot exceeds 8 MiB resource limit".into(),
        ));
    }
    // Serialize only a validated state, but still run the decoder at the
    // persistence boundary so the bytes committed to disk have an executable
    // replay proof and the same strict schema checks as imported data.
    let decoded: DecodedPointSurvey = serde_json::from_slice(&bytes).map_err(|error| {
        StoreError::Corrupt(format!("serialized survey failed replay: {error}"))
    })?;
    if decoded.receipt.input_schema_version != PointSnapshotInputVersion::V2
        || decoded.survey != *survey
    {
        return Err(StoreError::Corrupt(
            "serialized survey did not round-trip as current schema".into(),
        ));
    }
    Ok(bytes)
}

fn decode_snapshot(bytes: &[u8]) -> Result<DecodedPointSurvey> {
    if bytes.len() as u64 > MAX_SURVEY_SNAPSHOT_BYTES {
        return Err(StoreError::Corrupt(
            "survey snapshot exceeds 8 MiB read budget".into(),
        ));
    }
    serde_json::from_slice(bytes)
        .map_err(|error| StoreError::Corrupt(format!("survey snapshot replay failed: {error}")))
}

fn snapshot_entry(bytes: &[u8], input: PointSnapshotInputVersion) -> Result<ArtifactEntry> {
    if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err(StoreError::Invalid(
            "artifact exceeds 64 MiB chunk limit".into(),
        ));
    }
    Ok(ArtifactEntry {
        kind: ArtifactKind::SurveySnapshot,
        bytes: bytes.len() as u64,
        media_type: SNAPSHOT_MEDIA_TYPE.into(),
        provenance_id: format!("survey-snapshot-payload/{}", input_schema_text(input)),
    })
}

fn validate_snapshot_entry(entry: &ArtifactEntry, record: &SurveySnapshotRecord) -> Result<()> {
    if entry.kind != ArtifactKind::SurveySnapshot
        || entry.media_type != SNAPSHOT_MEDIA_TYPE
        || entry.provenance_id
            != format!(
                "survey-snapshot-payload/{}",
                input_schema_text(record.input_schema_version)
            )
        || entry.bytes > MAX_SURVEY_SNAPSHOT_BYTES
    {
        return Err(StoreError::Corrupt(
            "survey snapshot artifact metadata has unexpected semantics".into(),
        ));
    }
    Ok(())
}

fn validate_timestamp(utc_ms: i64) -> Result<()> {
    if utc_ms < 0 {
        return Err(StoreError::Invalid(
            "snapshot timestamp must be a nonnegative UTC millisecond value".into(),
        ));
    }
    Ok(())
}

fn validate_snapshot_timestamp(timestamp: i64, manifest: &crate::BundleManifest) -> Result<()> {
    validate_timestamp(timestamp)?;
    if timestamp < manifest.created_utc_ms || timestamp > manifest.updated_utc_ms {
        return Err(StoreError::Corrupt(
            "survey snapshot timestamp falls outside the committed manifest interval".into(),
        ));
    }
    Ok(())
}

fn ensure_snapshot_schema(bundle: &Bundle) -> Result<()> {
    if !sqlite_guard::has_survey_snapshot_schema(&bundle.connection)? {
        return Err(StoreError::UnsupportedVersion(1));
    }
    Ok(())
}

fn read_snapshot_bytes(root: &Path, hash: &str, declared_bytes: u64) -> Result<Vec<u8>> {
    if declared_bytes > MAX_SURVEY_SNAPSHOT_BYTES {
        return Err(StoreError::Corrupt(
            "survey snapshot artifact exceeds read budget".into(),
        ));
    }
    regular(&root.join("artifacts"), true)?;
    let bytes = bounded_read(
        &root.join("artifacts").join(hash),
        MAX_SURVEY_SNAPSHOT_BYTES,
    )
    .map_err(|error| match error {
        StoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            StoreError::Corrupt("survey snapshot artifact is missing".into())
        }
        other => other,
    })?;
    if bytes.len() as u64 != declared_bytes || content_hash(&bytes) != hash {
        return Err(StoreError::Corrupt(
            "survey snapshot artifact checksum/length mismatch".into(),
        ));
    }
    Ok(bytes)
}

fn existing_row(
    transaction: &rusqlite::Transaction<'_>,
    snapshot_id: &str,
) -> Result<Option<RawSnapshotRow>> {
    transaction
        .query_row(
            "SELECT snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,created_utc_ms,revision FROM survey_snapshots WHERE snapshot_id=?1",
            [snapshot_id],
            RawSnapshotRow::from_row,
        )
        .optional()
        .map_err(StoreError::from)
}

struct RawSnapshotRow {
    snapshot_id: String,
    project_id: String,
    session_id: String,
    point_id: String,
    source_id: String,
    collector_id: String,
    artifact_hash: String,
    input_schema: String,
    output_schema: String,
    decoder_version: String,
    source_version: String,
    created_utc_ms: i64,
    revision: i64,
}

impl RawSnapshotRow {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            snapshot_id: row.get(0)?,
            project_id: row.get(1)?,
            session_id: row.get(2)?,
            point_id: row.get(3)?,
            source_id: row.get(4)?,
            collector_id: row.get(5)?,
            artifact_hash: row.get(6)?,
            input_schema: row.get(7)?,
            output_schema: row.get(8)?,
            decoder_version: row.get(9)?,
            source_version: row.get(10)?,
            created_utc_ms: row.get(11)?,
            revision: row.get(12)?,
        })
    }

    fn record(self) -> Result<SurveySnapshotRecord> {
        let revision = u64::try_from(self.revision)
            .map_err(|_| StoreError::Corrupt("negative snapshot revision".into()))?;
        let created_utc_ms = self.created_utc_ms;
        validate_timestamp(created_utc_ms)?;
        let output_schema = parse_output_schema(&self.output_schema)?;
        let decoder_version = parse_decoder_version(&self.decoder_version)?;
        validate_hash(&self.artifact_hash)?;
        Ok(SurveySnapshotRecord {
            snapshot_id: parse_id(self.snapshot_id, "snapshot_id")?,
            project_id: parse_id(self.project_id, "project_id")?,
            session_id: parse_id(self.session_id, "session_id")?,
            point_id: parse_point_id(self.point_id)?,
            source_id: parse_source_id(self.source_id)?,
            collector_id: parse_collector_id(self.collector_id)?,
            artifact_hash: self.artifact_hash,
            input_schema_version: parse_input_schema(self.input_schema)?,
            output_schema_version: output_schema,
            decoder_version,
            source_version: Text::new(self.source_version)
                .map_err(|_| StoreError::Corrupt("invalid snapshot source version".into()))?,
            created_utc_ms,
            revision,
        })
    }
}

fn check_row_matches(
    row: &SurveySnapshotRecord,
    current_project: ProjectId,
    snapshot_id: SnapshotId,
    decoded: &DecodedPointSurvey,
) -> Result<()> {
    let data = decoded.survey.config().data();
    if row.snapshot_id != snapshot_id {
        return Err(StoreError::Corrupt(
            "snapshot index identity mismatch".into(),
        ));
    }
    if row.project_id != current_project
        || row.session_id != data.session_id
        || row.point_id != data.point_id
        || row.source_id != data.source_id
        || row.collector_id != data.collector_id
        || row.source_version != data.adapter_version
    {
        return Err(StoreError::Corrupt(
            "survey snapshot index does not match replayed state".into(),
        ));
    }
    if row.input_schema_version != decoded.receipt.input_schema_version
        || row.output_schema_version != decoded.receipt.output_schema_version
        || row.decoder_version != decoded.receipt.decoder_version
    {
        return Err(StoreError::Corrupt(
            "survey snapshot decoder receipt does not match index".into(),
        ));
    }
    Ok(())
}

const HISTORY_QUERY_LIMIT: i64 = MAX_SURVEY_SNAPSHOTS as i64 + 1;

struct RawHistoryRow {
    snapshot_id: String,
    project_id: String,
    session_id: String,
    point_id: String,
    source_id: String,
    collector_id: String,
    artifact_hash: String,
    input_schema: String,
    output_schema: String,
    decoder_version: String,
    source_version: String,
    operation: String,
    revision: i64,
    committed_utc_ms: i64,
}

impl RawHistoryRow {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            snapshot_id: row.get(0)?,
            project_id: row.get(1)?,
            session_id: row.get(2)?,
            point_id: row.get(3)?,
            source_id: row.get(4)?,
            collector_id: row.get(5)?,
            artifact_hash: row.get(6)?,
            input_schema: row.get(7)?,
            output_schema: row.get(8)?,
            decoder_version: row.get(9)?,
            source_version: row.get(10)?,
            operation: row.get(11)?,
            revision: row.get(12)?,
            committed_utc_ms: row.get(13)?,
        })
    }

    fn history(self) -> Result<SurveySnapshotHistory> {
        let revision = u64::try_from(self.revision)
            .map_err(|_| StoreError::Corrupt("negative history revision".into()))?;
        validate_timestamp(self.committed_utc_ms)?;
        let input_schema = parse_input_schema(self.input_schema)?;
        let output_schema = parse_output_schema(&self.output_schema)?;
        let decoder_version = parse_decoder_version(&self.decoder_version)?;
        validate_hash(&self.artifact_hash)?;
        let operation = Text::new(self.operation)
            .map_err(|_| StoreError::Corrupt("invalid snapshot history operation".into()))?;
        Ok(SurveySnapshotHistory {
            snapshot_id: parse_id(self.snapshot_id, "snapshot_id")?,
            project_id: parse_id(self.project_id, "project_id")?,
            session_id: parse_id(self.session_id, "session_id")?,
            point_id: parse_point_id(self.point_id)?,
            source_id: parse_source_id(self.source_id)?,
            collector_id: parse_collector_id(self.collector_id)?,
            artifact_hash: self.artifact_hash,
            input_schema_version: input_schema,
            output_schema_version: output_schema,
            decoder_version,
            source_version: Text::new(self.source_version)
                .map_err(|_| StoreError::Corrupt("invalid snapshot source version".into()))?,
            operation,
            revision,
            committed_utc_ms: self.committed_utc_ms,
        })
    }
}

fn read_index_rows(transaction: &rusqlite::Transaction<'_>) -> Result<Vec<SurveySnapshotRecord>> {
    let mut statement = transaction.prepare(
        "SELECT snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,created_utc_ms,revision FROM survey_snapshots ORDER BY revision,snapshot_id LIMIT ?1",
    )?;
    let mut rows = statement.query([HISTORY_QUERY_LIMIT])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(RawSnapshotRow::from_row(row)?.record()?);
    }
    if result.len() as i64 >= HISTORY_QUERY_LIMIT {
        return Err(StoreError::Corrupt(
            "survey snapshot inventory exceeds resource limit".into(),
        ));
    }
    Ok(result)
}

fn read_history_rows(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<Vec<SurveySnapshotHistory>> {
    let mut statement = transaction.prepare(
        "SELECT snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,revision,committed_utc_ms FROM survey_snapshot_history ORDER BY revision LIMIT ?1",
    )?;
    let mut rows = statement.query([HISTORY_QUERY_LIMIT])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(RawHistoryRow::from_row(row)?.history()?);
    }
    if result.len() as i64 >= HISTORY_QUERY_LIMIT {
        return Err(StoreError::Corrupt(
            "survey snapshot history exceeds resource limit".into(),
        ));
    }
    Ok(result)
}

fn validate_history(
    history: &SurveySnapshotHistory,
    manifest: &crate::BundleManifest,
) -> Result<()> {
    if history.project_id != manifest.project_id
        || history.revision == 0
        || history.operation.as_str() != SNAPSHOT_OPERATION
    {
        return Err(StoreError::Corrupt(
            "survey snapshot history has invalid project or revision".into(),
        ));
    }
    validate_hash(&history.artifact_hash)?;
    validate_snapshot_timestamp(history.committed_utc_ms, manifest)?;
    Ok(())
}

fn validate_history_matches_index(
    history: &SurveySnapshotHistory,
    index: &SurveySnapshotRecord,
    manifest: &crate::BundleManifest,
) -> Result<()> {
    validate_history(history, manifest)?;
    if index.project_id != manifest.project_id
        || index.revision == 0
        || index.revision > manifest.revision
    {
        return Err(StoreError::Corrupt(
            "survey snapshot index has invalid project or revision".into(),
        ));
    }
    validate_snapshot_timestamp(index.created_utc_ms, manifest)?;
    if history.snapshot_id != index.snapshot_id
        || history.project_id != index.project_id
        || history.session_id != index.session_id
        || history.point_id != index.point_id
        || history.source_id != index.source_id
        || history.collector_id != index.collector_id
        || history.artifact_hash != index.artifact_hash
        || history.input_schema_version != index.input_schema_version
        || history.output_schema_version != index.output_schema_version
        || history.decoder_version != index.decoder_version
        || history.source_version != index.source_version
        || history.committed_utc_ms != index.created_utc_ms
        || history.revision != index.revision
    {
        return Err(StoreError::Corrupt(
            "survey snapshot history does not exactly match its immutable index".into(),
        ));
    }
    Ok(())
}

fn validate_history_inventory(
    transaction: &rusqlite::Transaction<'_>,
    manifest: &crate::BundleManifest,
) -> Result<(Vec<SurveySnapshotRecord>, Vec<SurveySnapshotHistory>)> {
    let indexes = read_index_rows(transaction)?;
    let histories = read_history_rows(transaction)?;
    if indexes.len() != histories.len() {
        return Err(StoreError::Corrupt(
            "survey snapshot index and history cardinalities differ".into(),
        ));
    }
    for index in &indexes {
        let mut matches = histories
            .iter()
            .filter(|history| history.snapshot_id == index.snapshot_id);
        let Some(history) = matches.next() else {
            return Err(StoreError::Corrupt(
                "survey snapshot index has missing history".into(),
            ));
        };
        if matches.next().is_some() {
            return Err(StoreError::Corrupt(
                "survey snapshot index has duplicate history".into(),
            ));
        }
        validate_history_matches_index(history, index, manifest)?;
    }
    for history in &histories {
        let mut matches = indexes
            .iter()
            .filter(|index| index.snapshot_id == history.snapshot_id);
        let Some(index) = matches.next() else {
            return Err(StoreError::Corrupt(
                "survey snapshot history has an extra index".into(),
            ));
        };
        if matches.next().is_some() {
            return Err(StoreError::Corrupt(
                "survey snapshot history has duplicate index".into(),
            ));
        }
        validate_history_matches_index(history, index, manifest)?;
    }
    Ok((indexes, histories))
}

fn validate_snapshot_history_pair(
    transaction: &rusqlite::Transaction<'_>,
    index: &SurveySnapshotRecord,
    manifest: &crate::BundleManifest,
) -> Result<()> {
    let mut statement = transaction.prepare(
        "SELECT snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,revision,committed_utc_ms FROM survey_snapshot_history WHERE snapshot_id=?1 LIMIT 2",
    )?;
    let mut rows = statement.query([snapshot_id_text(index.snapshot_id)])?;
    let mut matches = Vec::new();
    while let Some(row) = rows.next()? {
        matches.push(RawHistoryRow::from_row(row)?.history()?);
    }
    if matches.len() != 1 {
        return Err(StoreError::Corrupt(
            "survey snapshot has missing or duplicate history".into(),
        ));
    }
    validate_history_matches_index(&matches[0], index, manifest)
}

fn read_and_replay_snapshot(
    root: &Path,
    manifest: &crate::BundleManifest,
    record: &SurveySnapshotRecord,
    cancel: &dyn Cancellation,
) -> Result<DecodedPointSurvey> {
    if cancel.is_cancelled() {
        return Err(StoreError::Cancelled);
    }
    let entry = manifest
        .artifacts
        .get(&record.artifact_hash)
        .ok_or_else(|| StoreError::Corrupt("survey snapshot artifact is unregistered".into()))?;
    validate_snapshot_entry(entry, record)?;
    let bytes = read_snapshot_bytes(root, &record.artifact_hash, entry.bytes)?;
    if cancel.is_cancelled() {
        return Err(StoreError::Cancelled);
    }
    let decoded = decode_snapshot(&bytes)?;
    if cancel.is_cancelled() {
        return Err(StoreError::Cancelled);
    }
    check_row_matches(record, manifest.project_id, record.snapshot_id, &decoded)?;
    Ok(decoded)
}

impl Bundle {
    /// Persist a current V2 point survey under a caller-owned immutable
    /// snapshot identity. A duplicate identity with identical bytes is
    /// idempotent and does not advance the project revision or history.
    pub fn save_survey_snapshot(
        &mut self,
        snapshot_id: SnapshotId,
        survey: &PointSurvey,
        utc_ms: i64,
    ) -> Result<SurveySnapshotRecord> {
        self.save_survey_snapshot_if_revision(snapshot_id, survey, utc_ms, None)
    }

    /// The optional expected revision provides optimistic concurrency for UI
    /// handles. It is checked inside the same immediate transaction that writes
    /// the artifact reference and snapshot index.
    pub fn save_survey_snapshot_if_revision(
        &mut self,
        snapshot_id: SnapshotId,
        survey: &PointSurvey,
        utc_ms: i64,
        expected_revision: Option<u64>,
    ) -> Result<SurveySnapshotRecord> {
        let bytes = encode_current(survey)?;
        let decoded = decode_snapshot(&bytes)?;
        self.persist_survey_snapshot(snapshot_id, &bytes, &decoded, utc_ms, expected_revision)
    }

    /// Import a V1 or V2 snapshot wire document. V1 bytes remain immutable in
    /// the artifact store; loading returns the survey decoder's explicit
    /// migration receipt and never silently rewrites the evidence artifact.
    pub fn import_survey_snapshot(
        &mut self,
        snapshot_id: SnapshotId,
        bytes: &[u8],
        utc_ms: i64,
        expected_revision: Option<u64>,
    ) -> Result<SurveySnapshotRecord> {
        let decoded = decode_snapshot(bytes)?;
        self.persist_survey_snapshot(snapshot_id, bytes, &decoded, utc_ms, expected_revision)
    }

    fn persist_survey_snapshot(
        &mut self,
        snapshot_id: SnapshotId,
        bytes: &[u8],
        decoded: &DecodedPointSurvey,
        utc_ms: i64,
        expected_revision: Option<u64>,
    ) -> Result<SurveySnapshotRecord> {
        if self.mode == crate::OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        validate_timestamp(utc_ms)?;
        ensure_snapshot_schema(self)?;
        let input_schema = decoded.receipt.input_schema_version;
        let entry = snapshot_entry(bytes, input_schema)?;
        let hash = self.write_artifact_file(bytes)?;
        self.start_operation()?;
        let artifact_root = self.root.clone();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut manifest = load_manifest(&transaction)?;
        if manifest.schema_version != crate::manifest::SCHEMA_VERSION
            || !manifest.required_features.is_empty()
        {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        if utc_ms < manifest.created_utc_ms {
            return Err(StoreError::Invalid(
                "snapshot timestamp precedes project creation".into(),
            ));
        }
        if let Some(expected) = expected_revision
            && expected != manifest.revision
        {
            return Err(StoreError::Invalid(format!(
                "stale project revision: expected {expected}, current {}",
                manifest.revision
            )));
        }
        let snapshot_text = snapshot_id_text(snapshot_id);
        if utc_ms < manifest.updated_utc_ms {
            return Err(StoreError::Invalid(
                "snapshot timestamp precedes the committed project timestamp".into(),
            ));
        }
        // Preserve the fail-closed property for all write paths: an existing
        // index/history divergence must not be hidden by a later snapshot.
        validate_history_inventory(&transaction, &manifest)?;
        if let Some(raw) = existing_row(&transaction, &snapshot_text)? {
            let existing = raw.record()?;
            if existing.project_id != manifest.project_id {
                return Err(StoreError::Corrupt(
                    "snapshot index belongs to another project".into(),
                ));
            }
            if existing.artifact_hash != hash
                || existing.session_id != decoded.survey.config().data().session_id
                || existing.point_id != decoded.survey.config().data().point_id
                || existing.input_schema_version != input_schema
            {
                return Err(StoreError::Invalid(
                    "snapshot identity already has different evidence".into(),
                ));
            }
            check_row_matches(&existing, manifest.project_id, snapshot_id, decoded)?;
            validate_snapshot_history_pair(&transaction, &existing, &manifest)?;
            let registered = manifest
                .artifacts
                .get(&hash)
                .ok_or_else(|| StoreError::Corrupt("snapshot artifact is unregistered".into()))?;
            validate_snapshot_entry(registered, &existing)?;
            if registered != &entry
                || read_snapshot_bytes(&artifact_root, &hash, bytes.len() as u64)? != bytes
            {
                return Err(StoreError::Corrupt(
                    "existing snapshot artifact failed integrity verification".into(),
                ));
            }
            return Ok(existing);
        }
        let mut count = 0_u64;
        let mut count_statement =
            transaction.prepare("SELECT snapshot_id FROM survey_snapshots LIMIT ?1")?;
        let mut count_rows = count_statement.query([MAX_SURVEY_SNAPSHOTS as i64])?;
        while count_rows.next()?.is_some() {
            count = count
                .checked_add(1)
                .ok_or_else(|| StoreError::Corrupt("snapshot count overflow".into()))?;
            if count >= MAX_SURVEY_SNAPSHOTS {
                break;
            }
        }
        drop(count_rows);
        drop(count_statement);
        if count >= MAX_SURVEY_SNAPSHOTS {
            return Err(StoreError::Invalid(
                "survey snapshot inventory exceeds resource limit".into(),
            ));
        }
        if let Some(existing) = manifest.artifacts.get(&hash)
            && existing != &entry
        {
            return Err(StoreError::Invalid(
                "snapshot content hash is registered with different semantics".into(),
            ));
        }
        let project_id = manifest.project_id;
        let session_id = decoded.survey.config().data().session_id;
        let point_id = decoded.survey.config().data().point_id;
        let source_version = source_version(&decoded.survey)?;
        let revision = manifest
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("revision exhausted".into()))?;
        if !manifest.artifacts.contains_key(&hash) {
            manifest.artifacts.insert(hash.clone(), entry);
        }
        manifest.revision = revision;
        manifest.updated_utc_ms = utc_ms;
        let encoded = manifest.encode()?;
        let revision_i64 = i64::try_from(revision)
            .map_err(|_| StoreError::Invalid("revision exhausted".into()))?;
        let changed = transaction.execute(
            "UPDATE bundle_manifest SET revision=?1, body=?2 WHERE singleton=1",
            (revision_i64, &encoded),
        )?;
        if changed != 1 {
            return Err(StoreError::Corrupt(
                "manifest update did not affect exactly one row".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO survey_snapshots (snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,created_utc_ms,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            (
                &snapshot_text,
                project_id_text(project_id),
                session_id_text(session_id),
                point_id_text(point_id),
                source_id_text(decoded.survey.config().data().source_id),
                collector_id_text(decoded.survey.config().data().collector_id),
                &hash,
                input_schema_text(input_schema),
                "2",
                decoded.receipt.decoder_version,
                source_version.as_str(),
                utc_ms,
                revision_i64,
            ),
        )?;
        transaction.execute(
            "INSERT INTO survey_snapshot_history (revision,snapshot_id,project_id,session_id,point_id,source_id,collector_id,artifact_hash,input_schema,output_schema,decoder_version,source_version,operation,committed_utc_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            (
                revision_i64,
                &snapshot_text,
                project_id_text(project_id),
                session_id_text(session_id),
                point_id_text(point_id),
                source_id_text(decoded.survey.config().data().source_id),
                collector_id_text(decoded.survey.config().data().collector_id),
                &hash,
                input_schema_text(input_schema),
                "2",
                decoded.receipt.decoder_version,
                source_version.as_str(),
                SNAPSHOT_OPERATION,
                utc_ms,
            ),
        )?;
        let committed = load_manifest(&transaction)?;
        let stored: Vec<u8> = transaction.query_row(
            "SELECT body FROM bundle_manifest WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if committed != manifest || stored != encoded {
            return Err(StoreError::Corrupt(
                "survey snapshot manifest failed authoritative readback".into(),
            ));
        }
        let row = existing_row(&transaction, &snapshot_text)?
            .ok_or_else(|| StoreError::Corrupt("survey snapshot insert disappeared".into()))?;
        let record = row.record()?;
        let loaded_entry = manifest
            .artifacts
            .get(&hash)
            .ok_or_else(|| StoreError::Corrupt("snapshot artifact registration missing".into()))?;
        validate_snapshot_entry(loaded_entry, &record)?;
        if loaded_entry.bytes != bytes.len() as u64 {
            return Err(StoreError::Corrupt(
                "snapshot artifact registration failed readback".into(),
            ));
        }
        if record.revision != revision {
            return Err(StoreError::Corrupt(
                "snapshot index revision failed readback".into(),
            ));
        }
        check_row_matches(&record, project_id, snapshot_id, decoded)?;
        validate_snapshot_history_pair(&transaction, &record, &manifest)?;
        atomic_projection(&self.root, &manifest)?;
        transaction.commit()?;
        Ok(record)
    }

    /// Load and verify one immutable snapshot. The returned survey has passed
    /// the survey crate's semantic replay validation before it is exposed.
    pub fn load_survey_snapshot(&self, snapshot_id: SnapshotId) -> Result<LoadedSurveySnapshot> {
        self.load_survey_snapshot_with_cancel(snapshot_id, None, &NeverCancel)
    }

    /// Optional session checking makes accidental cross-session reuse explicit
    /// at the query boundary; the row's own session is always checked too.
    pub fn load_survey_snapshot_for_session(
        &self,
        snapshot_id: SnapshotId,
        expected_session: Option<SessionId>,
    ) -> Result<LoadedSurveySnapshot> {
        self.load_survey_snapshot_with_cancel(snapshot_id, expected_session, &NeverCancel)
    }

    /// Cancellation-aware snapshot replay. Cancellation is checked before
    /// schema/database work and around the bounded artifact read and survey
    /// decoder. The complete validated survey is returned or no snapshot is
    /// returned; a partially decoded state never crosses this boundary.
    pub fn load_survey_snapshot_with_cancel(
        &self,
        snapshot_id: SnapshotId,
        expected_session: Option<SessionId>,
        cancel: &dyn Cancellation,
    ) -> Result<LoadedSurveySnapshot> {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        self.start_operation()?;
        ensure_snapshot_schema(self)?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        // Validate the physical schema before evaluating any snapshot table
        // query. An already-open handle must not trust an externally replaced
        // table/view between operations.
        let manifest = load_manifest(&transaction)?;
        let (indexes, _) = validate_history_inventory(&transaction, &manifest)?;
        let record = indexes
            .into_iter()
            .find(|record| record.snapshot_id == snapshot_id)
            .ok_or_else(|| StoreError::Invalid("survey snapshot is not registered".into()))?;
        if expected_session.is_some_and(|expected| expected != record.session_id) {
            return Err(StoreError::Invalid(
                "survey snapshot session mismatch".into(),
            ));
        }
        validate_snapshot_history_pair(&transaction, &record, &manifest)?;
        let decoded = read_and_replay_snapshot(&self.root, &manifest, &record, cancel)?;
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        transaction.commit()?;
        Ok(LoadedSurveySnapshot {
            record,
            survey: decoded.survey,
            decode_receipt: decoded.receipt,
        })
    }

    /// Read append-only snapshot history in commit order. The complete bounded
    /// inventory is checked before applying an optional session filter, so a
    /// caller cannot hide an extra or missing history row by choosing a filter.
    pub fn list_survey_snapshot_history(
        &self,
        session_id: Option<SessionId>,
    ) -> Result<Vec<SurveySnapshotHistory>> {
        self.start_operation()?;
        ensure_snapshot_schema(self)?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        let (indexes, histories) = validate_history_inventory(&transaction, &manifest)?;
        // Replay every bounded index row before applying the optional session
        // projection. A filtered query must not conceal corruption in another
        // session or return metadata detached from its evidence bytes.
        for index in &indexes {
            read_and_replay_snapshot(&self.root, &manifest, index, &NeverCancel)?;
        }
        let result: Vec<_> = histories
            .into_iter()
            .filter(|history| session_id.is_none_or(|session| history.session_id == session))
            .collect();
        transaction.commit()?;
        Ok(result)
    }
}
