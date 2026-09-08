//! Versioned composition for native normalized observations.
//!
//! The inward contract consumes canonical ReceivedObservation values and a
//! pure PointSurvey. The native adapter's NormalizedCapture is unwrapped only
//! at this composition edge, and the persistence port exposes neither SQLite
//! nor Parquet types to the survey or domain crates.

use kyberia_capture_adapter::macos::{Completion as AdapterCompletion, NormalizedCapture};
use kyberia_domain::{
    capability::CapabilityDocument,
    evidence::{ArtifactReference, Evidence, SchemaVersion},
    identity::{ContentHash, ObservationId, SnapshotId, Text},
    observation::{ObservationEnvelope, PayloadRetention, ReceivedObservation, SourceKind},
};
use kyberia_survey::{PointSurvey, SurveyError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt};

mod bundle;

pub const MAX_BATCH_OBSERVATIONS: usize = 4_096;
pub const MAX_SOURCE_RECORDS: usize = 4_164;
pub const MAX_SOURCE_RECORD_BYTES: usize = 16_384;
pub const MAX_SOURCE_BYTES: u64 = (MAX_SOURCE_RECORDS * MAX_SOURCE_RECORD_BYTES) as u64;
pub const MAX_CAPTURE_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
pub const PIPELINE_METHOD_VERSION: &str = "native-observation-pipeline/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTerminalStatus {
    Ok,
    Partial,
    PermissionRequired,
    Unsupported,
    Unavailable,
    Error,
    Timeout,
    Cancelled,
}

