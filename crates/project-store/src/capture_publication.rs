//! Recoverable links between one immutable capture manifest and its outputs.

use crate::bundle::load_manifest;
use crate::manifest::validate_hash;
use crate::{Bundle, Result, StoreError, content_hash, sqlite_guard};
use kyberia_domain::{
    capture::{CaptureManifest, RawSourceDisposition},
    identity::{ObservationId, ProjectId, SnapshotId, Text},
    observation::{ObservationEnvelope, ObservationPayload},
};
use kyberia_survey::{CaptureMode, PointSurvey};
use rusqlite::{OptionalExtension, TransactionBehavior};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const CAPTURE_MANIFEST_MEDIA_TYPE: &str =
    "application/vnd.kyberia.capture-manifest+json";
pub(crate) const MAX_CAPTURE_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePublicationStatus {
    Manifest,
    Chunk,
    Complete,
    Terminal,
}

impl CapturePublicationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::Chunk => "chunk",
            Self::Complete => "complete",
            Self::Terminal => "terminal",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "manifest" => Ok(Self::Manifest),
            "chunk" => Ok(Self::Chunk),
            "complete" => Ok(Self::Complete),
            "terminal" => Ok(Self::Terminal),
            _ => Err(StoreError::Corrupt(
                "capture publication has an unknown status".into(),
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturePublicationRecord {
    pub manifest_hash: String,
    pub project_id: ProjectId,
    pub chunk_hash: Option<String>,
    pub snapshot_id: Option<SnapshotId>,
    pub status: CapturePublicationStatus,
    pub observation_count: u64,
    pub raw_record_count: u64,
    pub revision: u64,
}

pub struct CaptureManifestRegistration {
    manifest: CaptureManifest,
    provenance_id: Text,
    utc_ms: i64,
}

impl CaptureManifestRegistration {
    /// Store a validated domain manifest and its bounded publication context.
    /// Artifact bytes, hash, counts, and terminal status are derived by the
    /// store so callers cannot register contradictory free-form metadata.
    pub fn new(manifest: CaptureManifest, provenance_id: Text, utc_ms: i64) -> Result<Self> {
        manifest.canonical_bytes().map_err(StoreError::Invalid)?;
        if utc_ms < 0 {
            return Err(StoreError::Invalid(
                "capture publication time must be UTC".into(),
            ));
        }
        Ok(Self {
            manifest,
            provenance_id,
            utc_ms,
        })
    }
}

fn parse_project_id(value: String) -> Result<ProjectId> {
    value
        .try_into()
        .map_err(|_| StoreError::Corrupt("capture publication project is invalid".into()))
}

fn parse_snapshot_id(value: Option<String>) -> Result<Option<SnapshotId>> {
    value
        .map(|value| {
            value
                .try_into()
                .map_err(|_| StoreError::Corrupt("capture publication snapshot is invalid".into()))
        })
        .transpose()
}

fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CapturePublicationRecord> {
    let manifest_hash: String = row.get(0)?;
    let project_id: String = row.get(1)?;
    let chunk_hash: Option<String> = row.get(2)?;
    let snapshot_id: Option<String> = row.get(3)?;
    let status: String = row.get(4)?;
    let observation_count: i64 = row.get(5)?;
    let raw_record_count: i64 = row.get(6)?;
    let revision: i64 = row.get(7)?;
    Ok(CapturePublicationRecord {
        manifest_hash,
        project_id: parse_project_id(project_id).map_err(|_| rusqlite::Error::InvalidQuery)?,
        chunk_hash,
        snapshot_id: parse_snapshot_id(snapshot_id).map_err(|_| rusqlite::Error::InvalidQuery)?,
        status: CapturePublicationStatus::parse(&status)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        observation_count: u64::try_from(observation_count)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        raw_record_count: u64::try_from(raw_record_count)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        revision: u64::try_from(revision).map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

fn read_record(
    transaction: &rusqlite::Transaction<'_>,
    manifest_hash: &str,
) -> Result<Option<CapturePublicationRecord>> {
    transaction
        .query_row(
            "SELECT manifest_hash,project_id,chunk_hash,snapshot_id,status,observation_count,raw_record_count,revision FROM capture_publications WHERE manifest_hash=?1",
            [manifest_hash],
            record_from_row,
        )
        .optional()
        .map_err(StoreError::from)
}

fn validate_record_shape(record: &CapturePublicationRecord) -> Result<()> {
    validate_hash(&record.manifest_hash)?;
    if let Some(hash) = &record.chunk_hash {
        validate_hash(hash)?;
    }
    match record.status {
        CapturePublicationStatus::Manifest
            if record.chunk_hash.is_some() || record.snapshot_id.is_some() =>
        {
            return Err(StoreError::Corrupt(
                "manifest capture publication has an output link".into(),
            ));
        }
        CapturePublicationStatus::Chunk
            if record.chunk_hash.is_none() || record.snapshot_id.is_some() =>
        {
            return Err(StoreError::Corrupt(
                "chunk capture publication has inconsistent output links".into(),
            ));
        }
        CapturePublicationStatus::Complete
            if record.chunk_hash.is_none() || record.snapshot_id.is_none() =>
        {
            return Err(StoreError::Corrupt(
                "complete capture publication is missing an output link".into(),
            ));
        }
        CapturePublicationStatus::Terminal
            if (record.observation_count == 0 && record.chunk_hash.is_some())
                || (record.observation_count > 0
                    && record.snapshot_id.is_some()
                    && record.chunk_hash.is_none()) =>
        {
            return Err(StoreError::Corrupt(
                "terminal capture publication has inconsistent output links".into(),
            ));
        }
        _ => {}
    }
    Ok(())
}

fn read_capture_manifest(bundle: &Bundle, manifest_hash: &str) -> Result<CaptureManifest> {
    let bundle_manifest = bundle.manifest()?;
    let entry = bundle_manifest
        .artifacts
        .get(manifest_hash)
        .ok_or_else(|| StoreError::Corrupt("capture manifest artifact is missing".into()))?;
    if entry.kind != crate::ArtifactKind::RawCapture
        || entry.media_type != CAPTURE_MANIFEST_MEDIA_TYPE
        || entry.bytes > MAX_CAPTURE_MANIFEST_BYTES
    {
        return Err(StoreError::Corrupt(
            "capture manifest artifact has unexpected semantics".into(),
        ));
    }
    let bytes = bundle.read_registered_artifact(manifest_hash, entry)?;
    CaptureManifest::from_canonical_bytes(&bytes).map_err(StoreError::Corrupt)
}

fn manifest_observation_ids(
    manifest: &CaptureManifest,
) -> BTreeSet<kyberia_domain::identity::ObservationId> {
    manifest
        .observation_ids_in_source_order()
        .iter()
        .copied()
        .collect()
}

fn validate_snapshot_association_closure(
    survey: &PointSurvey,
    observations: &[ObservationEnvelope],
    expected_ids: &BTreeSet<ObservationId>,
) -> Result<()> {
    let associations: BTreeMap<ObservationId, _> = survey
        .associations()
        .iter()
        .map(|association| (association.observation_id(), association))
        .collect();
    let mut canonical_by_id = BTreeMap::new();
    for observation in observations {
        if canonical_by_id
            .insert(observation.data().id, observation)
            .is_some()
        {
            return Err(StoreError::Corrupt(
                "capture publication chunk has duplicate observation identities".into(),
            ));
        }
    }
    for id in expected_ids {
        let association = associations.get(id).ok_or_else(|| {
            StoreError::Corrupt(
                "capture publication snapshot omits a manifest observation association".into(),
            )
        })?;
        let observation = canonical_by_id.get(id).ok_or_else(|| {
            StoreError::Corrupt(
                "capture publication chunk omits a manifest observation envelope".into(),
            )
        })?;
        let config = survey.config().data();
        if observation.data().session_id != config.session_id
            || observation.data().source.source_id != config.source_id
            || observation.data().source.collector_id != config.collector_id
            || observation.data().source.adapter_version != config.adapter_version
            || !matches!(
                (&config.mode, &observation.data().payload),
                (CaptureMode::Scan, ObservationPayload::Scan(_))
                    | (CaptureMode::Frame, ObservationPayload::Frame(_))
            )
        {
            return Err(StoreError::Corrupt(
                "capture publication observation differs from survey source identity or mode"
                    .into(),
            ));
        }
        if !association.matches_canonical_observation(observation) {
            return Err(StoreError::Corrupt(
                "capture publication association differs from its canonical observation".into(),
            ));
        }
    }
    Ok(())
}

impl Bundle {
    /// Register the immutable capture manifest and its recoverable publication
    /// row. The manifest artifact is content-addressed and this operation is
    /// exact-idempotent.
    pub fn persist_capture_manifest(
        &mut self,
        input: CaptureManifestRegistration,
    ) -> Result<CapturePublicationRecord> {
        let CaptureManifestRegistration {
            manifest,
            provenance_id,
            utc_ms,
        } = input;
        if self.mode == crate::OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        let manifest_bytes = manifest.canonical_bytes().map_err(StoreError::Invalid)?;
        let manifest_hash = content_hash(&manifest_bytes);
        let observation_count = u64::from(manifest.completion().observation_count());
        let raw_record_count = match manifest.raw_source_disposition() {
            RawSourceDisposition::Retained => manifest.source_records().len() as u64,
            RawSourceDisposition::NotRetained => 0,
        };
        let terminal = manifest.terminal();
        if manifest_bytes.is_empty() || manifest_bytes.len() as u64 > MAX_CAPTURE_MANIFEST_BYTES {
            return Err(StoreError::Invalid(
                "capture manifest exceeds resource limit".into(),
            ));
        }
        validate_hash(&manifest_hash)?;
        if !sqlite_guard::has_capture_publication_schema(&self.connection)? {
            return Err(StoreError::UnsupportedVersion(1));
        }
        let entry = crate::ArtifactEntry {
            kind: crate::ArtifactKind::RawCapture,
            bytes: manifest_bytes.len() as u64,
            media_type: CAPTURE_MANIFEST_MEDIA_TYPE.into(),
            provenance_id: provenance_id.as_str().to_owned(),
        };
        self.put_artifact(&manifest_bytes, entry, utc_ms)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let manifest = load_manifest(&transaction)?;
        let existing = read_record(&transaction, &manifest_hash)?;
        if let Some(existing) = existing {
            validate_record_shape(&existing)?;
            if existing.project_id != manifest.project_id
                || existing.observation_count != observation_count
                || existing.raw_record_count != raw_record_count
                || (existing.status == CapturePublicationStatus::Terminal) != terminal
            {
                return Err(StoreError::Corrupt(
                    "capture manifest publication metadata differs".into(),
                ));
            }
            transaction.commit()?;
            return Ok(existing);
        }
        let status = if terminal {
            CapturePublicationStatus::Terminal
        } else {
            CapturePublicationStatus::Manifest
        };
        transaction.execute(
            "INSERT INTO capture_publications (manifest_hash,project_id,chunk_hash,snapshot_id,status,observation_count,raw_record_count,revision) VALUES (?1,?2,NULL,NULL,?3,?4,?5,?6)",
            (
                &manifest_hash,
                String::from(manifest.project_id),
                status.as_str(),
                i64::try_from(observation_count)
                    .map_err(|_| StoreError::Invalid("observation count exceeds SQLite integer".into()))?,
                i64::try_from(raw_record_count)
                    .map_err(|_| StoreError::Invalid("raw record count exceeds SQLite integer".into()))?,
                i64::try_from(manifest.revision)
                    .map_err(|_| StoreError::Invalid("manifest revision exhausted".into()))?,
            ),
        )?;
        let record = read_record(&transaction, &manifest_hash)?
            .ok_or_else(|| StoreError::Corrupt("capture publication insert disappeared".into()))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn link_capture_chunk(
        &mut self,
        manifest_hash: &str,
        chunk_hash: &str,
        observation_count: u64,
    ) -> Result<CapturePublicationRecord> {
        if self.mode == crate::OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        validate_hash(manifest_hash)?;
        validate_hash(chunk_hash)?;
        let capture_manifest = read_capture_manifest(self, manifest_hash)?;
        if u64::from(capture_manifest.completion().observation_count()) != observation_count {
            return Err(StoreError::Corrupt(
                "capture publication manifest row count differs from its manifest".into(),
            ));
        }
        let expected_ids = manifest_observation_ids(&capture_manifest);
        let chunk = self.read_observation_chunk(chunk_hash)?;
        let actual_ids: BTreeSet<_> = chunk
            .iter()
            .map(|observation| observation.data().id)
            .collect();
        if chunk.len() as u64 != observation_count || actual_ids != expected_ids {
            return Err(StoreError::Corrupt(
                "capture publication chunk identities differ from its manifest".into(),
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let manifest = load_manifest(&transaction)?;
        let mut record = read_record(&transaction, manifest_hash)?
            .ok_or_else(|| StoreError::Corrupt("capture publication manifest is missing".into()))?;
        validate_record_shape(&record)?;
        if record.project_id != manifest.project_id || record.observation_count != observation_count
        {
            return Err(StoreError::Corrupt(
                "capture publication identity or row count differs".into(),
            ));
        }
        let stored_count: Option<i64> = transaction
            .query_row(
                "SELECT row_count FROM observation_chunks WHERE chunk_hash=?1",
                [chunk_hash],
                |row| row.get(0),
            )
            .optional()?;
        if stored_count
            != Some(i64::try_from(observation_count).map_err(|_| {
                StoreError::Invalid("observation count exceeds SQLite integer".into())
            })?)
        {
            return Err(StoreError::Corrupt(
                "capture publication chunk is missing or has a different row count".into(),
            ));
        }
        if let Some(existing) = &record.chunk_hash
            && existing != chunk_hash
        {
            return Err(StoreError::Corrupt(
                "capture publication already links another chunk".into(),
            ));
        }
        record.chunk_hash = Some(chunk_hash.into());
        if record.status == CapturePublicationStatus::Manifest {
            record.status = CapturePublicationStatus::Chunk;
        }
        transaction.execute(
            "UPDATE capture_publications SET chunk_hash=?1,status=?2,revision=?3 WHERE manifest_hash=?4",
            (
                chunk_hash,
                record.status.as_str(),
                i64::try_from(record.revision)
                    .map_err(|_| StoreError::Invalid("manifest revision exhausted".into()))?,
                manifest_hash,
            ),
        )?;
        let stored = read_record(&transaction, manifest_hash)?
            .ok_or_else(|| StoreError::Corrupt("capture publication link disappeared".into()))?;
        transaction.commit()?;
        Ok(stored)
    }

    pub fn link_capture_snapshot(
        &mut self,
        manifest_hash: &str,
        snapshot_id: SnapshotId,
        expected_survey: &kyberia_survey::PointSurvey,
    ) -> Result<CapturePublicationRecord> {
        if self.mode == crate::OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        validate_hash(manifest_hash)?;
        let capture_manifest = read_capture_manifest(self, manifest_hash)?;
        let loaded_snapshot = self.load_survey_snapshot(snapshot_id)?;
        if loaded_snapshot.survey != *expected_survey {
            return Err(StoreError::Corrupt(
                "capture publication snapshot differs from the supplied survey".into(),
            ));
        }
        let expected_ids = manifest_observation_ids(&capture_manifest);
        let snapshot_ids: BTreeSet<_> = loaded_snapshot
            .survey
            .associations()
            .iter()
            .map(|association| association.observation_id())
            .collect();
        if !expected_ids.is_subset(&snapshot_ids) {
            return Err(StoreError::Corrupt(
                "capture publication snapshot omits a manifest observation association".into(),
            ));
        }
        let preflight_record = self
            .capture_publication(manifest_hash)?
            .ok_or_else(|| StoreError::Corrupt("capture publication manifest is missing".into()))?;
        let preflight_chunk = if expected_ids.is_empty() {
            None
        } else {
            let chunk_hash = preflight_record.chunk_hash.as_deref().ok_or_else(|| {
                StoreError::Corrupt(
                    "capture snapshot cannot link before its observation chunk".into(),
                )
            })?;
            Some(self.read_observation_chunk(chunk_hash)?)
        };
        if let Some(chunk) = &preflight_chunk {
            validate_snapshot_association_closure(&loaded_snapshot.survey, chunk, &expected_ids)?;
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let manifest = load_manifest(&transaction)?;
        let mut record = read_record(&transaction, manifest_hash)?
            .ok_or_else(|| StoreError::Corrupt("capture publication manifest is missing".into()))?;
        validate_record_shape(&record)?;
        if record.project_id != manifest.project_id {
            return Err(StoreError::Corrupt(
                "capture publication belongs to another project".into(),
            ));
        }
        if record.chunk_hash != preflight_record.chunk_hash {
            return Err(StoreError::Corrupt(
                "capture publication chunk changed during snapshot validation".into(),
            ));
        }
        if record.observation_count > 0 && record.chunk_hash.is_none() {
            return Err(StoreError::Corrupt(
                "capture snapshot cannot link before its observation chunk".into(),
            ));
        }
        let snapshot_text: String = snapshot_id.into();
        let stored_project: Option<String> = transaction
            .query_row(
                "SELECT project_id FROM survey_snapshots WHERE snapshot_id=?1",
                [&snapshot_text],
                |row| row.get(0),
            )
            .optional()?;
        if stored_project.as_deref() != Some(String::from(manifest.project_id).as_str()) {
            return Err(StoreError::Corrupt(
                "capture publication snapshot is missing or belongs to another project".into(),
            ));
        }
        if let Some(existing) = record.snapshot_id
            && existing != snapshot_id
        {
            return Err(StoreError::Corrupt(
                "capture publication already links another snapshot".into(),
            ));
        }
        record.snapshot_id = Some(snapshot_id);
        if record.status != CapturePublicationStatus::Terminal {
            record.status = CapturePublicationStatus::Complete;
        }
        transaction.execute(
            "UPDATE capture_publications SET snapshot_id=?1,status=?2,revision=?3 WHERE manifest_hash=?4",
            (
                snapshot_text,
                record.status.as_str(),
                i64::try_from(record.revision)
                    .map_err(|_| StoreError::Invalid("manifest revision exhausted".into()))?,
                manifest_hash,
            ),
        )?;
        let stored = read_record(&transaction, manifest_hash)?
            .ok_or_else(|| StoreError::Corrupt("capture publication link disappeared".into()))?;
        transaction.commit()?;
        Ok(stored)
    }

    pub fn capture_publication(
        &self,
        manifest_hash: &str,
    ) -> Result<Option<CapturePublicationRecord>> {
        validate_hash(manifest_hash)?;
        if !sqlite_guard::has_capture_publication_schema(&self.connection)? {
            return Ok(None);
        }
        self.start_operation()?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        let artifact = manifest.artifacts.get(manifest_hash).ok_or_else(|| {
            StoreError::Corrupt("capture publication manifest artifact is missing".into())
        })?;
        if artifact.kind != crate::ArtifactKind::RawCapture
            || artifact.media_type != CAPTURE_MANIFEST_MEDIA_TYPE
            || artifact.bytes > MAX_CAPTURE_MANIFEST_BYTES
        {
            return Err(StoreError::Corrupt(
                "capture publication manifest artifact has unexpected semantics".into(),
            ));
        }
        let record = read_record(&transaction, manifest_hash)?;
        if let Some(record) = &record {
            validate_record_shape(record)?;
            if record.project_id != manifest.project_id {
                return Err(StoreError::Corrupt(
                    "capture publication belongs to another project".into(),
                ));
            }
            if let Some(chunk_hash) = &record.chunk_hash {
                let chunk_count: Option<i64> = transaction
                    .query_row(
                        "SELECT row_count FROM observation_chunks WHERE chunk_hash=?1",
                        [chunk_hash],
                        |row| row.get(0),
                    )
                    .optional()?;
                if chunk_count
                    != Some(i64::try_from(record.observation_count).map_err(|_| {
                        StoreError::Invalid("observation count exceeds SQLite integer".into())
                    })?)
                {
                    return Err(StoreError::Corrupt(
                        "capture publication chunk link is missing or inconsistent".into(),
                    ));
                }
            }
            if let Some(snapshot_id) = record.snapshot_id {
                let snapshot_text: String = snapshot_id.into();
                let snapshot_project: Option<String> = transaction
                    .query_row(
                        "SELECT project_id FROM survey_snapshots WHERE snapshot_id=?1",
                        [&snapshot_text],
                        |row| row.get(0),
                    )
                    .optional()?;
                if snapshot_project.as_deref() != Some(String::from(manifest.project_id).as_str()) {
                    return Err(StoreError::Corrupt(
                        "capture publication snapshot link is missing or inconsistent".into(),
                    ));
                }
            }
        }
        transaction.commit()?;
        if let Some(record) = &record {
            // The SQLite row is only a bounded index. Re-read the immutable
            // manifest artifact after leaving the transaction so this query
            // also proves the stored canonical bytes still match its hash.
            let bytes = self.read_registered_artifact(manifest_hash, artifact)?;
            let decoded =
                CaptureManifest::from_canonical_bytes(&bytes).map_err(StoreError::Corrupt)?;
            let expected_raw_record_count = match decoded.raw_source_disposition() {
                RawSourceDisposition::Retained => decoded.source_records().len() as u64,
                RawSourceDisposition::NotRetained => 0,
            };
            let expected_status = if decoded.terminal() {
                CapturePublicationStatus::Terminal
            } else if record.snapshot_id.is_some() {
                CapturePublicationStatus::Complete
            } else if record.chunk_hash.is_some() {
                CapturePublicationStatus::Chunk
            } else {
                CapturePublicationStatus::Manifest
            };
            if record.observation_count != u64::from(decoded.completion().observation_count())
                || record.raw_record_count != expected_raw_record_count
                || record.status != expected_status
            {
                return Err(StoreError::Corrupt(
                    "capture publication row disagrees with its canonical manifest".into(),
                ));
            }
            let expected_ids = manifest_observation_ids(&decoded);
            let verified_chunk = if let Some(chunk_hash) = &record.chunk_hash {
                let chunk = self.read_observation_chunk(chunk_hash)?;
                let chunk_ids: BTreeSet<_> = chunk
                    .iter()
                    .map(|observation| observation.data().id)
                    .collect();
                if chunk.len() as u64 != record.observation_count || chunk_ids != expected_ids {
                    return Err(StoreError::Corrupt(
                        "capture publication chunk identities differ from its manifest".into(),
                    ));
                }
                Some(chunk)
            } else {
                None
            };
            if let Some(snapshot_id) = record.snapshot_id {
                let loaded = self.load_survey_snapshot(snapshot_id)?;
                let snapshot_ids: BTreeSet<_> = loaded
                    .survey
                    .associations()
                    .iter()
                    .map(|association| association.observation_id())
                    .collect();
                if !expected_ids.is_subset(&snapshot_ids) {
                    return Err(StoreError::Corrupt(
                        "capture publication snapshot omits a manifest observation association"
                            .into(),
                    ));
                }
                if !expected_ids.is_empty() {
                    let chunk = verified_chunk.as_ref().ok_or_else(|| {
                        StoreError::Corrupt(
                            "capture publication snapshot is missing its observation chunk".into(),
                        )
                    })?;
                    validate_snapshot_association_closure(&loaded.survey, chunk, &expected_ids)?;
                }
            }
        }
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::CapturePublicationStatus;

    #[test]
    fn status_wire_is_closed() {
        assert!(matches!(
            CapturePublicationStatus::parse("complete"),
            Ok(CapturePublicationStatus::Complete)
        ));
        assert!(CapturePublicationStatus::parse("future").is_err());
    }
}
