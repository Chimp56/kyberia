//! Read-only, bounded KismetDB 5–10 packet metadata reader.
//! Input must be a closed, regular SQLite file, not a live capture database.
//! This is the foreign decoding boundary; normalization emits only canonical
//! Kyberia observations and adapter-owned import receipts.
use kyberia_domain::{
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{AdapterId, CollectorId, ContentHash, ObservationId, SessionId, SourceId, Text},
    observation::{
        CalibrationState, ChannelContext, EnvelopeData, FrameMetadata, ObservationEnvelope,
        ObservationPayload, PayloadRetention, PrivacyState, QualityFlag, RadioIdentityEvidence,
        ReceivedObservation, SignalReading, SourceDescriptor, SourceKind,
    },
    time::{CaptureTime, UtcTimestamp, WallClockReading},
    units::{Hertz, Mbps, Seconds},
};
use rusqlite::{
    Connection, OpenFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    limits::Limit,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_BATCH: usize = 4096;
const MAX_SOURCES: usize = 1024;
const NORMALIZATION_SCHEMA_VERSION: u8 = 1;
const NORMALIZATION_PARSER_VERSION: &str = "kismetdb-observation/1";
const KISMETDB_MEDIA_TYPE: &str = "application/vnd.kismet.kismetdb";

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Invalid(&'static str),
    UnsupportedVersion(i64),
    Cancelled,
    Deadline,
    ResourceLimit,
    SourceChanged,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Imported SQL errors and paths may contain private data. Do not echo them.
        match self {
            Self::Io(_) => f.write_str("KismetDB I/O failure"),
            Self::Sqlite(_) => f.write_str("KismetDB query or schema failure"),
            Self::Invalid(reason) => write!(f, "invalid KismetDB: {reason}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported KismetDB version {v}"),
            Self::Cancelled => f.write_str("KismetDB operation cancelled"),
            Self::Deadline => f.write_str("KismetDB deadline exceeded"),
            Self::ResourceLimit => f.write_str("KismetDB resource limit exceeded"),
            Self::SourceChanged => {
                f.write_str("KismetDB source changed; close capture before import")
            }
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(v: std::io::Error) -> Self {
        Self::Io(v)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(v: rusqlite::Error) -> Self {
        Self::Sqlite(v)
    }
}

/// An explicit deadline and shared cancellation token for a complete import.
#[derive(Clone)]
pub struct Budget {
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
impl Budget {
    pub fn new(timeout: Duration, cancelled: Arc<AtomicBool>) -> Result<Self, Error> {
        if timeout.is_zero() || timeout > Duration::from_secs(120) {
            return Err(Error::Invalid(
                "timeout must be positive and at most 120 seconds",
            ));
        }
        Ok(Self {
            deadline: Instant::now() + timeout,
            cancelled,
        })
    }
    fn check(&self) -> Result<(), Error> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(Error::Deadline);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Datasource {
    /// Foreign UUID evidence, not a canonical Kyberia source ID.
    pub uuid: String,
    pub source_type: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PacketRecord {
    /// Stable identity only within the pinned database content hash.
    pub row_id: i64,
    pub datasource_uuid: String,
    pub phy_name: String,
    /// Source-reported wall time. Sensor/server clock alignment remains unknown.
    pub reported_time: UtcTimestamp,
    pub frequency: Evidence<Hertz>,
    /// PHY-specific raw integer; this field is deliberately not typed as dBm.
    pub reported_signal: i64,
    pub captured_length: u32,
    /// Missing/stripped bytes do not erase independently stored metadata.
    pub stored_payload_length: Evidence<u32>,
    pub original_length: Evidence<u32>,
    pub link_type: u32,
    pub source_error: bool,
    pub phy_rate: Evidence<Mbps>,
    pub packet_id: Evidence<u64>,
    pub payload_crc32: Evidence<u32>,
}

#[derive(Debug)]
pub struct Batch {
    pub records: Vec<PacketRecord>,
    pub next_after: Option<i64>,
    pub complete: bool,
}

/// Canonical source identity supplied by the application for one foreign
/// Kismet datasource UUID. A Kismet UUID is evidence from the imported file;
/// it is not used as a Kyberia identity by this adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceMapping {
    pub datasource_uuid: String,
    pub source_id: SourceId,
    pub collector_id: CollectorId,
    pub sensor_id: Evidence<kyberia_domain::identity::SensorId>,
    pub adapter_id: Evidence<AdapterId>,
}

/// Explicit application decisions needed to admit Kismet packet metadata into
/// canonical observations. The adapter cannot choose project identities or
/// privacy policy, and it does not manufacture a monotonic clock epoch.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizationContext {
    pub session_id: SessionId,
    pub sources: Vec<SourceMapping>,
    pub privacy: PrivacyState,
}

impl NormalizationContext {
    pub fn new(
        session_id: SessionId,
        sources: Vec<SourceMapping>,
        privacy: PrivacyState,
    ) -> Result<Self, Error> {
        if sources.len() > MAX_SOURCES {
            return Err(Error::ResourceLimit);
        }
        let mut seen = BTreeSet::new();
        let mut seen_source_ids = BTreeSet::new();
        for source in &sources {
            let normalized_uuid = source.datasource_uuid.to_ascii_lowercase();
            if !uuid(&normalized_uuid) || !seen.insert(normalized_uuid) {
                return Err(Error::Invalid("datasource mapping"));
            }
            if !seen_source_ids.insert(source.source_id) {
                return Err(Error::Invalid("duplicate canonical source mapping"));
            }
        }
        if !matches!(
            privacy.payload,
            PayloadRetention::Discarded | PayloadRetention::NotApplicable
        ) {
            return Err(Error::Invalid("Kismet packet payload is not retained"));
        }
        Ok(Self {
            session_id,
            sources,
            privacy,
        })
    }

    fn source(&self, datasource_uuid: &str) -> Result<&SourceMapping, Error> {
        self.sources
            .iter()
            .find(|source| source.datasource_uuid.eq_ignore_ascii_case(datasource_uuid))
            .ok_or(Error::Invalid("packet datasource mapping"))
    }
}

/// Provenance for one bounded normalized batch. `complete` is about the
/// requested database tail; callers must not publish a partial sequence as a
/// complete import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportReceipt {
    pub schema_version: u8,
    pub source_hash: ContentHash,
    pub source_byte_length: u64,
    pub database_schema_version: u8,
    pub producer_version: Evidence<Text>,
    pub adapter_version: Text,
    pub parser_version: Text,
    pub row_count: u64,
    pub complete: bool,
}

#[derive(Debug)]
pub struct NormalizedBatch {
    pub observations: Vec<ReceivedObservation>,
    /// One receipt per observation, in the same deterministic row order. The
    /// raw signal is retained as an untyped source integer because the Kismet
    /// schema does not establish dBm semantics.
    pub row_receipts: Vec<ObservationReceipt>,
    pub next_after: Option<i64>,
    pub complete: bool,
    pub receipt: ImportReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationReceipt {
    pub observation_id: ObservationId,
    pub source_hash: ContentHash,
    pub source_row_id: i64,
    pub raw_signal: i64,
    pub source_error: bool,
}

pub struct KismetDb {
    connection: Connection,
    snapshot: tempfile::NamedTempFile,
    schema_version: u8,
    sha256: [u8; 32],
    byte_length: u64,
    datasources: Vec<Datasource>,
    budget: Budget,
}

fn snapshot(
    path: &Path,
    budget: &Budget,
) -> Result<(tempfile::NamedTempFile, [u8; 32], u64), Error> {
    budget.check()?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::Invalid("regular file required"));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(Error::ResourceLimit);
    }
    for suffix in ["-wal", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        if Path::new(&sidecar).try_exists()? {
            return Err(Error::SourceChanged);
        }
    }
    let mut file = File::open(path)?;
    let mut copy = tempfile::Builder::new()
        .prefix("kyberia-kismet-private-")
        .tempfile()?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut count = 0u64;
    loop {
        budget.check()?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > MAX_FILE_BYTES {
            return Err(Error::ResourceLimit);
        }
        copy.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
    }
    if count != metadata.len() {
        return Err(Error::SourceChanged);
    }
    copy.flush()?;
    Ok((copy, hash.finalize().into(), count))
}

fn text(row: &rusqlite::Row<'_>, index: usize) -> Result<String, Error> {
    let value: String = row.get(index)?;
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(Error::Invalid("metadata text"));
    }
    Ok(value)
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
        && value.bytes().any(|b| b != b'0' && b != b'-')
}
fn unsigned(value: i64) -> Result<u32, Error> {
    value
        .try_into()
        .map_err(|_| Error::Invalid("unsigned packet metadata"))
}
fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}

