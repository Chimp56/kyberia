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
    assert_eq!(first.completion(), batch.manifest().completion());
    let second = publish(&mut bundle);
    assert_eq!(first, second);
    assert!(first.publication().snapshot().is_none());
    let hash = String::from(first.publication().manifest().hash());
    let record = bundle.capture_publication(&hash).unwrap().unwrap();
    assert_eq!(record.status, CapturePublicationStatus::Chunk);
    assert!(record.snapshot_id.is_none());
    assert_eq!(first.publication().raw_record_count(), 0);
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
        assert_eq!(receipt.completion(), batch.manifest().completion());
        assert!(receipt.publication().chunk().is_none());
        assert!(receipt.publication().snapshot().is_none());
        let hash = String::from(receipt.publication().manifest().hash());
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
        calls.get() >= 3
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
    assert_eq!(receipt.publication().manifest().hash(), hash);
    assert!(receipt.publication().chunk().is_some());
    assert!(receipt.publication().snapshot().is_none());
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
    assert_eq!(
        receipt.publication().raw_record_count(),
        batch.raw_records.len() as u64
    );
    assert!(receipt.publication().snapshot().is_none());
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

#[test]
fn partial_spool_with_rows_reports_partial_terminal_explicitly() {
    let capture = normalized_capture(
        include_bytes!("../../../collectors/macos/fixtures/partial.ndjson"),
        true,
    );
    let batch = ReceivedObservationBatch::from_normalized_capture(capture).unwrap();
    assert!(!batch.observations().is_empty());
    let directory = retained_tempdir();
    let mut bundle = project(&directory.path().join("spool"));
    let outcome = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &text("partial-spool/v1"), 2, &NeverCancel)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        outcome.completion().status(),
        CaptureTerminalStatus::Partial
    );
    assert!(outcome.completion().partial());
    assert!(outcome.publication().chunk().is_some());
    assert!(outcome.publication().snapshot().is_none());
    let stored = bundle
        .capture_publication(&String::from(outcome.publication().manifest().hash()))
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, CapturePublicationStatus::Terminal);
}

#[test]
fn spool_outcome_constructor_rejects_partial_and_survey_receipts() {
    use crate::spool::AcquisitionSpoolOutcome;
    let batch = batch();
    let provenance = text("spool-receipt/v1");
    let directory = retained_tempdir();
    let mut bundle = project(&directory.path().join("spool"));
    let request = AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap();
    let complete = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap(),
        )
        .unwrap();
    let publication = complete.publication();
    let missing_chunk =
        PublicationProgress::new(publication.manifest().clone(), 0, None, None).unwrap();
    assert!(AcquisitionSpoolOutcome::new(&request, missing_chunk).is_err());
    let snapshot = SnapshotReceipt::new(
        SnapshotId::from_bytes([90; 16]).unwrap(),
        publication.chunk().unwrap().project_revision(),
    )
    .unwrap();
    let wrong_snapshot = PublicationProgress::new(
        publication.manifest().clone(),
        0,
        publication.chunk().cloned(),
        Some(snapshot),
    )
    .unwrap();
    assert!(AcquisitionSpoolOutcome::new(&request, wrong_snapshot).is_err());
    assert_eq!(
        AcquisitionSpoolOutcome::new(&request, publication.clone()).unwrap(),
        complete
    );
}

#[test]
fn retained_spool_cancel_after_raw_publication_reopens_and_retries_exactly() {
    let batch = retained_batch();
    assert!(!batch.raw_records.is_empty());
    let directory = retained_tempdir();
    let path = directory.path().join("retained-cancel-spool");
    let mut bundle = project(&path);
    let calls = Cell::new(0);
    // Request admission, manifest admission, each raw record, then chunk admission.
    let stop_at = 3 + batch.raw_records.len();
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() >= stop_at
    };
    let provenance = text("retained-spool-cancel/v1");
    let error = bundle
        .persist_acquisition(AcquisitionSpoolRequest::new(&batch, &provenance, 2, &cancel).unwrap())
        .unwrap_err();
    assert_eq!(error.kind(), PortErrorKind::Cancelled);
    let progress = error.progress().unwrap().clone();
    assert_eq!(progress.raw_record_count(), batch.raw_records.len() as u64);
    assert!(progress.chunk().is_none());
    assert!(progress.snapshot().is_none());
    drop(bundle);
    let mut reopened = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    for record in &batch.raw_records {
        assert_eq!(
            reopened
                .read_artifact(&String::from(record.reference().sha256))
                .unwrap(),
            record.bytes()
        );
    }
    assert!(reopened.list_observation_chunks().unwrap().is_empty());
    let receipt = reopened
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap(),
        )
        .unwrap();
    assert_eq!(
        receipt.publication().manifest().hash(),
        progress.manifest().hash()
    );
    assert_eq!(receipt.completion(), batch.manifest().completion());
    assert_eq!(reopened.list_observation_chunks().unwrap().len(), 1);
    assert!(reopened.verify().unwrap().failures.is_empty());
}

