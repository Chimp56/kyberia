use kyberia_domain::{evidence::Evidence, time::UtcTimestamp};
use kyberia_kismet_adapter::database::{Budget, Error, KismetDb};
use rusqlite::Connection;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const SOURCE: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn budget() -> Budget {
    Budget::new(Duration::from_secs(30), Arc::new(AtomicBool::new(false))).unwrap()
}
fn fixture(version: u8, count: usize) -> PathBuf {
    // Retain explicitly synthetic fixture directories under the deletion policy.
    let dir = tempfile::Builder::new()
        .prefix("kyberia-kismet-synthetic-")
        .tempdir()
        .unwrap()
        .keep();
    let path = dir.join("fixture.kismet");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("BEGIN").unwrap();
    db.execute_batch("CREATE TABLE KISMET(db_version INTEGER); CREATE TABLE datasources(uuid TEXT,typestring TEXT,definition TEXT); CREATE TABLE devices(strongest_signal INTEGER,avg_lat REAL,avg_lon REAL);").unwrap();
    db.execute("INSERT INTO KISMET VALUES(?1)", [version])
        .unwrap();
    db.execute("INSERT INTO datasources VALUES(?1,'linuxwifi','private source definition must not be read')",[SOURCE]).unwrap();
    // An aggregate with excellent signal must have no effect on packet evidence.
    db.execute("INSERT INTO devices VALUES(-1,42,42)", [])
        .unwrap();
    let original = if version >= 9 {
        ",packet_full_len INTEGER"
    } else {
        ""
    };
    let rate = if version >= 7 { ",datarate REAL" } else { "" };
    let ids = if version >= 8 {
        ",packetid INTEGER,hash INTEGER"
    } else {
        ""
    };
    db.execute_batch(&format!("CREATE TABLE packets(ts_sec INTEGER,ts_usec INTEGER,phyname TEXT,frequency REAL,signal INTEGER,datasource TEXT,packet_len INTEGER,dlt INTEGER,error INTEGER,packet BLOB{original}{rate}{ids})")).unwrap();
    for i in 0..count {
        db.execute("INSERT INTO packets(ts_sec,ts_usec,phyname,frequency,signal,datasource,packet_len,dlt,error,packet) VALUES(1700000000,123456,'IEEE80211',2412000,-65,?1,4,127,0,x'00010203')", [SOURCE]).unwrap();
        if version >= 9 {
            db.execute(
                "UPDATE packets SET packet_full_len=12 WHERE rowid=?1",
                [(i + 1) as i64],
            )
            .unwrap();
        }
        if version >= 7 {
            db.execute(
                "UPDATE packets SET datarate=54 WHERE rowid=?1",
                [(i + 1) as i64],
            )
            .unwrap();
        }
        if version >= 8 {
            db.execute(
                "UPDATE packets SET packetid=7,hash=123 WHERE rowid=?1",
                [(i + 1) as i64],
            )
            .unwrap();
        }
    }
    db.execute_batch("COMMIT").unwrap();
    path
}
fn mutate(path: &std::path::Path, sql: &str) {
    Connection::open(path).unwrap().execute_batch(sql).unwrap();
}

