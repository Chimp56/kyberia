use crate::{
    CaptureManifestReceipt, CapturePersistencePort, CapturePersistenceRequest,
    ObservationChunkReceipt, PortError, PortErrorKind, PublicationProgress, SnapshotReceipt,
};
use kyberia_project_store::{
    ArtifactEntry, ArtifactKind, Bundle, CaptureManifestRegistration, ObservationChunkProvenance,
    StoreError,
};
use sha2::{Digest, Sha256};

fn map_store_error(error: StoreError) -> PortError {
    let kind = match error {
        StoreError::ReadOnly => PortErrorKind::ReadOnly,
        StoreError::Cancelled => PortErrorKind::Cancelled,
        StoreError::Invalid(_) => PortErrorKind::Invalid,
        StoreError::Corrupt(_) => PortErrorKind::Corrupt,
        StoreError::UnsupportedVersion(_) | StoreError::UnsupportedChunkVersion(_) => {
            PortErrorKind::Unsupported
        }
        StoreError::Io(_) | StoreError::Sql(_) | StoreError::Json(_) => PortErrorKind::Io,
        StoreError::Operation(_) | StoreError::ChunkCodec(_) => PortErrorKind::Invalid,
    };
    PortError::new(kind, error.to_string())
}

fn manifest_receipt(
    hash: String,
    observation_count: u64,
    raw_record_count: u64,
    revision: u64,
) -> Result<CaptureManifestReceipt, Box<PortError>> {
    let hash = kyberia_domain::identity::ContentHash::try_from(hash).map_err(|_| {
        Box::new(PortError::new(
            PortErrorKind::Corrupt,
            "capture manifest persistence returned an invalid hash",
        ))
    })?;
    Ok(CaptureManifestReceipt {
        hash,
        observation_count,
        raw_record_count,
        project_revision: revision,
    })
}

fn progress(
    manifest: CaptureManifestReceipt,
    raw_record_count: u64,
    chunk: Option<ObservationChunkReceipt>,
    snapshot: Option<SnapshotReceipt>,
) -> PublicationProgress {
    PublicationProgress {
        manifest,
        raw_record_count,
        chunk,
        snapshot,
    }
}