#[test]
fn spool_manifest_rejects_equal_size_chunk_with_unrelated_identity() {
    let batch = batch();
    let directory = retained_tempdir();
    let path = directory.path().join("spool-binding");
    let mut bundle = project(&path);
    let receipt = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &text("spool-binding/v1"), 2, &NeverCancel)
                .unwrap(),
        )
        .unwrap();
    let mut data = batch.observations()[0].envelope().clone().into_data();
    data.id = ObservationId::from_bytes([44; 16]).unwrap();
    let unrelated = ObservationEnvelope::new(data).unwrap();
    let wrong_chunk = bundle
        .publish_observation_chunk(&[unrelated], "unrelated-spool/v1", 4)
        .unwrap();
    let manifest_hash = String::from(receipt.publication().manifest().hash());
    assert!(matches!(
        bundle.link_capture_chunk(&manifest_hash, wrong_chunk.hash(), wrong_chunk.row_count()),
        Err(StoreError::Corrupt(_))
    ));
    drop(bundle);
    let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let publication = reopened
        .capture_publication(&manifest_hash)
        .unwrap()
        .unwrap();
    assert_eq!(
        publication.chunk_hash,
        Some(String::from(receipt.publication().chunk().unwrap().hash()))
    );
    assert!(publication.snapshot_id.is_none());
}

#[test]
fn spool_rejects_unexpected_sql_trigger_before_publication() {
    let batch = batch();
    let directory = retained_tempdir();
    let path = directory.path().join("spool-untrusted-schema");
    let mut bundle = project(&path);
    let database = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    database.execute_batch("CREATE TRIGGER fail_spool_link BEFORE UPDATE OF chunk_hash ON capture_publications BEGIN SELECT RAISE(ABORT, 'injected spool link failure'); END;").unwrap();
    let provenance = text("spool-untrusted-schema/v1");
    let error = bundle
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PortErrorKind::Corrupt);
    assert!(error.progress().is_none());
    let chunks: i64 = database
        .query_row("SELECT COUNT(*) FROM observation_chunks", [], |row| {
            row.get(0)
        })
        .unwrap();
    let publications: i64 = database
        .query_row("SELECT COUNT(*) FROM capture_publications", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!((chunks, publications), (0, 0));
    database
        .execute_batch("DROP TRIGGER fail_spool_link;")
        .unwrap();
    drop(database);
    drop(bundle);
    let mut reopened = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let outcome = reopened
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap(),
        )
        .unwrap();
    assert!(outcome.publication().snapshot().is_none());
    assert_eq!(reopened.list_observation_chunks().unwrap().len(), 1);
    assert_eq!(
        reopened.read_observations().unwrap(),
        batch
            .observations()
            .iter()
            .map(|row| row.envelope().clone())
            .collect::<Vec<_>>()
    );
    assert!(reopened.verify().unwrap().failures.is_empty());
}

#[test]
fn spool_link_corruption_reports_committed_chunk_and_retry_reuses_it() {
    let batch = batch();
    let directory = retained_tempdir();
    let path = directory.path().join("spool-link-corruption");
    let mut bundle = project(&path);
    let database = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    let injected = Cell::new(false);
    // Inject at the first cancellation checkpoint after manifest publication.
    // Return false: this models metadata corruption, not cancellation.
    let inject = || {
        if !injected.get() {
            let changed = database
                .execute(
                    "UPDATE capture_publications SET observation_count=observation_count+1",
                    [],
                )
                .unwrap();
            if changed != 0 {
                assert_eq!(changed, 1);
                injected.set(true);
            }
        }
        false
    };
    let provenance = text("spool-link-corruption/v1");
    let error = bundle
        .persist_acquisition(AcquisitionSpoolRequest::new(&batch, &provenance, 2, &inject).unwrap())
        .unwrap_err();
    assert!(injected.get());
    assert_eq!(error.kind(), PortErrorKind::Corrupt);
    let progress = error.progress().unwrap().clone();
    let chunk_hash = progress
        .chunk()
        .expect("committed chunk receipt survives link failure")
        .hash();
    assert!(progress.snapshot().is_none());
    assert_eq!(bundle.list_observation_chunks().unwrap().len(), 1);
    let linked: Option<String> = database
        .query_row("SELECT chunk_hash FROM capture_publications", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(linked.is_none());
    // Repair only the injected metadata; production does not silently repair it.
    assert_eq!(
        database
            .execute(
                "UPDATE capture_publications SET observation_count=observation_count-1",
                []
            )
            .unwrap(),
        1
    );
    drop(database);
    drop(bundle);
    let mut reopened = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let outcome = reopened
        .persist_acquisition(
            AcquisitionSpoolRequest::new(&batch, &provenance, 2, &NeverCancel).unwrap(),
        )
        .unwrap();
    assert_eq!(
        outcome.publication().manifest().hash(),
        progress.manifest().hash()
    );
    assert_eq!(outcome.publication().chunk().unwrap().hash(), chunk_hash);
    assert_eq!(reopened.list_observation_chunks().unwrap().len(), 1);
    assert!(reopened.verify().unwrap().failures.is_empty());
}
