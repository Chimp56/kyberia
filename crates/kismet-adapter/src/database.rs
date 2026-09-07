//! Read-only, bounded KismetDB 5–10 packet metadata reader.
//! Input must be a closed, regular SQLite file, not a live capture database.
//! This is the foreign decoding boundary; canonical normalization is separate.
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    time::UtcTimestamp,
    units::{Hertz, Mbps},
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
