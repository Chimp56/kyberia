use kyberia_domain::identity::{ContentHash, SnapshotId};
use kyberia_observation_pipeline::{
    CaptureManifestReceipt, CapturePersistencePort, CapturePersistenceRequest,
    ObservationChunkReceipt, PortError, PublicationProgress, SnapshotReceipt,
};

struct ExternalPort;

impl ExternalPort {
    fn valid_publication() -> PublicationProgress {
        let manifest =
            CaptureManifestReceipt::new(ContentHash::from_sha256([7; 32]), 1, 0, 1).unwrap();
        let chunk = ObservationChunkReceipt::new(ContentHash::from_sha256([8; 32]), 1, 2).unwrap();
        let snapshot = SnapshotReceipt::new(SnapshotId::from_bytes([9; 16]).unwrap(), 3).unwrap();
        PublicationProgress::new(manifest, 0, Some(chunk), Some(snapshot)).unwrap()
    }
}

impl CapturePersistencePort for ExternalPort {
    fn persist_capture(
        &mut self,
        request: CapturePersistenceRequest<'_>,
    ) -> Result<PublicationProgress, PortError> {
        // An external adapter can inspect the validated, association-bound
        // request and return receipts through the public validating builders.
        assert!(request.published_utc_ms() >= 0);
        Ok(Self::valid_publication())
    }
}

#[test]
fn external_persistence_port_can_build_validated_receipts() {
    let mut port = ExternalPort;
    let publication = ExternalPort::valid_publication();
    assert_eq!(publication.manifest().observation_count(), 1);
    assert_eq!(publication.chunk().unwrap().row_count(), 1);
    assert_eq!(publication.snapshot().unwrap().project_revision(), 3);

    // Keep the implementation type checked as an external crate even though
    // constructing a request is intentionally reserved for `ingest`.
    let _port: &mut dyn CapturePersistencePort = &mut port;
}

#[test]
fn external_receipt_builders_reject_unbounded_or_incomplete_progress() {
    assert!(CaptureManifestReceipt::new(ContentHash::from_sha256([1; 32]), 4_097, 0, 1).is_err());
    let manifest = CaptureManifestReceipt::new(ContentHash::from_sha256([2; 32]), 1, 0, 1).unwrap();
    let chunk = ObservationChunkReceipt::new(ContentHash::from_sha256([3; 32]), 1, 2).unwrap();
    assert!(PublicationProgress::new(manifest, 1, Some(chunk), None).is_err());
}
