//! Durable, domain-owned provenance for one acquisition session.
//!
//! A session record is deliberately stored in its own canonical BLOB/index
//! table.  It is not an annotation: its manifest, identity mapping, privacy,
//! and terminal evidence have one checked domain representation.

use crate::{
    ArtifactKind, Bundle, Cancellation, NeverCancel, OpenMode, Result, StoreError, content_hash,
    manifest::validate_hash, sqlite_guard,
};
use kyberia_domain::{
    capture::{CaptureManifest, CaptureTerminalStatus},
    capture_session::{
        CaptureSessionRecordV1, MAX_CAPTURE_SESSION_BYTES, MappingEvidenceSchemaVersion,
    },
    identity::{ClockEpochId, CollectorId, ContentHash, SessionId},
    observation::ObservationEnvelope,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::BTreeMap;

pub const MAX_CAPTURE_SESSIONS_PER_PAGE: u16 = 128;
pub(crate) const CAPTURE_SESSION_SCHEMA_VERSION: u32 = 1;

/// A checked request to associate one domain session record with the exact
/// canonical manifest and envelopes that produced it.
pub struct CaptureSessionRegistration<'a> {
    record: CaptureSessionRecordV1,
    manifest: CaptureManifest,
    observations: &'a [ObservationEnvelope],
    published_utc_ms: i64,
}

impl<'a> CaptureSessionRegistration<'a> {
    pub fn new(
        record: CaptureSessionRecordV1,
        manifest: CaptureManifest,
        observations: &'a [ObservationEnvelope],
        published_utc_ms: i64,
    ) -> Result<Self> {
        if published_utc_ms < 0 {
            return Err(StoreError::Invalid(
                "capture session publication time must be UTC".into(),
            ));
        }
        record
            .validate_against_manifest(&manifest, observations)
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        let bytes = record.canonical_bytes().map_err(StoreError::Invalid)?;
        if bytes.len() > MAX_CAPTURE_SESSION_BYTES {
            return Err(StoreError::Invalid(
                "capture session record exceeds resource limit".into(),
            ));
        }
        Ok(Self {
            record,
            manifest,
            observations,
            published_utc_ms,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureSessionReceipt {
    pub session_id: SessionId,
    pub manifest_hash: ContentHash,
    pub record_hash: ContentHash,
    pub revision: u64,
}

#[derive(Debug)]
struct StoredSessionRow {
    session_id: String,
    schema_version: i64,
    collector_id: String,
    clock_epoch_id: String,
    process_session_uuid: String,
    source_clock_uuid: String,
    manifest_hash: String,
    record_hash: String,
    terminal_status: String,
    terminal_reason: String,
    partial: i64,
    observation_count: i64,
    exit_code: i64,
    registry_version: String,
    source_mapping_count: i64,
    observation_mapping_count: i64,
    privacy_hash: String,
    published_utc_ms: i64,
    revision: i64,
    canonical_bytes: Vec<u8>,
}

fn row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredSessionRow> {
    Ok(StoredSessionRow {
        session_id: row.get(0)?,
        schema_version: row.get(1)?,
        collector_id: row.get(2)?,
        clock_epoch_id: row.get(3)?,
        process_session_uuid: row.get(4)?,
        source_clock_uuid: row.get(5)?,
        manifest_hash: row.get(6)?,
        record_hash: row.get(7)?,
        terminal_status: row.get(8)?,
        terminal_reason: row.get(9)?,
        partial: row.get(10)?,
        observation_count: row.get(11)?,
        exit_code: row.get(12)?,
        registry_version: row.get(13)?,
        source_mapping_count: row.get(14)?,
        observation_mapping_count: row.get(15)?,
        privacy_hash: row.get(16)?,
        published_utc_ms: row.get(17)?,
        revision: row.get(18)?,
        canonical_bytes: row.get(19)?,
    })
}

const SELECT_COLUMNS: &str = "session_id,schema_version,collector_id,clock_epoch_id,process_session_uuid,source_clock_uuid,manifest_hash,record_hash,terminal_status,terminal_reason,partial,observation_count,exit_code,registry_version,source_mapping_count,observation_mapping_count,privacy_hash,published_utc_ms,revision,canonical_bytes";

fn read_row(
    connection: &rusqlite::Connection,
    session_id: &str,
) -> Result<Option<StoredSessionRow>> {
    connection
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM capture_sessions WHERE session_id=?1"),
            [session_id],
            row_from_sql,
        )
        .optional()
        .map_err(StoreError::from)
}

fn read_row_transaction(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<Option<StoredSessionRow>> {
    transaction
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM capture_sessions WHERE session_id=?1"),
            [session_id],
            row_from_sql,
        )
        .optional()
        .map_err(StoreError::from)
}

fn status_text(status: CaptureTerminalStatus) -> &'static str {
    match status {
        CaptureTerminalStatus::Ok => "ok",
        CaptureTerminalStatus::Partial => "partial",
        CaptureTerminalStatus::PermissionRequired => "permission_required",
        CaptureTerminalStatus::Unsupported => "unsupported",
        CaptureTerminalStatus::Unavailable => "unavailable",
        CaptureTerminalStatus::Error => "error",
        CaptureTerminalStatus::Timeout => "timeout",
        CaptureTerminalStatus::Cancelled => "cancelled",
    }
}

fn parse_status(value: &str) -> Result<CaptureTerminalStatus> {
    match value {
        "ok" => Ok(CaptureTerminalStatus::Ok),
        "partial" => Ok(CaptureTerminalStatus::Partial),
        "permission_required" => Ok(CaptureTerminalStatus::PermissionRequired),
        "unsupported" => Ok(CaptureTerminalStatus::Unsupported),
        "unavailable" => Ok(CaptureTerminalStatus::Unavailable),
        "error" => Ok(CaptureTerminalStatus::Error),
        "timeout" => Ok(CaptureTerminalStatus::Timeout),
        "cancelled" => Ok(CaptureTerminalStatus::Cancelled),
        _ => Err(StoreError::Corrupt(
            "capture session has an unknown terminal status".into(),
        )),
    }
}

fn invalid_row(detail: impl Into<String>) -> StoreError {
    StoreError::Corrupt(format!("capture session index: {}", detail.into()))
}

fn check_cancel<C: Cancellation + ?Sized>(cancel: &C) -> Result<()> {
    if cancel.is_cancelled() {
        Err(StoreError::Cancelled)
    } else {
        Ok(())
    }
}

fn checked_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| invalid_row(format!("{field} is negative")))
}