fn unknown_as<T>(reason: UnknownReason) -> Evidence<T> {
    Evidence::Unknown(reason)
}

fn domain_text(value: impl Into<String>) -> Result<Text, Error> {
    Text::new(value).map_err(|_| Error::Invalid("canonical text"))
}

fn derived_id_bytes(hash: [u8; 32], tag: &[u8], row_id: Option<i64>) -> [u8; 16] {
    let mut digest = Sha256::new();
    digest.update(tag);
    digest.update(hash);
    if let Some(row_id) = row_id {
        digest.update(row_id.to_be_bytes());
    }
    let bytes: [u8; 32] = digest.finalize().into();
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes[..16]);
    // A cryptographic digest can theoretically be all zero, while domain IDs
    // intentionally reject that value. Keep the derivation total and stable.
    if id == [0; 16] {
        id[0] = 1;
    }
    id
}

fn observation_id(hash: [u8; 32], row_id: i64) -> Result<ObservationId, Error> {
    ObservationId::from_bytes(derived_id_bytes(hash, b"kismet-observation", Some(row_id)))
        .map_err(|_| Error::Invalid("observation identity"))
}

fn capture_time(record: &PacketRecord) -> Result<CaptureTime, Error> {
    Ok(CaptureTime {
        wall: Evidence::Known(WallClockReading {
            time: record.reported_time,
            source: domain_text("KismetDB packets.ts_sec+ts_usec")?,
            precision: Seconds::new(0.000001).map_err(|_| Error::Invalid("timestamp precision"))?,
            uncertainty: unknown(),
        }),
        // KismetDB packet rows do not contain a monotonic source clock or a
        // cross-clock model. The source wall timestamp is never substituted.
        monotonic: unknown_as(UnknownReason::ClockUnavailable),
        synchronization: unknown_as(UnknownReason::ClockUnavailable),
    })
}

