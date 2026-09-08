//! Versioned composition for native normalized observations.
//!
//! The inward contract consumes canonical ReceivedObservation values and a
//! pure PointSurvey. The native adapter's NormalizedCapture is unwrapped only
//! at this composition edge, and the persistence port exposes neither SQLite
//! nor Parquet types to the survey or domain crates.

use kyberia_capture_adapter::macos::{Completion as AdapterCompletion, NormalizedCapture};
use kyberia_domain::{
    evidence::{ArtifactReference, Evidence},
    identity::{ContentHash, ObservationId, SnapshotId, Text},
    observation::{ObservationEnvelope, PayloadRetention, ReceivedObservation},
};
use kyberia_survey::{PointSurvey, SurveyError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt};

mod bundle;

pub const MAX_BATCH_OBSERVATIONS: usize = kyberia_domain::capture::MAX_CAPTURE_OBSERVATIONS;
pub const MAX_SOURCE_RECORDS: usize = kyberia_domain::capture::MAX_CAPTURE_SOURCE_RECORDS;
pub const MAX_SOURCE_RECORD_BYTES: usize =
    kyberia_domain::capture::MAX_CAPTURE_SOURCE_RECORD_BYTES as usize;
pub const MAX_SOURCE_BYTES: u64 = (MAX_SOURCE_RECORDS * MAX_SOURCE_RECORD_BYTES) as u64;
pub const MAX_CAPTURE_MANIFEST_BYTES: usize = kyberia_domain::capture::MAX_CAPTURE_MANIFEST_BYTES;
pub const PIPELINE_METHOD_VERSION: &str = kyberia_domain::capture::CAPTURE_MANIFEST_METHOD_VERSION;

pub use kyberia_domain::capture::{
    CaptureCompletion, CaptureManifest, CaptureTerminalStatus, RawSourceDisposition,
    SourceRecordManifest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

fn capture_completion(completion: AdapterCompletion) -> CaptureCompletion {
    completion.into()
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
            source_records.push(SourceRecordManifest::new(source.reference.clone()));
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
        let mut observation_ids = BTreeSet::new();
        for observation in &capture.observations {
            let data = observation.envelope().data();
            if !observation_ids.insert(data.id) {
                return Err(BatchError::DuplicateObservation(data.id));
            }
            if let Evidence::Known(reference) = &data.raw_source {
                if !source_records
                    .iter()
                    .any(|record| record.reference().sha256 == reference.sha256)
                {
                    return Err(BatchError::MissingRawReference);
                }
                if !source_records
                    .iter()
                    .any(|record| record.reference() == reference)
                {
                    return Err(BatchError::SourceReferenceMismatch);
                }
            }
        }

        let raw_records = if raw_source_disposition == RawSourceDisposition::Retained {
            raw_records
        } else {
            Vec::new()
        };
        let manifest = CaptureManifest::new(
            capture.schema_version,
            Text::new(PIPELINE_METHOD_VERSION).map_err(|_| BatchError::SourceReferenceMismatch)?,
            capture.evidence_origin,
            capture.collector_build,
            capture.capabilities,
            capture_completion(capture.completion),
            source_records,
            raw_source_disposition,
            capture
                .observations
                .iter()
                .map(|observation| observation.envelope().data().id)
                .collect(),
        )
        .map_err(|_| BatchError::SourceReferenceMismatch)?;
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
    pub fn new(
        hash: ContentHash,
        observation_count: u64,
        raw_record_count: u64,
        project_revision: u64,
    ) -> Result<Self, Box<PortError>> {
        if observation_count > MAX_BATCH_OBSERVATIONS as u64 {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "capture manifest receipt exceeds observation limit",
            )));
        }
        if raw_record_count > MAX_SOURCE_RECORDS as u64 {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "capture manifest receipt exceeds source-record limit",
            )));
        }
        if project_revision == 0 {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "capture manifest receipt has no project revision",
            )));
        }
        Ok(Self {
            hash,
            observation_count,
            raw_record_count,
            project_revision,
        })
    }

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
    pub fn new(
        hash: ContentHash,
        row_count: u64,
        project_revision: u64,
    ) -> Result<Self, Box<PortError>> {
        if row_count == 0 {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "durable chunk receipt has no rows",
            )));
        }
        if project_revision == 0 {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "durable chunk receipt has no project revision",
            )));
        }
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
    pub fn new(snapshot_id: SnapshotId, project_revision: u64) -> Result<Self, Box<PortError>> {
        if project_revision == 0 {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "survey snapshot receipt has no project revision",
            )));
        }
        Ok(Self {
            snapshot_id,
            project_revision,
        })
    }

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
    pub fn new(
        manifest: CaptureManifestReceipt,
        raw_record_count: u64,
        chunk: Option<ObservationChunkReceipt>,
        snapshot: Option<SnapshotReceipt>,
    ) -> Result<Self, Box<PortError>> {
        if raw_record_count > manifest.raw_record_count {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "publication progress exceeds manifest raw-record count",
            )));
        }
        if let Some(chunk) = &chunk
            && (manifest.observation_count == 0
                || chunk.row_count > manifest.observation_count
                || chunk.project_revision < manifest.project_revision)
        {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "publication progress has an invalid observation chunk receipt",
            )));
        }
        if let Some(snapshot) = &snapshot
            && (snapshot.project_revision < manifest.project_revision
                || chunk
                    .as_ref()
                    .is_some_and(|chunk| snapshot.project_revision < chunk.project_revision))
        {
            return Err(Box::new(PortError::new(
                PortErrorKind::Corrupt,
                "publication progress has an invalid snapshot receipt",
            )));
        }
        Ok(Self {
            manifest,
            raw_record_count,
            chunk,
            snapshot,
        })
    }

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
    manifest: &'a CaptureManifest,
    raw_records: &'a [RawCaptureRecord],
    observations: &'a [ObservationEnvelope],
    snapshot_id: SnapshotId,
    survey: &'a PointSurvey,
    provenance_id: &'a Text,
    published_utc_ms: i64,
    cancel: &'a dyn Cancellation,
}