fn privacy_hash(record: &CaptureSessionRecordV1) -> Result<String> {
    let bytes = serde_json::to_vec(record.privacy())?;
    Ok(content_hash(&bytes))
}

fn validate_projection(row: &StoredSessionRow) -> Result<CaptureSessionRecordV1> {
    if row.schema_version != i64::from(CAPTURE_SESSION_SCHEMA_VERSION)
        || row.partial != 0 && row.partial != 1
    {
        return Err(invalid_row("schema or partial flag is invalid"));
    }
    let record =
        CaptureSessionRecordV1::from_canonical_bytes(&row.canonical_bytes).map_err(invalid_row)?;
    if content_hash(&row.canonical_bytes) != row.record_hash {
        return Err(invalid_row("canonical record hash differs from bytes"));
    }
    let session_id = String::from(record.session_id());
    let collector_id = String::from(record.collector_id());
    let clock_epoch_id = String::from(record.clock_epoch_id());
    let manifest_hash = String::from(record.manifest_hash());
    let expected_privacy_hash = privacy_hash(&record)?;
    let expected_partial = i64::from(record.partial());
    let expected_observation_count = i64::from(record.observation_count());
    let expected_source_mapping_count = i64::try_from(record.mapping().source_mappings().len())
        .map_err(|_| invalid_row("source mapping count is too large"))?;
    let expected_observation_mapping_count =
        i64::try_from(record.mapping().observation_mappings().len())
            .map_err(|_| invalid_row("observation mapping count is too large"))?;
    checked_u64(row.revision, "revision")?;
    if row.session_id != session_id
        || row.collector_id != collector_id
        || row.clock_epoch_id != clock_epoch_id
        || row.process_session_uuid != record.process_session_uuid().as_str()
        || row.source_clock_uuid != record.source_clock_uuid().as_str()
        || row.manifest_hash != manifest_hash
        || row.terminal_status != status_text(record.terminal())
        || row.terminal_reason != record.terminal_reason().as_str()
        || row.partial != expected_partial
        || row.observation_count != expected_observation_count
        || row.exit_code != i64::from(record.exit_code())
        || row.registry_version != record.mapping().registry_version().as_str()
        || row.source_mapping_count != expected_source_mapping_count
        || row.observation_mapping_count != expected_observation_mapping_count
        || row.privacy_hash != expected_privacy_hash
        || row.published_utc_ms < 0
        || row.revision <= 0
        || row.record_hash.len() != 64
    {
        return Err(invalid_row(
            "indexed projection differs from canonical bytes",
        ));
    }
    parse_status(&row.terminal_status)?;
    validate_hash(&row.manifest_hash)?;
    validate_hash(&row.record_hash)?;
    ContentHash::try_from(row.manifest_hash.clone())
        .map_err(|_| invalid_row("manifest hash is invalid"))?;
    ContentHash::try_from(row.record_hash.clone())
        .map_err(|_| invalid_row("record hash is invalid"))?;
    SessionId::try_from(row.session_id.clone())
        .map_err(|_| invalid_row("session ID is invalid"))?;
    CollectorId::try_from(row.collector_id.clone())
        .map_err(|_| invalid_row("collector ID is invalid"))?;
    ClockEpochId::try_from(row.clock_epoch_id.clone())
        .map_err(|_| invalid_row("clock epoch ID is invalid"))?;
    if MappingEvidenceSchemaVersion::V1 != record.mapping().schema_version() {
        return Err(invalid_row("mapping schema version is unsupported"));
    }
    Ok(record)
}