fn unknown_identity() -> RadioIdentityEvidence {
    RadioIdentityEvidence {
        physical_device: unknown(),
        radio: unknown(),
        bss: unknown(),
        bssid: unknown(),
        ess: unknown(),
        mld: unknown(),
        link_id: unknown(),
        client: unknown(),
        grouping_evidence: unknown_as(UnknownReason::NotRetained),
    }
}

fn canonical_channel(record: &PacketRecord) -> ChannelContext {
    ChannelContext {
        // The Kismet packet table supplies a frequency but not a complete
        // RF Atlas channel geometry. Keep the source frequency in its typed
        // primary-frequency slot and leave channel/band/width geometry open.
        band: unknown(),
        primary_channel: unknown(),
        primary_frequency: record.frequency.clone(),
        center_frequency: unknown_as(UnknownReason::SourceDidNotProvide),
        second_center_frequency: unknown_as(UnknownReason::SourceDidNotProvide),
        width: unknown_as(UnknownReason::SourceDidNotProvide),
        puncturing: unknown_as(UnknownReason::SourceDidNotProvide),
    }
}

fn source_descriptor(
    mapping: &SourceMapping,
    schema_version: u8,
) -> Result<SourceDescriptor, Error> {
    Ok(SourceDescriptor {
        source_id: mapping.source_id,
        collector_id: mapping.collector_id,
        sensor_id: mapping.sensor_id.clone(),
        adapter_id: mapping.adapter_id.clone(),
        kind: SourceKind::DatabaseImport,
        source_name: domain_text("KismetDB")?,
        // A KismetDB schema version is not a Kismet producer/software version.
        source_version: unknown(),
        source_schema_version: domain_text(format!("kismetdb/{schema_version}"))?,
        adapter_name: domain_text("kyberia-kismet-adapter")?,
        adapter_version: domain_text(env!("CARGO_PKG_VERSION"))?,
        parser_version: domain_text(NORMALIZATION_PARSER_VERSION)?,
        driver_version: unknown(),
        os_version: unknown(),
    })
}

