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

/// Publishing evidence does not require or manufacture a point-survey snapshot.
/// Success returns durable manifest/raw/chunk receipts; snapshot is absent.
/// A publication failure preserves any completed progress for explicit retry.
pub trait AcquisitionSpoolPort {
    fn persist_acquisition(
        &mut self,
        request: AcquisitionSpoolRequest<'_>,
    ) -> Result<PublicationProgress, PortError>;
}

impl AcquisitionSpoolPort for Bundle {
    fn persist_acquisition(
        &mut self,
        request: AcquisitionSpoolRequest<'_>,
    ) -> Result<PublicationProgress, PortError> {
        let observations = request
            .batch
            .observations
            .iter()
            .map(|observation| observation.envelope().clone())
            .collect::<Vec<_>>();
        crate::bundle::persist_evidence(
            self,
            &request.batch.manifest,
            &request.batch.raw_records,
            &observations,
            request.provenance,
            request.published_utc_ms,
            request.cancel,
        )
    }
}