fn load_manifest(bundle: &Bundle, hash: &str) -> Result<CaptureManifest> {
    validate_hash(hash)?;
    let bundle_manifest = bundle.manifest()?;
    let entry = bundle_manifest.artifacts.get(hash).ok_or_else(|| {
        StoreError::Corrupt("capture session manifest artifact is missing".into())
    })?;
    if entry.kind != ArtifactKind::RawCapture
        || entry.media_type != crate::capture_publication::CAPTURE_MANIFEST_MEDIA_TYPE
    {
        return Err(StoreError::Corrupt(
            "capture session manifest artifact has unexpected semantics".into(),
        ));
    }
    let bytes = bundle.read_registered_artifact(hash, entry)?;
    if content_hash(&bytes) != hash {
        return Err(StoreError::Corrupt(
            "capture session manifest hash differs from bytes".into(),
        ));
    }
    CaptureManifest::from_canonical_bytes(&bytes).map_err(StoreError::Corrupt)
}

fn ordered_observations(
    manifest: &CaptureManifest,
    observations: Vec<ObservationEnvelope>,
) -> Result<Vec<ObservationEnvelope>> {
    let mut by_id = BTreeMap::new();
    for observation in observations {
        if by_id.insert(observation.data().id, observation).is_some() {
            return Err(StoreError::Corrupt(
                "capture session chunk has duplicate observation IDs".into(),
            ));
        }
    }
    let mut ordered = Vec::with_capacity(by_id.len());
    for id in manifest.observation_ids_in_source_order() {
        ordered.push(by_id.remove(id).ok_or_else(|| {
            StoreError::Corrupt("capture session chunk omits a manifest observation".into())
        })?);
    }
    if !by_id.is_empty() {
        return Err(StoreError::Corrupt(
            "capture session chunk has an unlisted observation".into(),
        ));
    }
    Ok(ordered)
}

fn load_manifest_observations<C: Cancellation + ?Sized>(
    bundle: &Bundle,
    record: &CaptureSessionRecordV1,
    cancel: &C,
) -> Result<(CaptureManifest, Vec<ObservationEnvelope>)> {
    check_cancel(cancel)?;
    let hash = String::from(record.manifest_hash());
    let manifest = load_manifest(bundle, &hash)?;
    check_cancel(cancel)?;
    let publication = bundle
        .capture_publication(&hash)?
        .ok_or_else(|| StoreError::Corrupt("capture session publication row is missing".into()))?;
    let observations = if record.observation_count() == 0 {
        if publication.chunk_hash.is_some() {
            return Err(StoreError::Corrupt(
                "empty capture session publication unexpectedly has a chunk".into(),
            ));
        }
        Vec::new()
    } else {
        let chunk_hash = publication.chunk_hash.as_deref().ok_or_else(|| {
            StoreError::Corrupt("capture session publication has no observation chunk".into())
        })?;
        check_cancel(cancel)?;
        ordered_observations(&manifest, bundle.read_observation_chunk(chunk_hash)?)?
    };
    check_cancel(cancel)?;
    record
        .validate_against_manifest(&manifest, &observations)
        .map_err(|error| StoreError::Corrupt(error.to_string()))?;
    check_cancel(cancel)?;
    Ok((manifest, observations))
}

