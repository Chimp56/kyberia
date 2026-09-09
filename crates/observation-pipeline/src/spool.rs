//! Durable publication of a validated capture batch without survey association.
//!
//! This bounded batch operation preserves native completion and unknown fields.
//! It does not journal an in-flight collector or mark a survey point complete.

use crate::{
    Cancellation, PortError, PortErrorKind, PublicationProgress, ReceivedObservationBatch,
};
use kyberia_domain::identity::Text;
use kyberia_project_store::Bundle;

/// Opaque request constructed only from an already validated immutable batch.
pub struct AcquisitionSpoolRequest<'a> {
    batch: &'a ReceivedObservationBatch,
    provenance: &'a Text,
    published_utc_ms: i64,
    cancel: &'a dyn Cancellation,
}

impl<'a> AcquisitionSpoolRequest<'a> {
    pub fn new(
        batch: &'a ReceivedObservationBatch,
        provenance: &'a Text,
        published_utc_ms: i64,
        cancel: &'a dyn Cancellation,
    ) -> Result<Self, PortError> {
        if published_utc_ms < 0 {
            return Err(PortError::new(
                PortErrorKind::Invalid,
                "publication time must be UTC",
            ));
        }
        Ok(Self {
            batch,
            provenance,
            published_utc_ms,
            cancel,
        })
    }

    pub fn batch(&self) -> &ReceivedObservationBatch {
        self.batch
    }
    /// Only records admitted under the explicit retained-payload policy.
    pub fn raw_records(&self) -> &[crate::RawCaptureRecord] {
        &self.batch.raw_records
    }

    pub fn provenance(&self) -> &Text {
        self.provenance
    }
    pub const fn published_utc_ms(&self) -> i64 {
        self.published_utc_ms
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// Durable publication plus the exact native terminal evidence. A chunk does
/// not imply successful or complete capture; partial/error captures can carry rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcquisitionSpoolOutcome {
    publication: PublicationProgress,
    completion: crate::CaptureCompletion,
}

impl AcquisitionSpoolOutcome {
    /// Bind a replaceable port's receipt to this exact validated request.
    pub fn new(
        request: &AcquisitionSpoolRequest<'_>,
        publication: PublicationProgress,
    ) -> Result<Self, PortError> {
        use sha2::{Digest, Sha256};
        let bytes = request
            .batch
            .manifest
            .canonical_bytes()
            .map_err(|error| PortError::new(PortErrorKind::Invalid, error))?;
        let hash = kyberia_domain::identity::ContentHash::from_sha256(Sha256::digest(bytes).into());
        let count = request.batch.observations.len() as u64;
        if publication.snapshot().is_some()
            || publication.manifest().hash() != hash
            || publication.manifest().observation_count() != count
            || publication.manifest().raw_record_count() != request.batch.raw_records.len() as u64
            || publication.raw_record_count() != request.batch.raw_records.len() as u64
            || match publication.chunk() {
                Some(chunk) => chunk.row_count() != count || count == 0,
                None => count != 0,
            }
        {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "acquisition receipt does not match the validated batch",
            ));
        }
        Ok(Self {
            publication,
            completion: request.batch.manifest.completion().clone(),
        })
    }

    pub const fn publication(&self) -> &PublicationProgress {
        &self.publication
    }
    pub const fn completion(&self) -> &crate::CaptureCompletion {
        &self.completion
    }
}

/// Publishing evidence does not require or manufacture a point-survey snapshot.
/// Success returns durable manifest/raw/chunk receipts; snapshot is absent.
/// A publication failure preserves any completed progress for explicit retry.
pub trait AcquisitionSpoolPort {
    fn persist_acquisition(
        &mut self,
        request: AcquisitionSpoolRequest<'_>,
    ) -> Result<AcquisitionSpoolOutcome, PortError>;
}

impl AcquisitionSpoolPort for Bundle {
    fn persist_acquisition(
        &mut self,
        request: AcquisitionSpoolRequest<'_>,
    ) -> Result<AcquisitionSpoolOutcome, PortError> {
        if request.is_cancelled() {
            return Err(PortError::new(
                PortErrorKind::Cancelled,
                "cancelled before acquisition batch materialization",
            ));
        }
        let observations = request
            .batch
            .observations
            .iter()
            .map(|observation| observation.envelope().clone())
            .collect::<Vec<_>>();
        let publication = crate::bundle::persist_evidence(
            self,
            &request.batch.manifest,
            &request.batch.raw_records,
            &observations,
            request.provenance,
            request.published_utc_ms,
            request.cancel,
        )?;
        AcquisitionSpoolOutcome::new(&request, publication)
    }
}
