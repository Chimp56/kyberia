use crate::manifest::{MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES, SCHEMA_VERSION, validate_hash};
use crate::{
    ArtifactEntry, ArtifactKind, BundleManifest, Result, StoreError, content_hash, sqlite_guard,
};
use kyberia_domain::identity::{ProjectId, SnapshotId};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    ReadOnly,
    ReadWrite,
}

pub struct Bundle {
    pub(crate) root: PathBuf,
    pub(crate) connection: Connection,
    pub(crate) mode: OpenMode,
}

#[derive(Debug, Serialize)]
pub struct Verification {
    pub schema_version: u32,
    pub artifact_count: usize,
    pub artifact_bytes: u64,
    pub projection_current: bool,
    pub failures: Vec<String>,
}

pub(crate) fn regular(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(StoreError::Invalid(format!(
            "expected nonsymlink {}: {}",
            if directory { "directory" } else { "file" },
            path.display()
        )));
    }
    Ok(())
}

fn open_nonsymlink_read(path: &Path) -> Result<File> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;

        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        options.open(path).map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                StoreError::Invalid("regular file required; symlink is forbidden".into())
            } else {
                StoreError::Io(error)
            }
        })?
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            SECURITY_IDENTIFICATION,
        };

        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .security_qos_flags(SECURITY_IDENTIFICATION);
        options.open(path)?
    };
    #[cfg(not(any(unix, windows)))]
    return Err(StoreError::Invalid(
        "safe nonsymlink artifact reads are unsupported on this platform".into(),
    ));

    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(StoreError::Invalid(format!(
            "expected nonsymlink file: {}",
            path.display()
        )));
    }
    Ok(file)
}

fn read_file_after_size(mut file: File, expected_bytes: u64) -> Result<(Vec<u8>, bool)> {
    let expected_bytes = usize::try_from(expected_bytes)
        .map_err(|_| StoreError::Invalid("file size is not representable".into()))?;
    let mut result = vec![0_u8; expected_bytes];
    let mut offset = 0;
    while offset < expected_bytes {
        let read = file.read(&mut result[offset..])?;
        if read == 0 {
            result.truncate(offset);
            break;
        }
        offset += read;
    }
    let mut extra = [0_u8; 1];
    let has_extra = file.read(&mut extra)? != 0;
    Ok((result, has_extra))
}

#[cfg(test)]
mod read_admission_tests {
    use super::*;

    fn retained_file() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
        fs::create_dir_all(&root).unwrap();
        tempfile::Builder::new()
            .prefix("artifact-read-admission-")
            .tempdir_in(root)
            .unwrap()
            .keep()
            .join("artifact")
    }

    #[test]
    fn growth_after_metadata_uses_only_admitted_buffer_and_stack_guard() {
        for original in [0, 1, 1024] {
            let path = retained_file();
            fs::write(&path, vec![7; original]).unwrap();
            let file = open_nonsymlink_read(&path).unwrap();
            let admitted = file.metadata().unwrap().len();
            let mut writer = OpenOptions::new().append(true).open(&path).unwrap();
            writer.write_all(&[9; 4096]).unwrap();
            writer.sync_all().unwrap();
            let (bytes, has_extra) = read_file_after_size(file, admitted).unwrap();
            assert!(has_extra);
            assert_eq!(bytes, vec![7; original]);
            assert_eq!(bytes.capacity(), original);
        }
    }

    #[test]
    fn shrink_after_metadata_returns_short_bytes_for_integrity_rejection() {
        let path = retained_file();
        fs::write(&path, vec![7; 1024]).unwrap();
        let file = open_nonsymlink_read(&path).unwrap();
        let admitted = file.metadata().unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(3)
            .unwrap();
        let (bytes, has_extra) = read_file_after_size(file, admitted).unwrap();
        assert!(!has_extra);
        assert_eq!(bytes, [7; 3]);
        assert_ne!(bytes.len() as u64, admitted);
    }
}

pub(crate) fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = open_nonsymlink_read(path)?;
    let actual_bytes = file.metadata()?.len();
    if actual_bytes > limit {
        return Err(StoreError::Invalid("file exceeds read budget".into()));
    }
    let (result, has_extra) = read_file_after_size(file, actual_bytes)?;
    if has_extra {
        return Err(StoreError::Invalid("file exceeds read budget".into()));
    }
    Ok(result)
}

pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn atomic_projection(root: &Path, manifest: &BundleManifest) -> Result<()> {
    let path = root.join("manifest.json");
    if path.symlink_metadata().is_ok() {
        regular(&path, false)?;
    }
    let mut pending = tempfile::NamedTempFile::new_in(root)?;
    pending.write_all(&manifest.encode()?)?;
    pending.as_file().sync_all()?;
    pending
        .persist(&path)
        .map_err(|e| StoreError::Io(e.error))?;
    sync_directory(root)
}

fn configure_writable(connection: &Connection) -> Result<()> {
    connection.execute_batch("PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;")?;
    Ok(())
}

fn database_size(root: &Path) -> Result<()> {
    const SIDECAR_SUFFIXES: [&str; 3] = ["-wal", "-journal", "-shm"];
    let database = root.join("project.sqlite");
    regular(&database, false)?;
    let mut lengths = vec![fs::metadata(&database)?.len()];
    for suffix in SIDECAR_SUFFIXES {
        let sidecar = root.join(format!("project.sqlite{suffix}"));
        let metadata = match fs::symlink_metadata(&sidecar) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StoreError::Invalid(format!(
                "expected nonsymlink SQLite sidecar file: {}",
                sidecar.display()
            )));
        }
        lengths.push(metadata.len());
    }
    let total = checked_database_total(lengths)?;
    if total > sqlite_guard::MAX_DATABASE_BYTES {
        return Err(StoreError::Invalid(
            "metadata database and SQLite sidecars exceed 64 MiB read budget".into(),
        ));
    }
    Ok(())
}

fn checked_database_total(lengths: impl IntoIterator<Item = u64>) -> Result<u64> {
    lengths.into_iter().try_fold(0_u64, |total, length| {
        total
            .checked_add(length)
            .ok_or_else(|| StoreError::Invalid("metadata database size overflow".into()))
    })
}

pub(crate) fn load_manifest(connection: &Connection) -> Result<BundleManifest> {
    sqlite_guard::validate_schema(connection)?;
    let mut statement =
        connection.prepare("SELECT singleton,revision,body FROM main.bundle_manifest LIMIT 2")?;
    let mut rows = statement.query([])?;
    let row = rows
        .next()?
        .ok_or_else(|| StoreError::Corrupt("missing singleton manifest".into()))?;
    let singleton: i64 = row.get(0)?;
    let revision: i64 = row.get(1)?;
    let bytes: Vec<u8> = row.get(2)?;
    if singleton != 1 || rows.next()?.is_some() {
        return Err(StoreError::Corrupt(
            "exactly one singleton manifest required".into(),
        ));
    }
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(StoreError::Corrupt("oversized database manifest".into()));
    }
    let manifest: BundleManifest = serde_json::from_slice(&bytes)?;
    manifest.validate()?;
    if i64::try_from(manifest.revision).ok() != Some(revision) {
        return Err(StoreError::Corrupt("manifest revision mismatch".into()));
    }
    let database_version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if manifest.schema_version != database_version {
        return Err(StoreError::Corrupt(
            "database schema version differs from manifest".into(),
        ));
    }
    Ok(manifest)
}

/// Read the manifest envelope without consulting SQLite's `user_version`.
/// Writable opens use this before an additive migration so a future logical
/// manifest cannot be changed by a migration intended for the current schema.
/// The caller performs the physical-version comparison after the compatibility
/// decision, while this function still validates the complete current envelope
/// and its revision binding.
fn preflight_manifest(connection: &Connection) -> Result<BundleManifest> {
    sqlite_guard::validate_schema(connection)?;
    let mut statement =
        connection.prepare("SELECT singleton,revision,body FROM main.bundle_manifest LIMIT 2")?;
    let mut rows = statement.query([])?;
    let row = rows
        .next()?
        .ok_or_else(|| StoreError::Corrupt("missing singleton manifest".into()))?;
    let singleton: i64 = row.get(0)?;
    let revision: i64 = row.get(1)?;
    let bytes: Vec<u8> = row.get(2)?;
    if singleton != 1 || rows.next()?.is_some() {
        return Err(StoreError::Corrupt(
            "exactly one singleton manifest required".into(),
        ));
    }
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(StoreError::Corrupt("oversized database manifest".into()));
    }
    let manifest: BundleManifest = serde_json::from_slice(&bytes)?;
    manifest.validate()?;
    if i64::try_from(manifest.revision).ok() != Some(revision) {
        return Err(StoreError::Corrupt("manifest revision mismatch".into()));
    }
    Ok(manifest)
}