fn receipt(
    record: &CaptureSessionRecordV1,
    record_hash: &str,
    revision: i64,
) -> Result<CaptureSessionReceipt> {
    let revision = checked_u64(revision, "revision")?;
    if revision == 0 {
        return Err(invalid_row("revision is zero"));
    }
    Ok(CaptureSessionReceipt {
        session_id: record.session_id(),
        manifest_hash: record.manifest_hash(),
        record_hash: ContentHash::try_from(record_hash.to_owned())
            .map_err(|_| invalid_row("record hash is invalid"))?,
        revision,
    })
}

impl Bundle {
    /// Store one canonical session record after the capture manifest and its
    /// canonical observations have been published. The record/index write is
    /// exact-idempotent and does not use the generic annotation artifact path.
    pub fn register_capture_session(
        &mut self,
        registration: CaptureSessionRegistration<'_>,
    ) -> Result<CaptureSessionReceipt> {
        self.register_capture_session_with_cancel(registration, &NeverCancel)
    }

    /// Cancellation-aware session registration. Cancellation before commit
    /// rolls back the SQLite row; cancellation after commit returns a retryable
    /// error while the complete immutable row remains discoverable.
    pub fn register_capture_session_with_cancel<C: Cancellation + ?Sized>(
        &mut self,
        registration: CaptureSessionRegistration<'_>,
        cancel: &C,
    ) -> Result<CaptureSessionReceipt> {
        check_cancel(cancel)?;
        if self.mode == OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        if !sqlite_guard::has_capture_session_schema(&self.connection)? {
            return Err(StoreError::UnsupportedVersion(
                CAPTURE_SESSION_SCHEMA_VERSION,
            ));
        }
        let CaptureSessionRegistration {
            record,
            manifest,
            observations,
            published_utc_ms,
        } = registration;
        check_cancel(cancel)?;
        let canonical_bytes = record.canonical_bytes().map_err(StoreError::Invalid)?;
        let record_hash = content_hash(&canonical_bytes);
        let manifest_hash = String::from(record.manifest_hash());
        let manifest_bytes = manifest.canonical_bytes().map_err(StoreError::Invalid)?;
        if content_hash(&manifest_bytes) != manifest_hash {
            return Err(StoreError::Invalid(
                "capture session manifest hash differs from supplied manifest".into(),
            ));
        }
        let stored_manifest = load_manifest(self, &manifest_hash)?;
        check_cancel(cancel)?;
        if stored_manifest != manifest {
            return Err(StoreError::Corrupt(
                "capture session supplied manifest differs from stored bytes".into(),
            ));
        }
        let publication = self.capture_publication(&manifest_hash)?.ok_or_else(|| {
            StoreError::Corrupt("capture session publication row is missing".into())
        })?;
        if record.observation_count() > 0 && publication.chunk_hash.is_none() {
            return Err(StoreError::Corrupt(
                "capture session requires a published observation chunk".into(),
            ));
        }
        if record.observation_count() == 0 && publication.chunk_hash.is_some() {
            return Err(StoreError::Corrupt(
                "empty capture session cannot link an observation chunk".into(),
            ));
        }
        record
            .validate_against_manifest(&stored_manifest, observations)
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
        check_cancel(cancel)?;
        if let Some(chunk_hash) = publication.chunk_hash.as_deref() {
            let stored_observations =
                ordered_observations(&stored_manifest, self.read_observation_chunk(chunk_hash)?)?;
            if stored_observations != observations {
                return Err(StoreError::Corrupt(
                    "capture session observations differ from published chunk".into(),
                ));
            }
        }
        check_cancel(cancel)?;
        let session_id = String::from(record.session_id());
        let privacy_hash = privacy_hash(&record)?;
        let source_mapping_count = i64::try_from(record.mapping().source_mappings().len())
            .map_err(|_| {
                StoreError::Invalid("source mapping count exceeds SQLite integer".into())
            })?;
        let observation_mapping_count =
            i64::try_from(record.mapping().observation_mappings().len()).map_err(|_| {
                StoreError::Invalid("observation mapping count exceeds SQLite integer".into())
            })?;
        self.start_operation()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_cancel(cancel)?;
        if let Some(existing) = read_row_transaction(&transaction, &session_id)? {
            let existing_record = validate_projection(&existing)?;
            if existing_record != record
                || existing.manifest_hash != manifest_hash
                || existing.record_hash != record_hash
            {
                return Err(StoreError::Invalid(
                    "capture session ID already has different canonical evidence".into(),
                ));
            }
            let stored = read_row_transaction(&transaction, &session_id)?.ok_or_else(|| {
                StoreError::Corrupt("capture session idempotent row disappeared".into())
            })?;
            let result = receipt(&existing_record, &stored.record_hash, stored.revision)?;
            check_cancel(cancel)?;
            transaction.commit()?;
            check_cancel(cancel)?;
            let _ = self.read_capture_session_with_cancel(result.session_id, cancel)?;
            return Ok(result);
        }
        let manifest_owner: Option<String> = transaction
            .query_row(
                "SELECT session_id FROM capture_sessions WHERE manifest_hash=?1",
                [&manifest_hash],
                |row| row.get(0),
            )
            .optional()?;
        if manifest_owner.is_some() {
            return Err(StoreError::Invalid(
                "capture manifest is already bound to another session".into(),
            ));
        }
        // Session rows are immutable. The stable session-ID cursor, rather
        // than this projection-local marker, defines listing order; keeping a
        // positive marker preserves room for a reviewed future revision.
        let revision = 1_i64;
        transaction.execute(
            "INSERT INTO capture_sessions (session_id,schema_version,collector_id,clock_epoch_id,process_session_uuid,source_clock_uuid,manifest_hash,record_hash,terminal_status,terminal_reason,partial,observation_count,exit_code,registry_version,source_mapping_count,observation_mapping_count,privacy_hash,published_utc_ms,revision,canonical_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
            params![
                &session_id,
                i64::from(CAPTURE_SESSION_SCHEMA_VERSION),
                String::from(record.collector_id()),
                String::from(record.clock_epoch_id()),
                record.process_session_uuid().as_str(),
                record.source_clock_uuid().as_str(),
                &manifest_hash,
                &record_hash,
                status_text(record.terminal()),
                record.terminal_reason().as_str(),
                i64::from(record.partial()),
                i64::from(record.observation_count()),
                i64::from(record.exit_code()),
                record.mapping().registry_version().as_str(),
                source_mapping_count,
                observation_mapping_count,
                &privacy_hash,
                published_utc_ms,
                revision,
                &canonical_bytes,
            ],
        )?;
        let stored = read_row_transaction(&transaction, &session_id)?
            .ok_or_else(|| StoreError::Corrupt("capture session insert disappeared".into()))?;
        let stored_record = validate_projection(&stored)?;
        if stored_record != record {
            return Err(StoreError::Corrupt(
                "capture session insert readback differs from request".into(),
            ));
        }
        let result = receipt(&stored_record, &stored.record_hash, stored.revision)?;
        check_cancel(cancel)?;
        transaction.commit()?;
        check_cancel(cancel)?;
        let readback = self.read_capture_session_with_cancel(result.session_id, cancel)?;
        if readback.as_ref() != Some(&record) {
            return Err(StoreError::Corrupt(
                "capture session publication readback differs from request".into(),
            ));
        }
        Ok(result)
    }