impl From<kyberia_capture_adapter::macos::TerminalStatus> for CaptureTerminalStatus {
    fn from(status: kyberia_capture_adapter::macos::TerminalStatus) -> Self {
        match status {
            kyberia_capture_adapter::macos::TerminalStatus::Ok => Self::Ok,
            kyberia_capture_adapter::macos::TerminalStatus::Partial => Self::Partial,
            kyberia_capture_adapter::macos::TerminalStatus::PermissionRequired => {
                Self::PermissionRequired
            }
            kyberia_capture_adapter::macos::TerminalStatus::Unsupported => Self::Unsupported,
            kyberia_capture_adapter::macos::TerminalStatus::Unavailable => Self::Unavailable,
            kyberia_capture_adapter::macos::TerminalStatus::Error => Self::Error,
            kyberia_capture_adapter::macos::TerminalStatus::Timeout => Self::Timeout,
            kyberia_capture_adapter::macos::TerminalStatus::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureCompletion {
    status: CaptureTerminalStatus,
    reason: Text,
    partial: bool,
    observation_count: u16,
}

impl CaptureCompletion {
    pub const fn status(&self) -> CaptureTerminalStatus {
        self.status
    }

    pub fn reason(&self) -> &Text {
        &self.reason
    }

    pub const fn partial(&self) -> bool {
        self.partial
    }

    pub const fn observation_count(&self) -> u16 {
        self.observation_count
    }
}

impl From<AdapterCompletion> for CaptureCompletion {
    fn from(completion: AdapterCompletion) -> Self {
        Self {
            status: completion.status.into(),
            reason: completion.reason,
            partial: completion.partial,
            observation_count: completion.observation_count,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecordManifest {
    reference: ArtifactReference,
}

impl SourceRecordManifest {
    pub const fn reference(&self) -> &ArtifactReference {
        &self.reference
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawSourceDisposition {
    NotRetained,
    Retained,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureManifest {
    schema_version: SchemaVersion,
    method_version: Text,
    evidence_origin: SourceKind,
    collector_build: ContentHash,
    capabilities: Evidence<CapabilityDocument>,
    completion: CaptureCompletion,
    source_records: Vec<SourceRecordManifest>,
    raw_source_disposition: RawSourceDisposition,
    /// IDs in source stream order; the chunk adapter separately sorts rows by
    /// canonical observation ID for deterministic columnar bytes.
    observation_ids_in_source_order: Vec<ObservationId>,
}

impl CaptureManifest {
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub fn method_version(&self) -> &Text {
        &self.method_version
    }

    pub const fn evidence_origin(&self) -> &SourceKind {
        &self.evidence_origin
    }

    pub const fn collector_build(&self) -> ContentHash {
        self.collector_build
    }

    pub const fn capabilities(&self) -> &Evidence<CapabilityDocument> {
        &self.capabilities
    }

    pub const fn completion(&self) -> &CaptureCompletion {
        &self.completion
    }

    pub fn source_records(&self) -> &[SourceRecordManifest] {
        &self.source_records
    }

    pub const fn raw_source_disposition(&self) -> RawSourceDisposition {
        self.raw_source_disposition
    }

    pub fn observation_ids_in_source_order(&self) -> &[ObservationId] {
        &self.observation_ids_in_source_order
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("capture manifest encoding failed: {error}"))?;
        if bytes.len() > MAX_CAPTURE_MANIFEST_BYTES {
            return Err("capture manifest exceeds resource limit".into());
        }
        Ok(bytes)
    }

    fn terminal(&self) -> bool {
        self.completion.status != CaptureTerminalStatus::Ok
            || self.completion.partial
            || self.observation_ids_in_source_order.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawCaptureRecord {
    reference: ArtifactReference,
    bytes: Vec<u8>,
}

impl RawCaptureRecord {
    pub const fn reference(&self) -> &ArtifactReference {
        &self.reference
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedObservationBatch {
    observations: Vec<ReceivedObservation>,
    manifest: CaptureManifest,
    raw_records: Vec<RawCaptureRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchError {
    Limit,
    CompletionMismatch,
    DuplicateObservation(ObservationId),
    SourceRecordsLimit,
    SourceRecordLimit,
    SourceBytesLimit,
    SourceReferenceMismatch,
    MissingRawReference,
    InconsistentRetention,
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit => f.write_str("native observation batch exceeds resource limit"),
            Self::CompletionMismatch => {
                f.write_str("native capture completion count differs from normalized observations")
            }
            Self::DuplicateObservation(_) => {
                f.write_str("native observation batch repeats an observation identity")
            }
            Self::SourceRecordsLimit => f.write_str("native capture has too many source records"),
            Self::SourceRecordLimit => f.write_str("native source record exceeds resource limit"),
            Self::SourceBytesLimit => f.write_str("native source bytes exceed resource limit"),
            Self::SourceReferenceMismatch => {
                f.write_str("native source reference hash or length does not match bytes")
            }
            Self::MissingRawReference => {
                f.write_str("native envelope raw-source reference has no source record")
            }
            Self::InconsistentRetention => {
                f.write_str("native observations disagree on raw retention policy")
            }
        }
    }
}

impl std::error::Error for BatchError {}

impl ReceivedObservationBatch {
    /// Unwrap a complete adapter result at the outer composition boundary.
    /// Every source reference is checked against its exact bytes before bytes
    /// are either retained for explicit raw publication or discarded.
    pub fn from_normalized_capture(capture: NormalizedCapture) -> Result<Self, BatchError> {
        if capture.observations.len() > MAX_BATCH_OBSERVATIONS {
            return Err(BatchError::Limit);
        }
        if capture.source_records.len() > MAX_SOURCE_RECORDS {
            return Err(BatchError::SourceRecordsLimit);
        }
        if usize::from(capture.completion.observation_count) != capture.observations.len() {
            return Err(BatchError::CompletionMismatch);
        }
        let mut source_hashes = BTreeSet::new();
        let mut source_records = Vec::with_capacity(capture.source_records.len());
        let mut raw_records = Vec::new();
        let mut source_bytes = 0_u64;
        for source in capture.source_records {
            if source.bytes.len() > MAX_SOURCE_RECORD_BYTES {
                return Err(BatchError::SourceRecordLimit);
            }
            source_bytes = source_bytes
                .checked_add(source.bytes.len() as u64)
                .ok_or(BatchError::SourceBytesLimit)?;
            if source_bytes > MAX_SOURCE_BYTES
                || source.reference.byte_length != source.bytes.len() as u64
                || source.reference.sha256
                    != ContentHash::from_sha256(Sha256::digest(&source.bytes).into())
                || !source_hashes.insert(source.reference.sha256)
            {
                return Err(BatchError::SourceReferenceMismatch);
            }
            source_records.push(SourceRecordManifest {
                reference: source.reference.clone(),
            });
            raw_records.push(RawCaptureRecord {
                reference: source.reference,
                bytes: source.bytes,
            });
        }

        let retention = capture
            .observations
            .iter()
            .map(|observation| {
                matches!(
                    observation.envelope().data().privacy.payload,
                    PayloadRetention::Retained { .. }
                )
            })
            .collect::<BTreeSet<_>>();
        if retention.len() > 1 {
            return Err(BatchError::InconsistentRetention);
        }
        let raw_source_disposition = if retention.contains(&true) {
            RawSourceDisposition::Retained
        } else {
            RawSourceDisposition::NotRetained
        };
        let source_reference_set = source_records
            .iter()
            .map(|record| record.reference.sha256)
            .collect::<BTreeSet<_>>();
        let mut observation_ids = BTreeSet::new();
        for observation in &capture.observations {
            let data = observation.envelope().data();
            if !observation_ids.insert(data.id) {
                return Err(BatchError::DuplicateObservation(data.id));
            }
            if let Evidence::Known(reference) = &data.raw_source
                && !source_reference_set.contains(&reference.sha256)
            {
                return Err(BatchError::MissingRawReference);
            }
        }

        let raw_records = if raw_source_disposition == RawSourceDisposition::Retained {
            raw_records
        } else {
            Vec::new()
        };
        let manifest = CaptureManifest {
            schema_version: capture.schema_version,
            method_version: Text::new(PIPELINE_METHOD_VERSION)
                .map_err(|_| BatchError::SourceReferenceMismatch)?,
            evidence_origin: capture.evidence_origin,
            collector_build: capture.collector_build,
            capabilities: capture.capabilities,
            completion: capture.completion.into(),
            source_records,
            raw_source_disposition,
            observation_ids_in_source_order: capture
                .observations
                .iter()
                .map(|observation| observation.envelope().data().id)
                .collect(),
        };
        Ok(Self {
            observations: capture.observations,
            manifest,
            raw_records,
        })
    }

    pub fn observations(&self) -> &[ReceivedObservation] {
        &self.observations
    }

    pub const fn manifest(&self) -> &CaptureManifest {
        &self.manifest
    }
}

pub trait Cancellation {
    fn is_cancelled(&self) -> bool;
}

impl<F> Cancellation for F
where
    F: Fn() -> bool,
{
    fn is_cancelled(&self) -> bool {
        self()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortErrorKind {
    ReadOnly,
    Cancelled,
    Invalid,
    Corrupt,
    Unsupported,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureManifestReceipt {
    hash: ContentHash,
    observation_count: u64,
    raw_record_count: u64,
    project_revision: u64,
}

impl CaptureManifestReceipt {
    pub const fn hash(&self) -> ContentHash {
        self.hash
    }

    pub const fn observation_count(&self) -> u64 {
        self.observation_count
    }

    pub const fn raw_record_count(&self) -> u64 {
        self.raw_record_count
    }

    pub const fn project_revision(&self) -> u64 {
        self.project_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationChunkReceipt {
    hash: ContentHash,
    row_count: u64,
    project_revision: u64,
}

impl ObservationChunkReceipt {
    fn new(hash: String, row_count: u64, project_revision: u64) -> Result<Self, Box<PortError>> {
        if row_count == 0 {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "durable chunk receipt has no rows",
            )));
        }
        let hash = ContentHash::try_from(hash).map_err(|_| {
            Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "durable chunk receipt has an invalid content hash",
            ))
        })?;
        Ok(Self {
            hash,
            row_count,
            project_revision,
        })
    }

    pub const fn hash(&self) -> ContentHash {
        self.hash
    }

    pub const fn row_count(&self) -> u64 {
        self.row_count
    }

    pub const fn project_revision(&self) -> u64 {
        self.project_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotReceipt {
    snapshot_id: SnapshotId,
    project_revision: u64,
}

impl SnapshotReceipt {
    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    pub const fn project_revision(&self) -> u64 {
        self.project_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationProgress {
    manifest: CaptureManifestReceipt,
    raw_record_count: u64,
    chunk: Option<ObservationChunkReceipt>,
    snapshot: Option<SnapshotReceipt>,
}

impl PublicationProgress {
    pub const fn manifest(&self) -> &CaptureManifestReceipt {
        &self.manifest
    }

    pub const fn raw_record_count(&self) -> u64 {
        self.raw_record_count
    }

    pub const fn chunk(&self) -> Option<&ObservationChunkReceipt> {
        self.chunk.as_ref()
    }

    pub const fn snapshot(&self) -> Option<&SnapshotReceipt> {
        self.snapshot.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortError {
    kind: PortErrorKind,
    detail: String,
    progress: Option<Box<PublicationProgress>>,
}

impl PortError {
    pub fn new(kind: PortErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            progress: None,
        }
    }

    pub fn with_progress(mut self, progress: PublicationProgress) -> Self {
        self.progress = Some(Box::new(progress));
        self
    }

    pub const fn kind(&self) -> PortErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn progress(&self) -> Option<&PublicationProgress> {
        self.progress.as_deref()
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "observation persistence {:?}: {}",
            self.kind, self.detail
        )
    }
}

impl std::error::Error for PortError {}

pub trait CapturePersistencePort {
    fn persist_capture(
        &mut self,
        request: CapturePersistenceRequest<'_>,
    ) -> Result<PublicationProgress, PortError>;
}

pub struct CapturePersistenceRequest<'a> {
    pub manifest: &'a CaptureManifest,
    pub raw_records: &'a [RawCaptureRecord],
    pub observations: &'a [ObservationEnvelope],
    pub snapshot_id: SnapshotId,
    pub survey: &'a PointSurvey,
    pub provenance_id: &'a Text,
    pub published_utc_ms: i64,
    pub cancel: &'a dyn Cancellation,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineOutcome {
    pub schema_version: PipelineSchemaVersion,
    pub survey: PointSurvey,
    pub publication: PublicationProgress,
    pub association_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PipelineError {
    Batch(BatchError),
    Cancelled,
    Association {
        index: usize,
        error: SurveyError,
    },
    Storage(PortError),
    Partial {
        progress: Box<PublicationProgress>,
        error: PortError,
    },
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Batch(error) => error.fmt(f),
            Self::Cancelled => f.write_str("native observation pipeline cancelled"),
            Self::Association { index, error } => {
                write!(f, "observation association {index} failed: {error}")
            }
            Self::Storage(error) => error.fmt(f),
            Self::Partial { error, .. } => {
                write!(f, "capture publication is partial: {error}")
            }
        }
    }
}

impl std::error::Error for PipelineError {}

pub fn ingest<P, C>(
    port: &mut P,
    survey: &PointSurvey,
    batch: &ReceivedObservationBatch,
    request: &PipelineRequest,
    cancel: &C,
) -> Result<PipelineOutcome, PipelineError>
where
    P: CapturePersistencePort,
    C: Cancellation,
{
    if request.schema_version != PipelineSchemaVersion::V1 {
        return Err(PipelineError::Storage(PortError::new(
            PortErrorKind::Unsupported,
            "unsupported pipeline schema version",
        )));
    }
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let mut next = survey.clone();
    let mut envelopes = Vec::with_capacity(batch.observations.len());
    for (index, received) in batch.observations.iter().enumerate() {
        let (associated, _) = next
            .associate_received(received)
            .map_err(|error| PipelineError::Association { index, error })?;
        next = associated;
        envelopes.push(received.envelope().clone());
    }
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }

    let publication = port
        .persist_capture(CapturePersistenceRequest {
            manifest: &batch.manifest,
            raw_records: &batch.raw_records,
            observations: &envelopes,
            snapshot_id: request.snapshot_id,
            survey: &next,
            provenance_id: &request.provenance_id,
            published_utc_ms: request.published_utc_ms,
            cancel,
        })
        .map_err(|error| {
            if error.kind() == PortErrorKind::Cancelled && error.progress().is_none() {
                PipelineError::Cancelled
            } else if let Some(progress) = error.progress().cloned() {
                PipelineError::Partial {
                    progress: Box::new(progress),
                    error,
                }
            } else {
                PipelineError::Storage(error)
            }
        })?;
    let expected_manifest_hash = ContentHash::from_sha256(
        Sha256::digest(&batch.manifest.canonical_bytes().map_err(|error| {
            PipelineError::Storage(PortError::new(PortErrorKind::Invalid, error))
        })?)
        .into(),
    );
    let expected_raw_count = batch.raw_records.len() as u64;
    let chunk_valid = match publication.chunk() {
        Some(chunk) => {
            !envelopes.is_empty()
                && chunk.row_count() == envelopes.len() as u64
                && chunk.project_revision() >= publication.manifest.project_revision()
                && chunk.project_revision() > 0
        }
        None => envelopes.is_empty(),
    };
    let snapshot_valid = publication.snapshot().is_some_and(|snapshot| {
        snapshot.snapshot_id() == request.snapshot_id
            && snapshot.project_revision() >= publication.manifest.project_revision()
            && publication
                .chunk()
                .is_none_or(|chunk| snapshot.project_revision() >= chunk.project_revision())
            && snapshot.project_revision() > 0
    });
    let progress_valid = publication.manifest.hash() == expected_manifest_hash
        && publication.manifest.observation_count() == envelopes.len() as u64
        && publication.manifest.raw_record_count() == expected_raw_count
        && publication.manifest.project_revision() > 0
        && publication.raw_record_count() == expected_raw_count
        && chunk_valid
        && snapshot_valid;
    if !progress_valid {
        return Err(PipelineError::Storage(PortError::new(
            PortErrorKind::Corrupt,
            "capture persistence returned inconsistent receipts",
        )));
    }

    Ok(PipelineOutcome {
        schema_version: PipelineSchemaVersion::V1,
        survey: next,
        publication,
        association_count: batch.observations.len(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRequest {
    schema_version: PipelineSchemaVersion,
    snapshot_id: SnapshotId,
    provenance_id: Text,
    published_utc_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    NegativePublicationTime,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativePublicationTime => f.write_str("publication time must be UTC"),
        }
    }
}

impl std::error::Error for RequestError {}

impl PipelineRequest {
    pub fn new(
        snapshot_id: SnapshotId,
        provenance_id: Text,
        published_utc_ms: i64,
    ) -> Result<Self, RequestError> {
        if published_utc_ms < 0 {
            return Err(RequestError::NegativePublicationTime);
        }
        Ok(Self {
            schema_version: PipelineSchemaVersion::V1,
            snapshot_id,
            provenance_id,
            published_utc_ms,
        })
    }

    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }
}

#[cfg(test)]
mod tests;