impl<'a> CapturePersistenceRequest<'a> {
    /// Construct a request only after checking the cross-object invariants
    /// that must hold before an adapter can publish anything. This constructor
    /// is crate-private: only [`ingest`] can bind a survey after associating
    /// the exact batch, while replaceable persistence ports receive an opaque
    /// validated request through the public trait.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        manifest: &'a CaptureManifest,
        raw_records: &'a [RawCaptureRecord],
        observations: &'a [ObservationEnvelope],
        snapshot_id: SnapshotId,
        survey: &'a PointSurvey,
        provenance_id: &'a Text,
        published_utc_ms: i64,
        cancel: &'a dyn Cancellation,
    ) -> Result<Self, PortError> {
        let request = Self {
            manifest,
            raw_records,
            observations,
            snapshot_id,
            survey,
            provenance_id,
            published_utc_ms,
            cancel,
        };
        request.validate()?;
        Ok(request)
    }

    /// Revalidate at the adapter boundary as defense in depth for
    /// crate-internal construction and future code changes. This method is
    /// deliberately pure: it does no I/O and cannot mutate a bundle.
    pub fn validate(&self) -> Result<(), PortError> {
        if self.observations.len() > MAX_BATCH_OBSERVATIONS {
            return Err(PortError::new(
                PortErrorKind::Invalid,
                "capture persistence observation limit exceeded",
            ));
        }
        if self.manifest.source_records().len() > MAX_SOURCE_RECORDS {
            return Err(PortError::new(
                PortErrorKind::Invalid,
                "capture persistence source-record limit exceeded",
            ));
        }
        if self.published_utc_ms < 0 {
            return Err(PortError::new(
                PortErrorKind::Invalid,
                "capture persistence time must be UTC",
            ));
        }
        if usize::from(self.manifest.completion().observation_count()) != self.observations.len()
            || self.manifest.observation_ids_in_source_order().len() != self.observations.len()
        {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "capture manifest observation count differs from request",
            ));
        }

        let mut observation_ids = BTreeSet::new();
        for (index, observation) in self.observations.iter().enumerate() {
            let id = observation.data().id;
            if !observation_ids.insert(id)
                || self.manifest.observation_ids_in_source_order()[index] != id
            {
                return Err(PortError::new(
                    PortErrorKind::Corrupt,
                    "capture manifest observation identity/order differs from request",
                ));
            }
            let data = observation.data();
            if !self.survey.associations().iter().any(|association| {
                association.observation_id() == id
                    && association.session_id() == data.session_id
                    && association.source_id() == data.source.source_id
            }) {
                return Err(PortError::new(
                    PortErrorKind::Corrupt,
                    "capture survey has no association for a published observation",
                ));
            }
        }

        let mut source_hashes = BTreeSet::new();
        for source in self.manifest.source_records() {
            if source.reference().byte_length > MAX_SOURCE_RECORD_BYTES as u64
                || !source_hashes.insert(source.reference().sha256)
            {
                return Err(PortError::new(
                    PortErrorKind::Corrupt,
                    "capture manifest has an invalid or duplicate source reference",
                ));
            }
        }
        for observation in self.observations {
            if let Evidence::Known(reference) = &observation.data().raw_source
                && !self
                    .manifest
                    .source_records()
                    .iter()
                    .any(|source| source.reference() == reference)
            {
                return Err(PortError::new(
                    PortErrorKind::Corrupt,
                    "observation raw-source metadata has no manifest source record",
                ));
            }
        }

        let expected_retained = matches!(
            self.manifest.raw_source_disposition(),
            RawSourceDisposition::Retained
        );
        let observation_retention = self
            .observations
            .iter()
            .map(|observation| {
                matches!(
                    observation.data().privacy.payload,
                    PayloadRetention::Retained { .. }
                )
            })
            .collect::<BTreeSet<_>>();
        if observation_retention.len() > 1
            || (!observation_retention.is_empty()
                && observation_retention.first().copied() != Some(expected_retained))
        {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "capture manifest retention disposition differs from observations",
            ));
        }
        if self.raw_records.len()
            != if expected_retained {
                self.manifest.source_records().len()
            } else {
                0
            }
            || self
                .raw_records
                .iter()
                .any(|record| record.bytes().len() > MAX_SOURCE_RECORD_BYTES)
            || self.raw_records.iter().any(|record| {
                record.reference().byte_length != record.bytes().len() as u64
                    || record.reference().sha256
                        != ContentHash::from_sha256(Sha256::digest(record.bytes()).into())
                    || !self
                        .manifest
                        .source_records()
                        .iter()
                        .any(|source| source.reference() == record.reference())
            })
        {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "raw capture records do not match the manifest source closure",
            ));
        }
        if expected_retained
            && self.manifest.source_records().iter().any(|source| {
                !self
                    .raw_records
                    .iter()
                    .any(|record| record.reference() == source.reference())
            })
        {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "manifest source closure is missing a retained record",
            ));
        }
        Ok(())
    }

    pub const fn manifest(&self) -> &'a CaptureManifest {
        self.manifest
    }

    pub const fn raw_records(&self) -> &'a [RawCaptureRecord] {
        self.raw_records
    }

    pub const fn observations(&self) -> &'a [ObservationEnvelope] {
        self.observations
    }

    pub const fn snapshot_id(&self) -> SnapshotId {
        self.snapshot_id
    }

    pub const fn survey(&self) -> &'a PointSurvey {
        self.survey
    }

    pub const fn provenance_id(&self) -> &'a Text {
        self.provenance_id
    }

    pub const fn published_utc_ms(&self) -> i64 {
        self.published_utc_ms
    }

    pub const fn cancel(&self) -> &'a dyn Cancellation {
        self.cancel
    }
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

    let persistence_request = CapturePersistenceRequest::new(
        &batch.manifest,
        &batch.raw_records,
        &envelopes,
        request.snapshot_id,
        &next,
        &request.provenance_id,
        request.published_utc_ms,
        cancel,
    )
    .map_err(PipelineError::Storage)?;
    let publication = port.persist_capture(persistence_request).map_err(|error| {
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