    /// Read one session after validating its canonical BLOB, all indexed
    /// projections, the manifest hash, and the linked observation closure.
    pub fn read_capture_session(
        &self,
        session_id: SessionId,
    ) -> Result<Option<CaptureSessionRecordV1>> {
        self.read_capture_session_with_cancel(session_id, &NeverCancel)
    }

    /// Cancellation-aware read that never returns a partially validated row.
    pub fn read_capture_session_with_cancel<C: Cancellation + ?Sized>(
        &self,
        session_id: SessionId,
        cancel: &C,
    ) -> Result<Option<CaptureSessionRecordV1>> {
        check_cancel(cancel)?;
        if !sqlite_guard::has_capture_session_schema(&self.connection)? {
            return Ok(None);
        }
        self.start_operation()?;
        let session_text = String::from(session_id);
        let Some(row) = read_row(&self.connection, &session_text)? else {
            return Ok(None);
        };
        check_cancel(cancel)?;
        let record = validate_projection(&row)?;
        if record.session_id() != session_id {
            return Err(invalid_row("session lookup identity differs from row"));
        }
        let _ = load_manifest_observations(self, &record, cancel)?;
        Ok(Some(record))
    }

    /// Return a bounded page ordered by canonical session ID. `after` is an
    /// exclusive stable cursor, so SQLite incidental row order is irrelevant.
    pub fn list_capture_sessions(
        &self,
        limit: u16,
        after: Option<SessionId>,
    ) -> Result<Vec<CaptureSessionRecordV1>> {
        self.list_capture_sessions_with_cancel(limit, after, &NeverCancel)
    }

