use super::*;
use crate::spool::{AcquisitionSpoolPort, AcquisitionSpoolRequest};

#[test]
fn spool_reopens_exact_evidence_without_survey_and_retries_idempotently() {
    let batch = batch();
    let directory = retained_tempdir();
    let path = directory.path().join("spool");
    let mut bundle = project(&path);
    let provenance = text("acquisition-spool/test-v1");
    let publish = |bundle: &mut Bundle| {
        bundle
            .persist_acquisition(
                AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap(),
            )
            .unwrap()
    };
    let first = publish(&mut bundle);
    let second = publish(&mut bundle);
    assert_eq!(first, second);
    assert!(first.snapshot().is_none());
    let hash = String::from(first.manifest().hash());
    let record = bundle.capture_publication(&hash).unwrap().unwrap();
    assert_eq!(record.status, CapturePublicationStatus::Chunk);
    assert!(record.snapshot_id.is_none());
    assert_eq!(first.raw_record_count(), 0);
    for source in batch.manifest().source_records() {
        assert!(
            bundle
                .read_artifact(&String::from(source.reference().sha256))
                .is_err()
        );
    }
    drop(bundle);
    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(
        bundle.read_observations().unwrap(),
        batch
            .observations()
            .iter()
            .map(|o| o.envelope().clone())
            .collect::<Vec<_>>()
    );
    assert!(bundle.verify().unwrap().failures.is_empty());
}

#[test]
fn empty_spool_preserves_native_terminal_and_has_no_chunk_or_snapshot() {
    for fixture in [
        include_bytes!("../../../collectors/macos/fixtures/empty.ndjson").as_slice(),
        include_bytes!("../../../collectors/macos/fixtures/error.ndjson").as_slice(),
        include_bytes!("../../../collectors/macos/fixtures/denied.ndjson").as_slice(),
    ] {
        let batch =
            ReceivedObservationBatch::from_normalized_capture(normalized_capture(fixture, true))
                .unwrap();
        let directory = retained_tempdir();
        let mut bundle = project(&directory.path().join("spool"));
        let receipt = bundle
            .persist_acquisition(
                AcquisitionSpoolRequest::new(&batch, &text("empty-spool/v1"), 2, &NeverCancel)
                    .unwrap(),
            )
            .unwrap();
        assert!(receipt.chunk().is_none());
        assert!(receipt.snapshot().is_none());
        let hash = String::from(receipt.manifest().hash());
        let record = bundle.capture_publication(&hash).unwrap().unwrap();
        assert_eq!(record.status, CapturePublicationStatus::Terminal);
        assert!(record.snapshot_id.is_none());
        let persisted: CaptureManifest =
            serde_json::from_slice(&bundle.read_artifact(&hash).unwrap()).unwrap();
        assert_eq!(&persisted, batch.manifest());
    }
}

#[test]
fn spool_cancellation_reports_durable_manifest_and_retry_finishes() {
    let batch = batch();
    let directory = retained_tempdir();
    let mut bundle = project(&directory.path().join("spool"));
    let calls = Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() >= 2
    };
    let error = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &text("spool-cancel/v1"), 2, &cancel).unwrap(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PortErrorKind::Cancelled);
    let partial = error.progress().unwrap();
    assert!(partial.chunk().is_none());
    assert!(partial.snapshot().is_none());
    let hash = partial.manifest().hash();
    let receipt = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &text("spool-cancel/v1"), 2, &NeverCancel)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(receipt.manifest().hash(), hash);
    assert!(receipt.chunk().is_some());
    assert!(receipt.snapshot().is_none());
}

#[test]
fn spool_rejects_negative_time_and_read_only_publication() {
    let batch = batch();
    let provenance = text("spool/test");
    assert!(AcquisitionSpoolRequest::new(&batch, &provenance, -1, &NeverCancel).is_err());
    let directory = retained_tempdir();
    let path = directory.path().join("spool");
    drop(project(&path));
    let mut bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let error = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PortErrorKind::ReadOnly);
    assert!(error.progress().is_none());
}

#[test]
fn retained_spool_verifies_raw_closure_without_a_survey() {
    let batch = retained_batch();
    let directory = retained_tempdir();
    let mut bundle = project(&directory.path().join("spool"));
    let receipt = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &text("retained-spool/v1"), 2, &NeverCancel)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(receipt.raw_record_count(), batch.raw_records.len() as u64);
    assert!(receipt.snapshot().is_none());
    for record in &batch.raw_records {
        assert_eq!(
            bundle
                .read_artifact(&String::from(record.reference().sha256))
                .unwrap(),
            record.bytes()
        );
    }
    assert!(bundle.verify().unwrap().failures.is_empty());
}
