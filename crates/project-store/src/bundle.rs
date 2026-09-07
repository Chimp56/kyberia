use crate::manifest::{MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES, SCHEMA_VERSION, validate_hash};
use crate::{ArtifactEntry, BundleManifest, Result, StoreError, content_hash};
use kyberia_domain::identity::ProjectId;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, limits::Limit};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    ReadOnly,
    ReadWrite,
}

pub struct Bundle {
    root: PathBuf,
    connection: Connection,
    mode: OpenMode,
}

#[derive(Debug, Serialize)]
pub struct Verification {
    pub schema_version: u32,
    pub artifact_count: usize,
    pub artifact_bytes: u64,
    pub projection_current: bool,
    pub failures: Vec<String>,
}

fn regular(path: &Path, directory: bool) -> Result<()> {
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

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    regular(path, false)?;
    let file = File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(StoreError::Invalid("file exceeds read budget".into()));
    }
    let mut result = Vec::new();
    file.take(limit + 1).read_to_end(&mut result)?;
    if result.len() as u64 > limit {
        return Err(StoreError::Invalid("file exceeds read budget".into()));
    }
    Ok(result)
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn atomic_projection(root: &Path, manifest: &BundleManifest) -> Result<()> {
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

fn configure(connection: &Connection, writable: bool) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(3))?;
    connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_MANIFEST_BYTES as i32)?;
    connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024)?;
    connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
    if writable {
        connection.execute_batch("PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;")?;
    }
    Ok(())
}

fn load_manifest(connection: &Connection) -> Result<BundleManifest> {
    let (revision, bytes): (i64, Vec<u8>) = connection.query_row(
        "SELECT revision, body FROM bundle_manifest WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
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
        configure(&connection, true)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch("CREATE TABLE bundle_manifest (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL CHECK(revision>=0), body BLOB NOT NULL); PRAGMA user_version=1;")?;
        transaction.execute(
            "INSERT INTO bundle_manifest VALUES (1, ?1, ?2)",
            (0, manifest.encode()?),
        )?;
        transaction.commit()?;
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
        regular(&root.join("project.sqlite"), false)?;
        regular(&root.join("artifacts"), true)?;
        let flags = match mode {
            OpenMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            OpenMode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
        };
        let connection = Connection::open_with_flags(root.join("project.sqlite"), flags)?;
        configure(&connection, false)?;
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
            configure(&bundle.connection, true)?;
        }
        Ok(bundle)
    }

    pub fn manifest(&self) -> Result<BundleManifest> {
        load_manifest(&self.connection)
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
        transaction.execute(
            "UPDATE bundle_manifest SET revision=?1, body=?2 WHERE singleton=1",
            (
                i64::try_from(manifest.revision)
                    .map_err(|_| StoreError::Invalid("revision exhausted".into()))?,
                manifest.encode()?,
            ),
        )?;
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

    fn read_registered_artifact(&self, hash: &str, entry: &ArtifactEntry) -> Result<Vec<u8>> {
        regular(&self.root.join("artifacts"), true)?;
        let bytes = bounded_read(&self.root.join("artifacts").join(hash), MAX_ARTIFACT_BYTES)?;
        if bytes.len() as u64 != entry.bytes || content_hash(&bytes) != hash {
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
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let manifest = self.manifest()?;
        if manifest.schema_version != SCHEMA_VERSION || !manifest.required_features.is_empty() {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        atomic_projection(&self.root, &manifest)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn verify(&self) -> Result<Verification> {
        let manifest = self.manifest()?;
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
        Ok(Verification {
            schema_version: manifest.schema_version,
            artifact_count: manifest.artifacts.len(),
            artifact_bytes: manifest.artifacts.values().map(|a| a.bytes).sum(),
            projection_current,
            failures,
        })
    }
}