impl Bundle {
    /// Creation reserves a new directory and never overwrites an existing path.
    /// A failed creation retains partial files for explicit diagnosis/recovery.
    pub fn create(root: &Path, project_id: ProjectId, name: String, utc_ms: i64) -> Result<Self> {
        let manifest = BundleManifest {
            schema_version: SCHEMA_VERSION,
            project_id,
            name,
            revision: 0,
            created_utc_ms: utc_ms,
            updated_utc_ms: utc_ms,
            required_features: vec![],
            artifacts: BTreeMap::new(),
        };
        manifest.validate()?;
        fs::create_dir(root)?;
        fs::create_dir(root.join("artifacts"))?;
        let mut connection = Connection::open(root.join("project.sqlite"))?;
        sqlite_guard::initialize(&connection)?;
        configure_writable(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(sqlite_guard::CREATE_MANIFEST)?;
        transaction.execute_batch(sqlite_guard::CREATE_SURVEY_SNAPSHOTS)?;
        transaction.execute_batch(sqlite_guard::CREATE_SURVEY_SNAPSHOT_HISTORY)?;
        transaction.execute_batch(sqlite_guard::CREATE_OPERATION_LOG_STATE)?;
        transaction.execute_batch(sqlite_guard::CREATE_OPERATIONS)?;
        transaction.execute_batch(sqlite_guard::CREATE_OBSERVATION_CHUNKS)?;
        transaction.execute_batch(sqlite_guard::CREATE_OBSERVATION_CHUNK_MEMBERS)?;
        transaction.execute_batch(sqlite_guard::CREATE_CAPTURE_PUBLICATIONS)?;
        transaction.execute_batch(sqlite_guard::CREATE_MATERIALIZATION_BASELINES)?;
        transaction.execute_batch(sqlite_guard::CREATE_MATERIALIZED_PROJECT_PUBLICATIONS)?;
        transaction.execute_batch(sqlite_guard::CREATE_MATERIALIZED_PROJECT_STATE)?;
        transaction.execute_batch("PRAGMA user_version=1")?;
        transaction.execute(
            "INSERT INTO bundle_manifest VALUES (1, ?1, ?2)",
            (0, manifest.encode()?),
        )?;
        transaction.execute(
            "INSERT INTO operation_log_state (singleton,project_id,project_revision) VALUES (1,?1,0)",
            [String::from(project_id)],
        )?;
        transaction.commit()?;
        sqlite_guard::restrict(&connection, true)?;
        atomic_projection(root, &manifest)?;
        sync_directory(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            connection,
            mode: OpenMode::ReadWrite,
        })
    }