fn normalize_record(
    record: &PacketRecord,
    mapping: &SourceMapping,
    schema_version: u8,
    session_id: SessionId,
    raw_source: &ArtifactReference,
    privacy: &PrivacyState,
) -> Result<ReceivedObservation, Error> {
    let mut quality = vec![QualityFlag::ClockUncertain, QualityFlag::UnknownCalibration];
    if record.source_error {
        // Kismet explicitly reported an error for this packet row. Preserve
        // the metadata row, but prevent it from being treated as clean frame
        // evidence by strict consumers.
        quality.push(QualityFlag::Malformed);
    }
    let signal = SignalReading {
        // `signal` is a PHY-specific Kismet integer. No dBm conversion is
        // justified by the supported database schema alone.
        rssi_dbm: unknown_as(UnknownReason::UnsupportedCapability),
        noise_dbm: unknown_as(UnknownReason::SourceDidNotProvide),
        chains: Vec::new(),
        calibration: Evidence::Known(CalibrationState::Uncalibrated),
        measurement_method: domain_text("KismetDB packet signal; source unit unknown")?,
    };
    let frame = FrameMetadata {
        identity: unknown_identity(),
        signal,
        frame_type: unknown(),
        frame_subtype: unknown(),
        retry: unknown(),
        length_bytes: record.captured_length,
        phy_rate_mbps: record.phy_rate.clone(),
        raw_information_elements: unknown_as(UnknownReason::NotRetained),
    };
    let envelope = ObservationEnvelope::new(EnvelopeData {
        schema_version: kyberia_domain::observation::ObservationSchemaVersion::V2,
        id: observation_id(raw_source.sha256.bytes(), record.row_id)?,
        session_id,
        source: source_descriptor(mapping, schema_version)?,
        time: capture_time(record)?,
        pose: unknown_as(UnknownReason::SourceDidNotProvide),
        channel: Evidence::Known(canonical_channel(record)),
        dwell: unknown_as(UnknownReason::SourceDidNotProvide),
        privacy: privacy.clone(),
        quality,
        raw_source: Evidence::Known(raw_source.clone()),
        payload: ObservationPayload::Frame(frame),
    })
    .map_err(|_| Error::Invalid("canonical observation"))?;
    ReceivedObservation::new(envelope, unknown_as(UnknownReason::SourceDidNotProvide))
        .map_err(|_| Error::Invalid("canonical reception"))
}