impl CapturePersistencePort for Bundle {
    fn persist_capture(
        &mut self,
        request: CapturePersistenceRequest<'_>,
    ) -> Result<PublicationProgress, PortError> {
        request.validate()?;
        let CapturePersistenceRequest {
            manifest,
            raw_records,
            observations,
            snapshot_id,
            survey,
            provenance_id,
            published_utc_ms,
            cancel,
        } = request;
        if cancel.is_cancelled() {
            return Err(PortError::new(
                PortErrorKind::Cancelled,
                "cancelled before capture manifest publication",
            ));
        }
        if raw_records.len() as u64
            != match manifest.raw_source_disposition() {
                crate::RawSourceDisposition::Retained => manifest.source_records().len() as u64,
                crate::RawSourceDisposition::NotRetained => 0,
            }
        {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "raw capture records do not match capture manifest disposition",
            ));
        }
        let manifest_bytes = manifest
            .canonical_bytes()
            .map_err(|error| PortError::new(PortErrorKind::Invalid, error))?;
        let manifest_hash = kyberia_project_store::content_hash(&manifest_bytes);
        let stored = self
            .persist_capture_manifest(CaptureManifestRegistration {
                manifest_hash: &manifest_hash,
                manifest_bytes: &manifest_bytes,
                provenance_id: provenance_id.as_str(),
                utc_ms: published_utc_ms,
                observation_count: observations.len() as u64,
                raw_record_count: raw_records.len() as u64,
                terminal: manifest.terminal(),
            })
            .map_err(map_store_error)?;
        if stored.observation_count != observations.len() as u64
            || stored.raw_record_count != raw_records.len() as u64
            || stored.manifest_hash != manifest_hash
        {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "capture manifest persistence returned mismatched metadata",
            ));
        }
        let manifest_receipt = manifest_receipt(
            stored.manifest_hash,
            stored.observation_count,
            stored.raw_record_count,
            stored.revision,
        )
        .map_err(|error| *error)?;
        let mut published_raw = 0_u64;
        let mut current = progress(manifest_receipt.clone(), 0, None, None);
        for record in raw_records {
            if cancel.is_cancelled() {
                return Err(PortError::new(
                    PortErrorKind::Cancelled,
                    "cancelled during raw capture publication",
                )
                .with_progress(current));
            }
            if record.reference().byte_length != record.bytes().len() as u64
                || record.reference().sha256
                    != kyberia_domain::identity::ContentHash::from_sha256(
                        Sha256::digest(record.bytes()).into(),
                    )
            {
                return Err(PortError::new(
                    PortErrorKind::Corrupt,
                    "raw capture reference does not match bytes",
                )
                .with_progress(current));
            }
            let entry = ArtifactEntry {
                kind: ArtifactKind::RawCapture,
                bytes: record.bytes().len() as u64,
                media_type: record.reference().media_type.as_str().to_owned(),
                provenance_id: provenance_id.as_str().to_owned(),
            };
            self.put_artifact(record.bytes(), entry, published_utc_ms)
                .map_err(|error| map_store_error(error).with_progress(current.clone()))?;
            published_raw += 1;
            current.raw_record_count = published_raw;
        }
        if observations.is_empty() {
            let snapshot = self
                .save_survey_snapshot(snapshot_id, survey, published_utc_ms)
                .map_err(|error| map_store_error(error).with_progress(current.clone()))?;
            let snapshot_receipt = SnapshotReceipt {
                snapshot_id: snapshot.snapshot_id,
                project_revision: snapshot.revision,
            };
            self.link_capture_snapshot(&manifest_hash, snapshot.snapshot_id)
                .map_err(|error| {
                    map_store_error(error).with_progress({
                        current.snapshot = Some(snapshot_receipt.clone());
                        current.clone()
                    })
                })?;
            current.snapshot = Some(snapshot_receipt);
            return Ok(current);
        }
        if cancel.is_cancelled() {
            return Err(PortError::new(
                PortErrorKind::Cancelled,
                "cancelled before normalized chunk publication",
            )
            .with_progress(current));
        }
        let provenance = ObservationChunkProvenance::new(provenance_id.as_str().to_owned())
            .map_err(map_store_error)?;
        let descriptor = self
            .publish_observation_chunk_with_cancel(
                observations,
                provenance,
                published_utc_ms,
                || cancel.is_cancelled(),
            )
            .map_err(|error| map_store_error(error).with_progress(current.clone()))?;
        let chunk = ObservationChunkReceipt::new(
            descriptor.hash().to_owned(),
            descriptor.row_count(),
            descriptor.revision(),
        )
        .map_err(|error| *error)?;
        if chunk.row_count() != observations.len() as u64 {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "normalized chunk returned a mismatched row count",
            )
            .with_progress(progress(
                manifest_receipt.clone(),
                published_raw,
                Some(chunk),
                None,
            )));
        }
        self.link_capture_chunk(&manifest_hash, descriptor.hash(), descriptor.row_count())
            .map_err(|error| {
                map_store_error(error).with_progress(progress(
                    manifest_receipt.clone(),
                    published_raw,
                    Some(chunk.clone()),
                    None,
                ))
            })?;
        current.chunk = Some(chunk);
        if cancel.is_cancelled() {
            return Err(PortError::new(
                PortErrorKind::Cancelled,
                "cancelled before survey snapshot publication",
            )
            .with_progress(current));
        }
        let snapshot = self
            .save_survey_snapshot(snapshot_id, survey, published_utc_ms)
            .map_err(|error| map_store_error(error).with_progress(current.clone()))?;
        let snapshot_receipt = SnapshotReceipt {
            snapshot_id: snapshot.snapshot_id,
            project_revision: snapshot.revision,
        };
        if snapshot_receipt.snapshot_id != snapshot_id {
            return Err(PortError::new(
                PortErrorKind::Corrupt,
                "survey snapshot persistence returned a mismatched identity",
            )
            .with_progress(current));
        }
        self.link_capture_snapshot(&manifest_hash, snapshot.snapshot_id)
            .map_err(|error| {
                map_store_error(error).with_progress({
                    current.snapshot = Some(snapshot_receipt.clone());
                    current.clone()
                })
            })?;
        current.snapshot = Some(snapshot_receipt);
        Ok(current)
    }
}