    pub fn open(root: &Path, mode: OpenMode) -> Result<Self> {
        regular(root, true)?;
        database_size(root)?;
        regular(&root.join("artifacts"), true)?;
        let flags = match mode {
            OpenMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            OpenMode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
        };
        let preflight = if mode == OpenMode::ReadWrite {
            // Probe through a read-only handle before opening SQLite in a
            // write-capable mode. SQLite may recover a hot rollback journal as
            // part of a writable open, so future compatibility is rejected
            // without permitting migration or recovery writes.
            let probe = Connection::open_with_flags(
                root.join("project.sqlite"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            // Apply the same defensive limits and VM/time budget before the
            // first schema or manifest query. Initialization only changes
            // connection-local SQLite configuration; it never migrates or
            // writes bundle metadata.
            sqlite_guard::initialize(&probe)?;
            sqlite_guard::validate_schema(&probe)?;
            let preflight = preflight_manifest(&probe)?;
            if preflight.schema_version != SCHEMA_VERSION || !preflight.required_features.is_empty()
            {
                return Err(StoreError::UnsupportedVersion(preflight.schema_version));
            }
            let database_version: u32 =
                probe.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            if preflight.schema_version != database_version {
                return Err(StoreError::Corrupt(
                    "database schema version differs from manifest".into(),
                ));
            }
            Some(preflight)
        } else {
            None
        };
        let mut connection = Connection::open_with_flags(root.join("project.sqlite"), flags)?;
        sqlite_guard::initialize(&connection)?;
        sqlite_guard::validate_schema(&connection)?;
        if mode == OpenMode::ReadWrite
            && (!sqlite_guard::has_survey_snapshot_schema(&connection)?
                || !sqlite_guard::has_operation_schema(&connection)?
                || !sqlite_guard::has_observation_chunk_schema(&connection)?
                || !sqlite_guard::has_capture_publication_schema(&connection)?
                || !sqlite_guard::has_materialized_project_schema(&connection)?)
        {
            // V1 bundles predate one or more optional metadata table groups.
            // This additive migration is performed before the authorizer is
            // installed and never rewrites the manifest or its revision.
            let migration = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if !sqlite_guard::has_survey_snapshot_schema(&migration)? {
                migration.execute_batch(sqlite_guard::CREATE_SURVEY_SNAPSHOTS)?;
                migration.execute_batch(sqlite_guard::CREATE_SURVEY_SNAPSHOT_HISTORY)?;
            }
            if !sqlite_guard::has_operation_schema(&migration)? {
                migration.execute_batch(sqlite_guard::CREATE_OPERATION_LOG_STATE)?;
                migration.execute_batch(sqlite_guard::CREATE_OPERATIONS)?;
                migration.execute(
                    "INSERT INTO operation_log_state (singleton,project_id,project_revision) VALUES (1,?1,0)",
                    [String::from(
                        preflight
                            .as_ref()
                            .ok_or_else(|| StoreError::Corrupt("missing migration preflight".into()))?
                            .project_id,
                    )],
                )?;
            }
            if !sqlite_guard::has_observation_chunk_schema(&migration)? {
                migration.execute_batch(sqlite_guard::CREATE_OBSERVATION_CHUNKS)?;
                migration.execute_batch(sqlite_guard::CREATE_OBSERVATION_CHUNK_MEMBERS)?;
            }
            if !sqlite_guard::has_capture_publication_schema(&migration)? {
                migration.execute_batch(sqlite_guard::CREATE_CAPTURE_PUBLICATIONS)?;
            }
            if !sqlite_guard::has_materialized_project_schema(&migration)? {
                let tables = migration
                    .prepare("SELECT name FROM main.sqlite_schema WHERE type='table' AND name IN ('materialization_baselines','materialized_project_publications','materialized_project_state')")?
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if !tables
                    .iter()
                    .any(|name| name == "materialization_baselines")
                {
                    migration.execute_batch(sqlite_guard::CREATE_MATERIALIZATION_BASELINES)?;
                }
                if !tables
                    .iter()
                    .any(|name| name == "materialized_project_publications")
                {
                    migration
                        .execute_batch(sqlite_guard::CREATE_MATERIALIZED_PROJECT_PUBLICATIONS)?;
                }
                if !tables
                    .iter()
                    .any(|name| name == "materialized_project_state")
                {
                    migration.execute_batch(sqlite_guard::CREATE_MATERIALIZED_PROJECT_STATE)?;
                }
            }
            migration.commit()?;
        }
        sqlite_guard::restrict(&connection, mode == OpenMode::ReadWrite)?;
        let bundle = Self {
            root: root.to_path_buf(),
            connection,
            mode,
        };
        let manifest = bundle.manifest()?;
        if mode == OpenMode::ReadWrite
            && (manifest.schema_version != SCHEMA_VERSION || !manifest.required_features.is_empty())
        {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        if mode == OpenMode::ReadWrite {
            configure_writable(&bundle.connection)?;
        }
        Ok(bundle)
    }

    pub(crate) fn start_operation(&self) -> Result<()> {
        database_size(&self.root)?;
        sqlite_guard::start_operation(&self.connection)
    }

    /// Publish an immutable content-addressed artifact file before its
    /// SQLite reference is committed. A duplicate hash is accepted only when
    /// its bytes match exactly; an unreferenced file after a failed transaction
    /// is harmless and is retained for explicit garbage collection.
    pub(crate) fn write_artifact_file(&self, bytes: &[u8]) -> Result<String> {
        if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
            return Err(StoreError::Invalid(
                "artifact exceeds 64 MiB chunk limit".into(),
            ));
        }
        regular(&self.root.join("artifacts"), true)?;
        let hash = content_hash(bytes);
        let path = self.root.join("artifacts").join(&hash);
        if path.symlink_metadata().is_ok() {
            if bounded_read(&path, MAX_ARTIFACT_BYTES)? != bytes {
                return Err(StoreError::Corrupt(
                    "existing content hash has different bytes".into(),
                ));
            }
            return Ok(hash);
        }
        let mut pending = tempfile::NamedTempFile::new_in(self.root.join("artifacts"))?;
        pending.write_all(bytes)?;
        pending.as_file().sync_all()?;
        match pending.persist_noclobber(&path) {
            Ok(_) => (),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                if bounded_read(&path, MAX_ARTIFACT_BYTES)? != bytes {
                    return Err(StoreError::Corrupt("concurrent artifact collision".into()));
                }
            }
            Err(error) => return Err(StoreError::Io(error.error)),
        }
        sync_directory(&self.root.join("artifacts"))?;
        Ok(hash)
    }

    pub fn manifest(&self) -> Result<BundleManifest> {
        self.start_operation()?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        transaction.commit()?;
        Ok(manifest)
    }

    /// Imports an immutable artifact then registers it transactionally. A crash
    /// before commit can leave an unreferenced blob, but never a committed missing blob.
    pub fn put_artifact(
        &mut self,
        bytes: &[u8],
        entry: ArtifactEntry,
        utc_ms: i64,
    ) -> Result<String> {
        if self.mode == OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        if matches!(entry.kind, ArtifactKind::NormalizedObservations) {
            return Err(StoreError::Invalid(
                "normalized observations require publish_observation_chunk".into(),
            ));
        }
        if matches!(
            entry.kind,
            ArtifactKind::MaterializationBaseline | ArtifactKind::MaterializedProject
        ) {
            return Err(StoreError::Invalid(
                "canonical evidence requires its validated publication API".into(),
            ));
        }
        self.start_operation()?;
        entry.validate()?;
        if bytes.len() as u64 != entry.bytes {
            return Err(StoreError::Invalid(
                "artifact length differs from declaration".into(),
            ));
        }
        regular(&self.root.join("artifacts"), true)?;
        let hash = content_hash(bytes);
        let path = self.root.join("artifacts").join(&hash);
        if path.symlink_metadata().is_ok() {
            if bounded_read(&path, MAX_ARTIFACT_BYTES)? != bytes {
                return Err(StoreError::Corrupt(
                    "existing content hash has different bytes".into(),
                ));
            }
        } else {
            let mut pending = tempfile::NamedTempFile::new_in(self.root.join("artifacts"))?;
            pending.write_all(bytes)?;
            pending.as_file().sync_all()?;
            match pending.persist_noclobber(&path) {
                Ok(_) => (),
                Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if bounded_read(&path, MAX_ARTIFACT_BYTES)? != bytes {
                        return Err(StoreError::Corrupt("concurrent artifact collision".into()));
                    }
                }
                Err(e) => return Err(StoreError::Io(e.error)),
            }
            sync_directory(&self.root.join("artifacts"))?;
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut manifest = load_manifest(&transaction)?;
        if manifest.schema_version != SCHEMA_VERSION || !manifest.required_features.is_empty() {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        if utc_ms < manifest.updated_utc_ms {
            return Err(StoreError::Invalid(
                "update wall time precedes committed manifest; clock correction required".into(),
            ));
        }
        if let Some(existing) = manifest.artifacts.get(&hash) {
            if existing != &entry {
                return Err(StoreError::Invalid(
                    "same bytes already registered with different semantics/provenance".into(),
                ));
            }
        } else {
            manifest.artifacts.insert(hash.clone(), entry);
            manifest.revision = manifest
                .revision
                .checked_add(1)
                .ok_or_else(|| StoreError::Invalid("revision exhausted".into()))?;
            manifest.updated_utc_ms = utc_ms;
        }
        let encoded = manifest.encode()?;
        let revision = i64::try_from(manifest.revision)
            .map_err(|_| StoreError::Invalid("revision exhausted".into()))?;
        let changed = transaction.execute(
            "UPDATE bundle_manifest SET revision=?1, body=?2 WHERE singleton=1",
            (revision, &encoded),
        )?;
        if changed != 1 {
            return Err(StoreError::Corrupt(
                "manifest update did not affect exactly one row".into(),
            ));
        }
        // Check the actual authoritative values before publishing the projection.
        // The supported schema forbids triggers; readback also protects this
        // invariant if a later reviewed migration changes that schema policy.
        let committed = load_manifest(&transaction)?;
        let stored: Vec<u8> = transaction.query_row(
            "SELECT body FROM bundle_manifest WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if committed != manifest || stored != encoded {
            return Err(StoreError::Corrupt(
                "manifest update failed authoritative readback".into(),
            ));
        }
        // Serialize projection publication with all other writers. If publishing
        // fails, the transaction rolls back. A crash after publication but before
        // commit leaves a detectable stale projection, not ambiguous committed input.
        atomic_projection(&self.root, &manifest)?;
        transaction.commit()?;
        Ok(hash)
    }

    pub fn read_artifact(&self, hash: &str) -> Result<Vec<u8>> {
        validate_hash(hash)?;
        let manifest = self.manifest()?;
        let entry = manifest
            .artifacts
            .get(hash)
            .ok_or_else(|| StoreError::Invalid("unregistered artifact".into()))?;
        self.read_registered_artifact(hash, entry)
    }

    pub(crate) fn read_registered_artifact(
        &self,
        hash: &str,
        entry: &ArtifactEntry,
    ) -> Result<Vec<u8>> {
        regular(&self.root.join("artifacts"), true)?;
        let path = self.root.join("artifacts").join(hash);
        let file = open_nonsymlink_read(&path)?;
        let actual_bytes = file.metadata()?.len();
        if actual_bytes > MAX_ARTIFACT_BYTES {
            return Err(StoreError::Invalid("file exceeds read budget".into()));
        }
        if actual_bytes != entry.bytes {
            return Err(StoreError::Corrupt(format!(
                "artifact checksum/length mismatch: {hash}"
            )));
        }
        let (bytes, has_extra) = read_file_after_size(file, entry.bytes)?;
        if has_extra || bytes.len() as u64 != entry.bytes || content_hash(&bytes) != hash {
            return Err(StoreError::Corrupt(format!(
                "artifact checksum/length mismatch: {hash}"
            )));
        }
        Ok(bytes)
    }

    /// Explicitly repairs only the redundant JSON projection from committed SQLite.
    pub fn recover_manifest(&self) -> Result<()> {
        if self.mode == OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        self.start_operation()?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let manifest = load_manifest(&transaction)?;
        if manifest.schema_version != SCHEMA_VERSION || !manifest.required_features.is_empty() {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        atomic_projection(&self.root, &manifest)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn verify(&self) -> Result<Verification> {
        self.start_operation()?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        let integrity: String = self
            .connection
            .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        let mut failures = Vec::new();
        if integrity != "ok" {
            failures.push(format!("SQLite: {integrity}"));
        }
        let projection_current = bounded_read(&self.root.join("manifest.json"), MAX_MANIFEST_BYTES)
            .and_then(|bytes| Ok(serde_json::from_slice::<BundleManifest>(&bytes)?))
            .is_ok_and(|projection| projection == manifest);
        if !projection_current {
            failures.push("manifest.json projection is stale/missing/corrupt; recover-manifest can rebuild it".into());
        }
        for (hash, entry) in &manifest.artifacts {
            if let Err(error) = self.read_registered_artifact(hash, entry) {
                failures.push(error.to_string());
            }
        }
        transaction.commit()?;
        if sqlite_guard::has_survey_snapshot_schema(&self.connection)? {
            let transaction = rusqlite::Transaction::new_unchecked(
                &self.connection,
                TransactionBehavior::Deferred,
            )?;
            let inventory_limit = crate::survey_snapshot::MAX_SURVEY_SNAPSHOTS as i64 + 1;
            let mut statement =
                transaction.prepare("SELECT snapshot_id FROM survey_snapshots LIMIT ?1")?;
            let mut rows = statement.query([inventory_limit])?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                ids.push(row.get::<_, String>(0)?);
                if ids.len() as i64 >= inventory_limit {
                    failures.push("survey snapshot inventory exceeds resource limit".into());
                    break;
                }
            }
            drop(rows);
            drop(statement);
            transaction.commit()?;
            for raw_id in ids {
                if SnapshotId::try_from(raw_id).is_err() {
                    failures.push("invalid snapshot_id in survey index".into());
                }
            }
            if let Err(error) = self.list_survey_snapshot_history(None) {
                failures.push(error.to_string());
            }
        }
        if sqlite_guard::has_operation_schema(&self.connection)?
            && let Err(error) = self.operation_store_state()
        {
            failures.push(error.to_string());
        }
        if let Err(error) = self.verify_observation_chunks() {
            failures.push(error.to_string());
        }
        if sqlite_guard::has_materialized_project_schema(&self.connection)?
            && let Err(error) = self.verify_materialized_project_publication()
        {
            failures.push(error.to_string());
        }
        Ok(Verification {
            schema_version: manifest.schema_version,
            artifact_count: manifest.artifacts.len(),
            artifact_bytes: manifest.artifacts.values().map(|a| a.bytes).sum(),
            projection_current,
            failures,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArtifactKind;
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

    #[test]
    fn database_and_sidecars_share_an_exact_checked_budget() {
        let retained = tempfile::tempdir().unwrap().keep();
        let database = retained.join("project.sqlite");
        let wal = retained.join("project.sqlite-wal");
        let half = sqlite_guard::MAX_DATABASE_BYTES / 2;
        File::create(&database).unwrap().set_len(half).unwrap();
        File::create(&wal).unwrap().set_len(half).unwrap();
        assert!(database_size(&retained).is_ok());

        File::options()
            .write(true)
            .open(&database)
            .unwrap()
            .set_len(half + 1)
            .unwrap();
        assert!(matches!(
            database_size(&retained),
            Err(StoreError::Invalid(message))
                if message == "metadata database and SQLite sidecars exceed 64 MiB read budget"
        ));
        assert!(matches!(
            checked_database_total([u64::MAX, 1]),
            Err(StoreError::Invalid(message)) if message == "metadata database size overflow"
        ));
    }

    #[test]
    fn successful_sql_row_count_is_not_a_substitute_for_authoritative_readback() {
        let retained = tempfile::tempdir().unwrap().keep();
        let root = retained.join("readback-fault");
        let mut bundle = Bundle::create(
            &root,
            ProjectId::from_bytes([1; 16]).unwrap(),
            "Readback".into(),
            1,
        )
        .unwrap();
        let before = bundle.manifest().unwrap();
        let projection = fs::read(root.join("manifest.json")).unwrap();
        // Independent fault injection: SQLite reports the matched row, but an
        // authorizer substitutes no-ops for column updates. This bypasses the
        // ordinary guard only in this private test and exercises readback itself.
        bundle
            .connection
            .authorizer(Some(|context: AuthContext<'_>| match context.action {
                AuthAction::Update {
                    table_name: "bundle_manifest",
                    ..
                } => Authorization::Ignore,
                _ => Authorization::Allow,
            }))
            .unwrap();
        let error = bundle
            .put_artifact(
                b"map",
                ArtifactEntry {
                    kind: ArtifactKind::MapSource,
                    bytes: 3,
                    media_type: "image/svg+xml".into(),
                    provenance_id: "test:readback-fault".into(),
                },
                2,
            )
            .unwrap_err();
        assert!(
            matches!(error, StoreError::Corrupt(message) if message == "manifest update failed authoritative readback")
        );
        assert_eq!(bundle.manifest().unwrap(), before);
        assert_eq!(fs::read(root.join("manifest.json")).unwrap(), projection);
    }
}