impl KismetDb {
    pub fn open(path: impl AsRef<Path>, budget: Budget) -> Result<Self, Error> {
        let (snapshot, sha256, byte_length) = snapshot(path.as_ref(), &budget)?;
        let connection = Connection::open_with_flags(
            snapshot.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::ZERO)?;
        connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, 1024 * 1024)?;
        connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 16 * 1024)?;
        connection.set_limit(Limit::SQLITE_LIMIT_COLUMN, 128)?;
        connection.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, 32)?;
        connection.set_limit(Limit::SQLITE_LIMIT_VDBE_OP, 100_000)?;
        connection.execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; PRAGMA temp_store=MEMORY; BEGIN;",
        )?;
        let watch = budget.clone();
        let mut operations = 0u64;
        connection.progress_handler(
            1000,
            Some(move || {
                operations += 1000;
                operations > 20_000_000 || watch.check().is_err()
            }),
        )?;
        connection.authorizer(Some(|context: AuthContext<'_>| {
            let allowed = context.accessor.is_none()
                && match context.action {
                    AuthAction::Select => true,
                    AuthAction::Read { table_name, .. } => [
                        "sqlite_master",
                        "sqlite_schema",
                        "KISMET",
                        "datasources",
                        "packets",
                    ]
                    .contains(&table_name),
                    AuthAction::Pragma {
                        pragma_name: "table_xinfo",
                        ..
                    } => true,
                    AuthAction::Function {
                        function_name: "length" | "typeof",
                    } => true,
                    _ => false,
                };
            if allowed {
                Authorization::Allow
            } else {
                Authorization::Deny
            }
        }))?;
        for table in ["KISMET", "datasources", "packets"] {
            let (kind, sql): (String, String) = connection.query_row(
                "SELECT type,sql FROM sqlite_schema WHERE name=?1",
                [table],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if kind != "table"
                || !sql
                    .trim_start()
                    .to_ascii_uppercase()
                    .starts_with("CREATE TABLE")
                || sql.to_ascii_uppercase().contains("WITHOUT ROWID")
            {
                return Err(Error::Invalid("ordinary rowid tables required"));
            }
            let mut statement = connection.prepare(&format!("PRAGMA table_xinfo({table})"))?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if ["rowid", "_rowid_", "oid"].contains(&name.to_ascii_lowercase().as_str()) {
                    return Err(Error::Invalid("shadowed row identity"));
                }
            }
        }
        let mut statement = connection.prepare("SELECT db_version FROM KISMET LIMIT 2")?;
        let versions: Vec<i64> = statement
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if versions.len() != 1 {
            return Err(Error::Invalid("exactly one database version required"));
        }
        let version = versions[0];
        if !(5..=10).contains(&version) {
            return Err(Error::UnsupportedVersion(version));
        }
        drop(statement);
        let mut statement = connection
            .prepare("SELECT uuid,typestring FROM datasources ORDER BY _rowid_ LIMIT 1025")?;
        let mut rows = statement.query([])?;
        let mut datasources = Vec::new();
        let mut seen = BTreeSet::new();
        while let Some(row) = rows.next()? {
            budget.check()?;
            let uuid = text(row, 0)?.to_ascii_lowercase();
            if !self::uuid(&uuid) || !seen.insert(uuid.clone()) {
                return Err(Error::Invalid("datasource UUID"));
            }
            datasources.push(Datasource {
                uuid,
                source_type: text(row, 1)?,
            });
            if datasources.len() > MAX_SOURCES {
                return Err(Error::ResourceLimit);
            }
        }
        drop(rows);
        drop(statement);
        budget.check()?;
        Ok(Self {
            connection,
            snapshot,
            schema_version: version as u8,
            sha256,
            byte_length,
            datasources,
            budget,
        })
    }

    pub fn schema_version(&self) -> u8 {
        self.schema_version
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    pub fn byte_length(&self) -> u64 {
        self.byte_length
    }
    pub fn datasources(&self) -> &[Datasource] {
        &self.datasources
    }

    /// Return the immutable source artifact identity used by every normalized
    /// observation from this database. The adapter never copies this artifact
    /// into a project or silently retains its packet payload.
    pub fn source_artifact(&self) -> Result<ArtifactReference, Error> {
        Ok(ArtifactReference {
            sha256: ContentHash::from_sha256(self.sha256),
            media_type: domain_text(KISMETDB_MEDIA_TYPE)?,
            byte_length: self.byte_length,
        })
    }

    /// Normalize one bounded row batch into canonical V2 received
    /// observations. The cursor and every returned batch must be retained by
    /// the caller; only a batch with `complete == true`, followed by
    /// [`KismetDb::finish`], represents a complete database import.
    pub fn normalize_batch(
        &self,
        after: Option<i64>,
        limit: usize,
        context: &NormalizationContext,
    ) -> Result<NormalizedBatch, Error> {
        self.budget.check()?;
        if self.datasources.len() != context.sources.len()
            || self
                .datasources
                .iter()
                .any(|source| context.source(&source.uuid).is_err())
        {
            return Err(Error::Invalid("incomplete datasource mapping"));
        }
        let batch = self.read_batch(after, limit)?;
        let raw_source = self.source_artifact()?;
        let mut observations = Vec::with_capacity(batch.records.len());
        let mut row_receipts = Vec::with_capacity(batch.records.len());
        for record in &batch.records {
            let mapping = context.source(&record.datasource_uuid)?;
            let id = observation_id(raw_source.sha256.bytes(), record.row_id)?;
            observations.push(normalize_record(
                record,
                mapping,
                self.schema_version,
                context.session_id,
                &raw_source,
                &context.privacy,
            )?);
            row_receipts.push(ObservationReceipt {
                observation_id: id,
                source_hash: raw_source.sha256,
                source_row_id: record.row_id,
                raw_signal: record.reported_signal,
                source_error: record.source_error,
            });
        }
        self.budget.check()?;
        Ok(NormalizedBatch {
            observations,
            row_receipts,
            next_after: batch.next_after,
            complete: batch.complete,
            receipt: ImportReceipt {
                schema_version: NORMALIZATION_SCHEMA_VERSION,
                source_hash: raw_source.sha256,
                source_byte_length: raw_source.byte_length,
                database_schema_version: self.schema_version,
                producer_version: unknown(),
                adapter_version: domain_text(env!("CARGO_PKG_VERSION"))?,
                parser_version: domain_text(NORMALIZATION_PARSER_VERSION)?,
                row_count: batch.records.len() as u64,
                complete: batch.complete,
            },
        })
    }

    /// Keyset pagination in file row order; never reorders by an uncertain clock.
    /// No packet bytes, identifiers, GPS, datasource definitions or device JSON
    /// are loaded. Partial batches must not be labeled a complete import.
    pub fn read_batch(&self, after: Option<i64>, limit: usize) -> Result<Batch, Error> {
        self.budget.check()?;
        if limit == 0 || limit > MAX_BATCH {
            return Err(Error::ResourceLimit);
        }
        let original = if self.schema_version >= 9 {
            "packet_full_len"
        } else {
            "NULL"
        };
        let rate = if self.schema_version >= 7 {
            "datarate"
        } else {
            "NULL"
        };
        let ids = if self.schema_version >= 8 {
            "packetid,hash"
        } else {
            "NULL,NULL"
        };
        let predicate = if after.is_some() {
            "WHERE _rowid_>?1"
        } else {
            ""
        };
        let sql = format!(
            "SELECT _rowid_,datasource,phyname,ts_sec,ts_usec,frequency,signal,packet_len,{original},dlt,error,{rate},{ids},length(packet),typeof(packet) FROM packets {predicate} ORDER BY _rowid_ LIMIT ?2"
        );
        let result = (|| {
            let mut statement = self.connection.prepare(&sql)?;
            let mut rows = statement.query(rusqlite::params![after, (limit + 1) as i64])?;
            let mut records = Vec::with_capacity(limit);
            let mut complete = true;
            while let Some(row) = rows.next()? {
                self.budget.check()?;
                if records.len() == limit {
                    complete = false;
                    break;
                }
                let datasource_uuid = text(row, 1)?.to_ascii_lowercase();
                if !self.datasources.iter().any(|s| s.uuid == datasource_uuid) {
                    return Err(Error::Invalid("packet references missing datasource"));
                }
                let seconds: i64 = row.get(3)?;
                let micros: i64 = row.get(4)?;
                if !(0..1_000_000).contains(&micros) {
                    return Err(Error::Invalid("microsecond timestamp"));
                }
                let nanos = seconds
                    .checked_mul(1_000_000_000)
                    .and_then(|s| s.checked_add(micros * 1000))
                    .ok_or(Error::Invalid("timestamp overflow"))?;
                let raw_frequency: f64 = row.get(5)?;
                let frequency = if raw_frequency == 0.0 {
                    unknown()
                } else {
                    if raw_frequency < 0.0 {
                        return Err(Error::Invalid("negative frequency"));
                    }
                    Evidence::Known(
                        Hertz::new(raw_frequency * 1000.0)
                            .map_err(|_| Error::Invalid("frequency"))?,
                    )
                };
                let captured_length = unsigned(row.get(7)?)?;
                let raw_length: Option<i64> = row.get(14)?;
                let payload_type: String = row.get(15)?;
                if !["blob", "null"].contains(&payload_type.as_str()) {
                    return Err(Error::Invalid("packet must be blob or absent"));
                }
                let stored_payload_length = match raw_length {
                    None => Evidence::Unknown(UnknownReason::NotRetained),
                    Some(0) if captured_length > 0 => Evidence::Unknown(UnknownReason::NotRetained),
                    Some(n) if n == i64::from(captured_length) => Evidence::Known(captured_length),
                    Some(_) => return Err(Error::Invalid("packet byte length mismatch")),
                };
                let original_length = match row.get::<_, Option<i64>>(8)? {
                    Some(n) if n >= i64::from(captured_length) => Evidence::Known(unsigned(n)?),
                    Some(_) => return Err(Error::Invalid("original length below captured length")),
                    None => unknown(),
                };
                let source_error = match row.get::<_, i64>(10)? {
                    0 => false,
                    1 => true,
                    _ => return Err(Error::Invalid("packet error flag")),
                };
                let phy_rate = match row.get::<_, Option<f64>>(11)? {
                    Some(0.0) | None => unknown(),
                    Some(n) => {
                        Evidence::Known(Mbps::new(n).map_err(|_| Error::Invalid("PHY rate"))?)
                    }
                };
                let packet_id = match row.get::<_, Option<i64>>(12)? {
                    Some(n) => {
                        Evidence::Known(n.try_into().map_err(|_| Error::Invalid("packet ID"))?)
                    }
                    None => unknown(),
                };
                let payload_crc32 = match row.get::<_, Option<i64>>(13)? {
                    Some(n) => Evidence::Known(unsigned(n)?),
                    None => unknown(),
                };
                records.push(PacketRecord {
                    row_id: row.get(0)?,
                    datasource_uuid,
                    phy_name: text(row, 2)?,
                    reported_time: UtcTimestamp(nanos),
                    frequency,
                    reported_signal: row.get(6)?,
                    captured_length,
                    stored_payload_length,
                    original_length,
                    link_type: unsigned(row.get(9)?)?,
                    source_error,
                    phy_rate,
                    packet_id,
                    payload_crc32,
                });
            }
            Ok(Batch {
                next_after: records.last().map(|r| r.row_id).or(after),
                records,
                complete,
            })
        })();
        self.budget.check()?;
        result
    }

    /// Complete the import budget check. All reads use the private hashed copy;
    /// original-path replacement after open cannot change admitted evidence.
    /// The owned temporary file is unlinked individually on drop, never by a
    /// recursive cleanup operation. Crash recovery does not imply secure erasure.
    pub fn finish(self) -> Result<(), Error> {
        self.budget.check()?;
        if self.snapshot.as_file().metadata()?.len() != self.byte_length {
            return Err(Error::SourceChanged);
        }
        Ok(())
    }
}