    /// Cancellation-aware bounded listing. A cancelled page never returns a
    /// prefix that could be mistaken for a complete result.
    pub fn list_capture_sessions_with_cancel<C: Cancellation + ?Sized>(
        &self,
        limit: u16,
        after: Option<SessionId>,
        cancel: &C,
    ) -> Result<Vec<CaptureSessionRecordV1>> {
        check_cancel(cancel)?;
        if limit == 0 || limit > MAX_CAPTURE_SESSIONS_PER_PAGE {
            return Err(StoreError::Invalid(
                "capture session page size exceeds resource limit".into(),
            ));
        }
        if !sqlite_guard::has_capture_session_schema(&self.connection)? {
            return Ok(Vec::new());
        }
        self.start_operation()?;
        let after_text = after.map(String::from);
        let sql = if after_text.is_some() {
            format!(
                "SELECT {SELECT_COLUMNS} FROM capture_sessions WHERE session_id>?1 ORDER BY session_id LIMIT ?2"
            )
        } else {
            format!("SELECT {SELECT_COLUMNS} FROM capture_sessions ORDER BY session_id LIMIT ?1")
        };
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = if let Some(after_text) = after_text {
            statement.query((&after_text, i64::from(limit)))?
        } else {
            statement.query([i64::from(limit)])?
        };
        let mut stored_rows = Vec::new();
        while let Some(row) = rows.next()? {
            check_cancel(cancel)?;
            stored_rows.push(row_from_sql(row)?);
        }
        drop(rows);
        drop(statement);
        stored_rows
            .into_iter()
            .map(|row| {
                check_cancel(cancel)?;
                let record = validate_projection(&row)?;
                let _ = load_manifest_observations(self, &record, cancel)?;
                Ok(record)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_domain::{
        capability::{CapabilityDocument, RawPayloadPolicy},
        capture::{CAPTURE_MANIFEST_METHOD_VERSION, CaptureCompletion, RawSourceDisposition},
        evidence::{Evidence, SchemaVersion, UnknownReason},
        identity::{ClockEpochId, CollectorId, ProjectId, Text},
        observation::{IdentifierPolicy, PrivacyState, SourceKind},
        time::CaptureTime,
    };
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, fs, path::PathBuf};

    fn retained_root() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn project_id() -> ProjectId {
        ProjectId::from_bytes([1; 16]).unwrap()
    }

    fn privacy() -> PrivacyState {
        PrivacyState {
            policy_version: Text::new("privacy/v1").unwrap(),
            identifiers: IdentifierPolicy::Redacted,
            payload: kyberia_domain::observation::PayloadRetention::Discarded,
        }
    }

    fn new_manifest() -> CaptureManifest {
        let collector = CollectorId::from_bytes([2; 16]).unwrap();
        CaptureManifest::new(
            SchemaVersion::V1,
            Text::new(CAPTURE_MANIFEST_METHOD_VERSION).unwrap(),
            SourceKind::NativeApi,
            ContentHash::from_sha256([9; 32]),
            Evidence::Known(CapabilityDocument {
                schema_version: SchemaVersion::V1,
                collector_id: collector,
                collector_version: Text::new("test/v1").unwrap(),
                probed_at: CaptureTime {
                    wall: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    monotonic: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    synchronization: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                },
                entries: BTreeMap::new(),
                raw_payload_policy: RawPayloadPolicy::Discard,
            }),
            CaptureCompletion::new(
                CaptureTerminalStatus::PermissionRequired,
                Text::new("permission required").unwrap(),
                false,
                0,
            )
            .unwrap(),
            vec![],
            RawSourceDisposition::NotRetained,
            vec![],
        )
        .unwrap()
    }

    fn make_record(
        manifest: &CaptureManifest,
        exit_code: i32,
        registry_version: &str,
    ) -> CaptureSessionRecordV1 {
        let bytes = manifest.canonical_bytes().unwrap();
        CaptureSessionRecordV1::new(
            SessionId::from_bytes([3; 16]).unwrap(),
            CollectorId::from_bytes([2; 16]).unwrap(),
            ClockEpochId::from_bytes([4; 16]).unwrap(),
            kyberia_domain::capture_session::NativeUuid::new(
                "00000000-0000-4000-8000-000000000001",
            )
            .unwrap(),
            kyberia_domain::capture_session::NativeUuid::new(
                "00000000-0000-4000-8000-000000000002",
            )
            .unwrap(),
            ContentHash::from_sha256(Sha256::digest(bytes).into()),
            kyberia_domain::capture_session::MappingEvidenceV1::new(
                Text::new(registry_version).unwrap(),
                vec![],
                vec![],
            )
            .unwrap(),
            privacy(),
            CaptureTerminalStatus::PermissionRequired,
            Text::new("permission required").unwrap(),
            false,
            0,
            exit_code,
        )
        .unwrap()
    }

    fn new_bundle() -> (PathBuf, Bundle) {
        let path = tempfile::Builder::new()
            .prefix("capture-session-store-")
            .tempdir_in(retained_root())
            .unwrap()
            .keep()
            .join("project");
        let bundle = Bundle::create(&path, project_id(), "Capture session".into(), 1).unwrap();
        (path, bundle)
    }

    fn publish_manifest(bundle: &mut Bundle, manifest: CaptureManifest) {
        bundle
            .persist_capture_manifest(
                crate::CaptureManifestRegistration::new(
                    manifest,
                    Text::new("capture-session-test").unwrap(),
                    1,
                )
                .unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn empty_session_registers_reads_reopens_and_lists_with_cursor() {
        let (path, mut bundle) = new_bundle();
        let manifest = new_manifest();
        let record = make_record(&manifest, 77, "registry/v1");
        publish_manifest(&mut bundle, manifest.clone());
        let receipt = bundle
            .register_capture_session(
                CaptureSessionRegistration::new(record.clone(), manifest, &[], 1).unwrap(),
            )
            .unwrap();
        assert_eq!(
            bundle.read_capture_session(receipt.session_id).unwrap(),
            Some(record.clone())
        );
        assert_eq!(
            bundle.list_capture_sessions(1, None).unwrap(),
            vec![record.clone()]
        );
        assert!(
            bundle
                .list_capture_sessions(1, Some(receipt.session_id))
                .unwrap()
                .is_empty()
        );
        drop(bundle);
        let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert_eq!(
            reopened.read_capture_session(receipt.session_id).unwrap(),
            Some(record)
        );
    }

    #[test]
    fn retry_is_idempotent_and_conflicting_identity_is_rejected() {
        let (_path, mut bundle) = new_bundle();
        let manifest = new_manifest();
        let record = make_record(&manifest, 77, "registry/v1");
        publish_manifest(&mut bundle, manifest.clone());
        let first = bundle
            .register_capture_session(
                CaptureSessionRegistration::new(record.clone(), manifest.clone(), &[], 1).unwrap(),
            )
            .unwrap();
        let second = bundle
            .register_capture_session(
                CaptureSessionRegistration::new(record.clone(), manifest.clone(), &[], 1).unwrap(),
            )
            .unwrap();
        assert_eq!(first, second);
        let conflicting = make_record(&manifest, 77, "registry/v2");
        let error = bundle
            .register_capture_session(
                CaptureSessionRegistration::new(conflicting, manifest, &[], 1).unwrap(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("different canonical evidence"));
    }

    #[test]
    fn missing_manifest_is_rejected_before_session_row() {
        let (_path, mut bundle) = new_bundle();
        let manifest = new_manifest();
        let record = make_record(&manifest, 77, "registry/v1");
        let error = bundle
            .register_capture_session(
                CaptureSessionRegistration::new(record.clone(), manifest, &[], 1).unwrap(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("manifest artifact is missing"));
        assert!(
            bundle
                .read_capture_session(record.session_id())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn invalid_page_size_and_read_only_write_are_rejected() {
        let (path, bundle) = new_bundle();
        assert!(bundle.list_capture_sessions(0, None).is_err());
        assert!(
            bundle
                .list_capture_sessions(MAX_CAPTURE_SESSIONS_PER_PAGE + 1, None)
                .is_err()
        );
        drop(bundle);
        let mut readonly = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        let manifest = new_manifest();
        let record = make_record(&manifest, 77, "registry/v1");
        assert!(
            readonly
                .register_capture_session(
                    CaptureSessionRegistration::new(record, manifest, &[], 1).unwrap()
                )
                .is_err()
        );
    }

    #[test]
    fn cancellation_before_session_publication_leaves_no_row_or_partial_page() {
        let (_path, mut bundle) = new_bundle();
        let manifest = new_manifest();
        let record = make_record(&manifest, 77, "registry/v1");
        publish_manifest(&mut bundle, manifest.clone());
        let cancelled = || true;
        let error = bundle
            .register_capture_session_with_cancel(
                CaptureSessionRegistration::new(record.clone(), manifest, &[], 1).unwrap(),
                &cancelled,
            )
            .unwrap_err();
        assert!(matches!(error, StoreError::Cancelled));
        assert!(
            bundle
                .read_capture_session(record.session_id())
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            bundle.read_capture_session_with_cancel(record.session_id(), &cancelled),
            Err(StoreError::Cancelled)
        ));
        assert!(matches!(
            bundle.list_capture_sessions_with_cancel(1, None, &cancelled),
            Err(StoreError::Cancelled)
        ));
    }

    #[test]
    fn readback_rejects_canonical_blob_and_index_projection_tampering() {
        let (path, mut bundle) = new_bundle();
        let manifest = new_manifest();
        let record = make_record(&manifest, 77, "registry/v1");
        publish_manifest(&mut bundle, manifest.clone());
        bundle
            .register_capture_session(
                CaptureSessionRegistration::new(record.clone(), manifest.clone(), &[], 1).unwrap(),
            )
            .unwrap();
        drop(bundle);
        let database = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
        database
            .execute(
                "UPDATE capture_sessions SET canonical_bytes=?1 WHERE session_id=?2",
                rusqlite::params![b"{}".as_slice(), String::from(record.session_id())],
            )
            .unwrap();
        drop(database);
        let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(
            reopened
                .read_capture_session(record.session_id())
                .unwrap_err()
                .to_string()
                .contains("capture session index")
        );

        let (path, mut bundle) = new_bundle();
        let manifest = new_manifest();
        let record = make_record(&manifest, 77, "registry/v1");
        publish_manifest(&mut bundle, manifest.clone());
        bundle
            .register_capture_session(
                CaptureSessionRegistration::new(record.clone(), manifest, &[], 1).unwrap(),
            )
            .unwrap();
        drop(bundle);
        let database = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
        database
            .execute(
                "UPDATE capture_sessions SET collector_id=?1 WHERE session_id=?2",
                rusqlite::params![
                    String::from(CollectorId::from_bytes([8; 16]).unwrap()),
                    String::from(record.session_id())
                ],
            )
            .unwrap();
        drop(database);
        let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(
            reopened
                .read_capture_session(record.session_id())
                .unwrap_err()
                .to_string()
                .contains("indexed projection")
        );
    }

    #[test]
    fn writable_open_migrates_a_v1_bundle_without_guessing_sessions() {
        let (path, bundle) = new_bundle();
        drop(bundle);
        let database = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
        database.execute("DROP TABLE capture_sessions", []).unwrap();
        drop(database);
        let migrated = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
        assert!(migrated.list_capture_sessions(1, None).unwrap().is_empty());
    }

    #[test]
    fn concurrent_same_record_writers_converge_to_one_row() {
        let (path, mut bundle) = new_bundle();
        let manifest = new_manifest();
        publish_manifest(&mut bundle, manifest);
        drop(bundle);
        let first_path = path.clone();
        let second_path = path.clone();
        let first = std::thread::spawn(move || {
            let mut bundle = Bundle::open(&first_path, OpenMode::ReadWrite).unwrap();
            let manifest = new_manifest();
            let record = make_record(&manifest, 77, "registry/v1");
            bundle
                .register_capture_session(
                    CaptureSessionRegistration::new(record, manifest, &[], 1).unwrap(),
                )
                .map(|_| ())
        });
        let second = std::thread::spawn(move || {
            let mut bundle = Bundle::open(&second_path, OpenMode::ReadWrite).unwrap();
            let manifest = new_manifest();
            let record = make_record(&manifest, 77, "registry/v1");
            bundle
                .register_capture_session(
                    CaptureSessionRegistration::new(record, manifest, &[], 1).unwrap(),
                )
                .map(|_| ())
        });
        assert!(first.join().unwrap().is_ok());
        assert!(second.join().unwrap().is_ok());
        let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert_eq!(reopened.list_capture_sessions(1, None).unwrap().len(), 1);
    }
}