#[test]
fn schema_versions_preserve_units_unknowns_and_never_use_device_aggregates() {
    for version in 5..=10 {
        let path = fixture(version, 1);
        let before = std::fs::read(&path).unwrap();
        let db = KismetDb::open(&path, budget()).unwrap();
        assert_eq!(db.schema_version(), version);
        let batch = db.read_batch(None, 10).unwrap();
        assert!(batch.complete);
        assert_eq!(batch.records.len(), 1);
        let record = &batch.records[0];
        assert_eq!(
            record.reported_time,
            UtcTimestamp(1_700_000_000_123_456_000)
        );
        assert_eq!(record.frequency.as_known().unwrap().get(), 2_412_000_000.0);
        assert_eq!(record.reported_signal, -65);
        assert_eq!(record.captured_length, 4);
        assert_eq!(
            record.original_length.as_known().copied(),
            if version >= 9 { Some(12) } else { None }
        );
        assert_eq!(
            record.phy_rate.as_known().map(|v| v.get()),
            if version >= 7 { Some(54.0) } else { None }
        );
        assert_eq!(
            record.packet_id.as_known().copied(),
            if version >= 8 { Some(7) } else { None }
        );
        db.finish().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn keyset_batches_keep_identical_transmissions_and_tied_timestamps() {
    let db = KismetDb::open(fixture(10, 5), budget()).unwrap();
    let first = db.read_batch(None, 2).unwrap();
    assert!(!first.complete);
    let second = db.read_batch(first.next_after, 2).unwrap();
    assert!(!second.complete);
    let last = db.read_batch(second.next_after, 2).unwrap();
    assert!(last.complete);
    let ids: Vec<_> = first
        .records
        .iter()
        .chain(&second.records)
        .chain(&last.records)
        .map(|r| r.row_id)
        .collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    // Same packetid/hash are correlation hints, not permission to drop evidence.
    assert_eq!(last.records[0].packet_id, Evidence::Known(7));
    let empty = db.read_batch(last.next_after, 2).unwrap();
    assert!(empty.complete && empty.records.is_empty());
    db.finish().unwrap();
}

#[test]
fn missing_sources_and_bad_metadata_fail_without_partial_success() {
    for sql in [
        "UPDATE packets SET datasource='ffffffff-ffff-ffff-ffff-ffffffffffff'",
        "UPDATE packets SET ts_usec=1000000",
        "UPDATE packets SET ts_usec=-1",
        "UPDATE packets SET ts_sec=9223372036854775807",
        "UPDATE packets SET frequency=-1",
        "UPDATE packets SET frequency=1e999",
        "UPDATE packets SET datarate=-2",
        "UPDATE packets SET datarate=1e999",
        "UPDATE packets SET packet_len=3",
        "UPDATE packets SET packet_full_len=2",
        "UPDATE packets SET packetid=-1",
        "UPDATE packets SET hash=4294967296",
        "UPDATE packets SET error=2",
        "UPDATE packets SET phyname=char(10)",
        "UPDATE packets SET ts_usec='invented'",
        "UPDATE packets SET packet='text'",
    ] {
        let path = fixture(10, 2);
        mutate(&path, sql);
        let db = KismetDb::open(path, budget()).unwrap();
        assert!(db.read_batch(None, 10).is_err(), "{sql}");
    }
}

#[test]
fn absent_frequency_and_rate_are_unknown_not_zero_measurements() {
    let path = fixture(10, 1);
    mutate(&path, "UPDATE packets SET frequency=0,datarate=0,signal=0");
    let db = KismetDb::open(path, budget()).unwrap();
    let record = &db.read_batch(None, 1).unwrap().records[0];
    assert!(matches!(record.frequency, Evidence::Unknown(_)));
    assert!(matches!(record.phy_rate, Evidence::Unknown(_)));
    // Raw zero remains raw metadata, never a known 0 dBm reading.
    assert_eq!(record.reported_signal, 0);
}

#[test]
fn schema_and_source_identity_are_strict() {
    for sql in [
        "UPDATE KISMET SET db_version=11",
        "UPDATE KISMET SET db_version=4",
        "INSERT INTO KISMET VALUES(10)",
        "UPDATE KISMET SET db_version='not a version'",
        "UPDATE datasources SET uuid='invalid'",
        "INSERT INTO datasources SELECT * FROM datasources",
        "ALTER TABLE packets ADD COLUMN _rowid_ INTEGER",
        "ALTER TABLE packets ADD COLUMN _rowid_ INTEGER GENERATED ALWAYS AS (1) VIRTUAL",
        "ALTER TABLE packets ADD COLUMN RoWiD INTEGER GENERATED ALWAYS AS (1) VIRTUAL",
        "ALTER TABLE packets ADD COLUMN oid INTEGER GENERATED ALWAYS AS (1) VIRTUAL",
        "ALTER TABLE KISMET RENAME TO old_version; CREATE TABLE KISMET(db_version INTEGER,_rowid_ INTEGER GENERATED ALWAYS AS (db_version) STORED); INSERT INTO KISMET(db_version) VALUES(10)",
        "ALTER TABLE packets RENAME TO hidden; CREATE VIEW packets AS SELECT * FROM hidden",
        "DROP TABLE packets; CREATE VIRTUAL TABLE packets USING fts5(content)",
    ] {
        let path = fixture(10, 1);
        mutate(&path, sql);
        assert!(KismetDb::open(path, budget()).is_err(), "{sql}");
    }
}

#[test]
fn cancellation_limits_and_deadlines_are_explicit() {
    let path = fixture(10, 1);
    let cancelled = Arc::new(AtomicBool::new(true));
    let b = Budget::new(Duration::from_secs(30), cancelled.clone()).unwrap();
    assert!(matches!(KismetDb::open(&path, b), Err(Error::Cancelled)));
    cancelled.store(false, Ordering::Relaxed);
    let db = KismetDb::open(
        &path,
        Budget::new(Duration::from_secs(30), cancelled.clone()).unwrap(),
    )
    .unwrap();
    assert!(matches!(db.read_batch(None, 0), Err(Error::ResourceLimit)));
    assert!(matches!(
        db.read_batch(None, 4097),
        Err(Error::ResourceLimit)
    ));
    cancelled.store(true, Ordering::Relaxed);
    assert!(matches!(db.read_batch(None, 1), Err(Error::Cancelled)));
    let deadline = Budget::new(Duration::from_nanos(1), Arc::new(AtomicBool::new(false))).unwrap();
    assert!(matches!(
        KismetDb::open(path, deadline),
        Err(Error::Deadline)
    ));
}

#[test]
fn changed_sources_wal_and_nonfiles_are_rejected() {
    let path = fixture(10, 1);
    let db = KismetDb::open(&path, budget()).unwrap();
    // Replacing the original path cannot alter the private hashed snapshot.
    let replacement = fixture(10, 2);
    let saved = path.with_extension("original-retained");
    std::fs::rename(&path, &saved).unwrap();
    std::fs::copy(replacement, &path).unwrap();
    assert_eq!(db.read_batch(None, 10).unwrap().records.len(), 1);
    db.finish().unwrap();
    std::fs::write(
        path.with_file_name("fixture.kismet-wal"),
        b"not checkpointed",
    )
    .unwrap();
    assert!(matches!(
        KismetDb::open(&path, budget()),
        Err(Error::SourceChanged)
    ));
    assert!(KismetDb::open(path.parent().unwrap(), budget()).is_err());
    #[cfg(unix)]
    {
        let link = path.with_extension("link");
        std::os::unix::fs::symlink(&saved, &link).unwrap();
        assert!(matches!(
            KismetDb::open(link, budget()),
            Err(Error::Invalid(_))
        ));
    }
}

#[test]
fn oversized_metadata_and_corrupt_sqlite_fail_bounded() {
    let path = fixture(10, 1);
    mutate(
        &path,
        "UPDATE datasources SET typestring=hex(zeroblob(600000))",
    );
    assert!(KismetDb::open(path, budget()).is_err());
    let path = fixture(10, 0);
    std::fs::write(&path, b"not sqlite").unwrap();
    assert!(KismetDb::open(path, budget()).is_err());
}

#[test]
fn negative_and_extreme_rowids_paginate_without_loss() {
    let path = fixture(10, 3);
    mutate(
        &path,
        "UPDATE packets SET rowid=-9223372036854775808 WHERE rowid=1; UPDATE packets SET rowid=0 WHERE rowid=2; UPDATE packets SET rowid=9223372036854775807 WHERE rowid=3;",
    );
    let db = KismetDb::open(path, budget()).unwrap();
    let a = db.read_batch(None, 1).unwrap();
    let b = db.read_batch(a.next_after, 1).unwrap();
    let c = db.read_batch(b.next_after, 1).unwrap();
    assert_eq!(
        vec![
            a.records[0].row_id,
            b.records[0].row_id,
            c.records[0].row_id
        ],
        vec![i64::MIN, 0, i64::MAX]
    );
    assert!(c.complete);
}

#[test]
fn stripped_packet_payloads_preserve_metadata_and_expose_absence() {
    for replacement in ["NULL", "x''"] {
        let path = fixture(10, 1);
        mutate(&path, &format!("UPDATE packets SET packet={replacement}"));
        let db = KismetDb::open(path, budget()).unwrap();
        let batch = db.read_batch(None, 1).unwrap();
        let record = &batch.records[0];
        assert_eq!(record.captured_length, 4);
        assert_eq!(record.reported_signal, -65);
        assert!(matches!(record.stored_payload_length, Evidence::Unknown(_)));
        assert_eq!(record.original_length, Evidence::Known(12));
        db.finish().unwrap();
    }
}

#[test]
fn snapshot_hash_is_of_the_exact_bytes_queried_despite_original_replacement() {
    use sha2::{Digest, Sha256};
    let path = fixture(10, 3);
    let bytes = std::fs::read(&path).unwrap();
    let db = KismetDb::open(&path, budget()).unwrap();
    assert_eq!(db.sha256(), <[u8; 32]>::from(Sha256::digest(&bytes)));
    mutate(&path, "UPDATE packets SET signal=-19");
    let replacement = KismetDb::open(&path, budget()).unwrap();
    assert_ne!(db.sha256(), replacement.sha256());
    assert_eq!(
        db.read_batch(None, 3).unwrap().records[0].reported_signal,
        -65
    );
    assert_eq!(
        replacement.read_batch(None, 3).unwrap().records[0].reported_signal,
        -19
    );
    db.finish().unwrap();
    replacement.finish().unwrap();
}

#[test]
fn random_corrupt_headers_never_panic_or_become_empty_success() {
    let path = fixture(10, 0);
    let mut state = 17u32;
    for _ in 0..64 {
        let bytes: Vec<_> = (0..512)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 24) as u8
            })
            .collect();
        std::fs::write(&path, bytes).unwrap();
        assert!(KismetDb::open(&path, budget()).is_err());
    }
}

#[test]
#[ignore = "explicit real SQLite throughput benchmark; retains synthetic input"]
fn metadata_batch_benchmark() {
    use std::time::Instant;
    for count in [10_000, 100_000] {
        let path = fixture(10, count);
        let start = Instant::now();
        let db = KismetDb::open(&path, budget()).unwrap();
        let opened = start.elapsed();
        let mut after = None;
        let mut total = 0;
        loop {
            let batch = db.read_batch(after, 512).unwrap();
            total += batch.records.len();
            after = batch.next_after;
            if batch.complete {
                break;
            }
        }
        db.finish().unwrap();
        assert_eq!(total, count);
        eprintln!(
            "KismetDB rows={count} batch=512 snapshot_open_ms={:.3} total_ms={:.3}",
            opened.as_secs_f64() * 1000.0,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
