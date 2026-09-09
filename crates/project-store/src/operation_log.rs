//! SQLite persistence for the canonical immutable operation DAG.
//!
//! The operation bytes are the authority for operation meaning. SQLite stores
//! the exact wire bytes, the exact unsigned canonical bytes, and their digest
//! in separate columns so a reopen can detect any substitution before replay.
//! The operation-only project revision is a local linear commit counter; it is
//! deliberately separate from each operation's causal depth.

use crate::bundle::{atomic_projection, load_manifest};
use crate::{Bundle, Result, StoreError, sqlite_guard};
use kyberia_domain::identity::{OperationId, ProjectId};
use kyberia_operation_log::{
    AppliedEffect, AppliedMutation, MAX_OPERATION_CANONICAL_BYTES, MAX_OPERATION_COUNT,
    MAX_OPERATION_WIRE_BYTES, MergeError, Operation, OperationError, OperationSet, ProjectVersion,
};
use rusqlite::{OptionalExtension, Row, Transaction, TransactionBehavior, params};

const OPERATION_ROW_LIMIT: i64 = MAX_OPERATION_COUNT as i64 + 1;

/// The local operation revision and count persisted by the operation adapter.
/// `project_revision` is incremented once for each accepted unique operation;
/// it is not derived from or interchangeable with [`CausalDepth`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationStoreState {
    project_id: ProjectId,
    project_revision: ProjectVersion,
    operation_count: usize,
}

impl OperationStoreState {
    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }

    pub const fn project_revision(self) -> ProjectVersion {
        self.project_revision
    }

    pub const fn operation_count(self) -> usize {
        self.operation_count
    }
}

/// The result of one transactional operation append. Bundle revision is the
/// overall metadata commit counter; project revision counts operation rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationAppendOutcome {
    Appended {
        project_revision: ProjectVersion,
        bundle_revision: u64,
    },
    Duplicate {
        project_revision: ProjectVersion,
        bundle_revision: u64,
    },
}

struct RawOperationRow {
    operation_id: String,
    project_id: String,
    project_revision: i64,
    logical_time: i64,
    causal_depth: i64,
    content_hash: String,
    canonical_bytes: Vec<u8>,
    wire_bytes: Vec<u8>,
}

struct RawOperationMetadata {
    operation_id: String,
    canonical_length: Option<i64>,
    wire_length: Option<i64>,
}

impl RawOperationMetadata {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            operation_id: row.get(0)?,
            canonical_length: row.get(1)?,
            wire_length: row.get(2)?,
        })
    }

    fn validate_blob_lengths(&self) -> Result<()> {
        checked_blob_length(
            self.canonical_length,
            MAX_OPERATION_CANONICAL_BYTES,
            "canonical",
        )?;
        checked_blob_length(self.wire_length, MAX_OPERATION_WIRE_BYTES, "wire")?;
        Ok(())
    }
}

impl RawOperationRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            operation_id: row.get(0)?,
            project_id: row.get(1)?,
            project_revision: row.get(2)?,
            logical_time: row.get(3)?,
            causal_depth: row.get(4)?,
            content_hash: row.get(5)?,
            canonical_bytes: row.get(6)?,
            wire_bytes: row.get(7)?,
        })
    }

    fn operation(self, project_id: ProjectId) -> Result<StoredOperation> {
        if self.canonical_bytes.len() > MAX_OPERATION_CANONICAL_BYTES {
            return Err(StoreError::Corrupt(
                "operation canonical bytes exceed read budget".into(),
            ));
        }
        if self.wire_bytes.len() > MAX_OPERATION_WIRE_BYTES {
            return Err(StoreError::Corrupt(
                "operation wire bytes exceed read budget".into(),
            ));
        }
        let operation = Operation::from_bytes(&self.wire_bytes).map_err(|error| {
            StoreError::Corrupt(format!("operation row decode failed: {error}"))
        })?;
        let operation_id = parse_id::<OperationId>(self.operation_id, "operation_id")?;
        let row_project_id = parse_id::<ProjectId>(self.project_id, "project_id")?;
        let project_revision = positive_i64(self.project_revision, "operation project revision")?;
        let logical_time = positive_i64(self.logical_time, "operation logical time")?;
        let causal_depth = nonnegative_i64(self.causal_depth, "operation causal depth")?;
        let expected_hash = String::from(operation.content_hash());
        if row_project_id != project_id
            || operation.project_id() != project_id
            || operation.operation_id() != operation_id
            || operation.logical_time().value() != logical_time
            || operation.causal_depth().value() != causal_depth
            || self.content_hash != expected_hash
            || operation.canonical_bytes() != self.canonical_bytes
        {
            return Err(StoreError::Corrupt(
                "operation row columns do not match canonical operation bytes".into(),
            ));
        }
        Ok(StoredOperation {
            operation,
            project_revision,
        })
    }
}

struct StoredOperation {
    operation: Operation,
    project_revision: u64,
}

fn parse_id<T>(raw: String, field: &str) -> Result<T>
where
    T: TryFrom<String>,
{
    T::try_from(raw).map_err(|_| StoreError::Corrupt(format!("invalid {field} in operation row")))
}

fn positive_i64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| StoreError::Corrupt(format!("invalid {field} in operation row")))
}

fn nonnegative_i64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| StoreError::Corrupt(format!("invalid {field} in operation row")))
}

fn checked_blob_length(length: Option<i64>, maximum: usize, name: &str) -> Result<usize> {
    let length = length.ok_or_else(|| {
        StoreError::Corrupt(format!("operation {name} bytes have no SQLite length"))
    })?;
    let length = usize::try_from(length)
        .map_err(|_| StoreError::Corrupt(format!("operation {name} bytes have invalid length")))?;
    if length > maximum {
        return Err(StoreError::Corrupt(format!(
            "operation {name} bytes exceed read budget"
        )));
    }
    Ok(length)
}

fn operation_error(error: OperationError) -> StoreError {
    StoreError::Operation(error.to_string())
}

fn validate_replay_admission(set: &OperationSet) -> std::result::Result<(), MergeError> {
    // Admission must validate toggle state independently. `OperationSet::replay`
    // intentionally reports semantic field conflicts before applying any
    // mutations, so accepting its `Conflicts` error could mask a sequential
    // repeated undo/redo and persist a set that cannot be replayed later.
    set.validate_replay_semantics()
}

fn ensure_schema(bundle: &Bundle) -> Result<()> {
    if !sqlite_guard::has_operation_schema(&bundle.connection)? {
        return Err(StoreError::UnsupportedVersion(1));
    }
    Ok(())
}

fn read_state(transaction: &Transaction<'_>, project_id: ProjectId) -> Result<OperationStoreState> {
    let mut statement = transaction
        .prepare("SELECT singleton,project_id,project_revision FROM operation_log_state LIMIT 2")?;
    let mut rows = statement.query([])?;
    let Some(row) = rows.next()? else {
        return Err(StoreError::Corrupt(
            "missing operation log state singleton".into(),
        ));
    };
    let singleton: i64 = row.get(0)?;
    let stored_project = parse_id::<ProjectId>(row.get(1)?, "project_id")?;
    let raw_revision: i64 = row.get(2)?;
    if singleton != 1 || rows.next()?.is_some() {
        return Err(StoreError::Corrupt(
            "operation log state requires exactly one singleton row".into(),
        ));
    }
    if stored_project != project_id {
        return Err(StoreError::Corrupt(
            "operation log state belongs to another project".into(),
        ));
    }
    let revision = u64::try_from(raw_revision)
        .map_err(|_| StoreError::Corrupt("negative operation project revision".into()))?;
    if revision > MAX_OPERATION_COUNT as u64 {
        return Err(StoreError::Corrupt(
            "operation project revision exceeds resource limit".into(),
        ));
    }
    Ok(OperationStoreState {
        project_id,
        project_revision: ProjectVersion::new(revision),
        operation_count: 0,
    })
}

fn read_rows(transaction: &Transaction<'_>, project_id: ProjectId) -> Result<Vec<StoredOperation>> {
    // Check SQLite's BLOB lengths in a scalar-only query before selecting the
    // BLOB values. A hostile row therefore fails its resource budget without
    // materializing a multi-megabyte value in Rust.
    let mut statement = transaction.prepare(
        "SELECT operation_id,length(canonical_bytes),length(wire_bytes) FROM project_operations ORDER BY project_revision,operation_id LIMIT ?1",
    )?;
    let mut rows = statement.query([OPERATION_ROW_LIMIT])?;
    let mut metadata = Vec::new();
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let row = RawOperationMetadata::from_row(row)?;
        row.validate_blob_lengths()?;
        metadata.push(row);
    }
    drop(rows);
    drop(statement);
    if metadata.len() as i64 >= OPERATION_ROW_LIMIT {
        return Err(StoreError::Corrupt(
            "operation inventory exceeds resource limit".into(),
        ));
    }
    for row in metadata {
        let operation_id = row.operation_id.clone();
        let loaded = transaction.query_row(
            "SELECT operation_id,project_id,project_revision,logical_time,causal_depth,content_hash,canonical_bytes,wire_bytes FROM project_operations WHERE operation_id=?1",
            [operation_id],
            RawOperationRow::from_row,
        )?;
        if loaded.canonical_bytes.len()
            != row.canonical_length.ok_or_else(|| {
                StoreError::Corrupt("operation canonical bytes have no SQLite length".into())
            })? as usize
            || loaded.wire_bytes.len()
                != row.wire_length.ok_or_else(|| {
                    StoreError::Corrupt("operation wire bytes have no SQLite length".into())
                })? as usize
        {
            return Err(StoreError::Corrupt(
                "operation BLOB length changed during read".into(),
            ));
        }
        result.push(loaded.operation(project_id)?);
    }
    Ok(result)
}

fn validate_inventory(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
) -> Result<(OperationStoreState, Vec<Operation>)> {
    let mut state = read_state(transaction, project_id)?;
    let rows = read_rows(transaction, project_id)?;
    if rows.len() != state.project_revision.value() as usize {
        return Err(StoreError::Corrupt(
            "operation row count does not match operation project revision".into(),
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        if row.project_revision != (index + 1) as u64 {
            return Err(StoreError::Corrupt(
                "operation project revisions are not contiguous".into(),
            ));
        }
    }
    let operations = rows
        .into_iter()
        .map(|row| row.operation)
        .collect::<Vec<_>>();
    if !operations.is_empty() {
        let set = OperationSet::from_operations(operations.clone()).map_err(|error| {
            StoreError::Corrupt(format!("stored operation graph is invalid: {error}"))
        })?;
        validate_replay_admission(&set).map_err(|error| {
            StoreError::Corrupt(format!("stored operation replay is invalid: {error}"))
        })?;
    }
    state.operation_count = operations.len();
    Ok((state, operations))
}

fn next_bundle_revision(current: u64) -> Result<(u64, i64)> {
    let next = current
        .checked_add(1)
        .ok_or_else(|| StoreError::Invalid("bundle revision exhausted".into()))?;
    let sql_value =
        i64::try_from(next).map_err(|_| StoreError::Invalid("bundle revision exhausted".into()))?;
    Ok((next, sql_value))
}

struct OperationColumns {
    operation_id: String,
    operation_hash: String,
    logical_time: i64,
    causal_depth: i64,
    canonical: Vec<u8>,
    wire: Vec<u8>,
}

fn operation_columns(operation: &Operation) -> Result<OperationColumns> {
    let canonical = operation.canonical_bytes().to_vec();
    if canonical.len() > MAX_OPERATION_CANONICAL_BYTES {
        return Err(StoreError::Operation(
            "operation canonical bytes exceed resource limit".into(),
        ));
    }
    let wire = operation.to_bytes().map_err(operation_error)?;
    if wire.len() > MAX_OPERATION_WIRE_BYTES {
        return Err(StoreError::Operation(
            "operation wire bytes exceed resource limit".into(),
        ));
    }
    let operation_hash = String::from(operation.content_hash());
    if crate::content_hash(&canonical) != operation_hash {
        return Err(StoreError::Corrupt(
            "operation content hash does not match canonical bytes".into(),
        ));
    }
    let logical_time = i64::try_from(operation.logical_time().value())
        .map_err(|_| StoreError::Operation("logical timestamp exceeds storage range".into()))?;
    let causal_depth = i64::try_from(operation.causal_depth().value())
        .map_err(|_| StoreError::Operation("causal depth exceeds storage range".into()))?;
    Ok(OperationColumns {
        operation_id: String::from(operation.operation_id()),
        operation_hash,
        logical_time,
        causal_depth,
        canonical,
        wire,
    })
}

fn row_for_id(
    transaction: &Transaction<'_>,
    id: OperationId,
    project_id: ProjectId,
) -> Result<Option<StoredOperation>> {
    let id = String::from(id);
    let metadata = transaction
        .query_row(
            "SELECT operation_id,length(canonical_bytes),length(wire_bytes) FROM project_operations WHERE operation_id=?1",
            [&id],
            RawOperationMetadata::from_row,
        )
        .optional()?;
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let canonical_length = checked_blob_length(
        metadata.canonical_length,
        MAX_OPERATION_CANONICAL_BYTES,
        "canonical",
    )?;
    let wire_length = checked_blob_length(metadata.wire_length, MAX_OPERATION_WIRE_BYTES, "wire")?;
    let row = transaction.query_row(
        "SELECT operation_id,project_id,project_revision,logical_time,causal_depth,content_hash,canonical_bytes,wire_bytes FROM project_operations WHERE operation_id=?1",
        [&id],
        RawOperationRow::from_row,
    )?;
    if row.canonical_bytes.len() != canonical_length || row.wire_bytes.len() != wire_length {
        return Err(StoreError::Corrupt(
            "operation BLOB length changed during read".into(),
        ));
    }
    row.operation(project_id).map(Some)
}

impl Bundle {
    /// Read and validate the operation-only project revision. It is a local
    /// linear counter and has no relationship to operation causal depth.
    pub fn operation_store_state(&self) -> Result<OperationStoreState> {
        ensure_schema(self)?;
        self.start_operation()?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        let (state, _) = validate_inventory(&transaction, manifest.project_id)?;
        transaction.commit()?;
        Ok(state)
    }

    /// Return the validated immutable operation set in canonical DAG form.
    pub fn operation_set(&self) -> Result<OperationSet> {
        ensure_schema(self)?;
        self.start_operation()?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        let (_, operations) = validate_inventory(&transaction, manifest.project_id)?;
        let result = if operations.is_empty() {
            Ok(OperationSet::empty(manifest.project_id))
        } else {
            OperationSet::from_operations(operations).map_err(operation_error)
        };
        transaction.commit()?;
        result
    }

    /// Deterministic topological replay from the persisted immutable set.
    pub fn replay_operations(&self) -> Result<Vec<AppliedMutation>> {
        self.operation_set()?
            .replay()
            .map_err(|error| StoreError::Operation(error.to_string()))
    }

    /// Deterministic topological replay from the persisted immutable set,
    /// retaining V2 typed effects such as an explicit unknown calibration
    /// state. This is the outward storage boundary for callers that cannot
    /// safely coerce every operation into the legacy mutation-only API.
    pub fn replay_operation_effects(&self) -> Result<Vec<AppliedEffect>> {
        self.operation_set()?
            .replay_effects()
            .map_err(|error| StoreError::Operation(error.to_string()))
    }

    /// Append one immutable operation atomically. Exact retries are idempotent
    /// and do not advance either revision; an ID with different bytes is a
    /// tamper error. The operation DAG is validated before any write.
    pub fn append_operation(&mut self, operation: Operation) -> Result<OperationAppendOutcome> {
        self.append_operation_if_revision(operation, None)
    }

    /// Append with an optional optimistic project-revision precondition. A
    /// mismatched precondition is a deterministic cancellation point before
    /// any operation, state, manifest, or projection write occurs.
    pub fn append_operation_if_revision(
        &mut self,
        operation: Operation,
        expected_project_revision: Option<ProjectVersion>,
    ) -> Result<OperationAppendOutcome> {
        self.append_operation_inner(operation, expected_project_revision, false)
    }

    #[cfg(test)]
    fn append_operation_after_projection_fault(
        &mut self,
        operation: Operation,
    ) -> Result<OperationAppendOutcome> {
        self.append_operation_inner(operation, None, true)
    }

    fn append_operation_inner(
        &mut self,
        operation: Operation,
        expected_project_revision: Option<ProjectVersion>,
        fail_after_projection: bool,
    ) -> Result<OperationAppendOutcome> {
        if self.mode == crate::OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        ensure_schema(self)?;
        let columns = operation_columns(&operation)?;
        self.start_operation()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let manifest = load_manifest(&transaction)?;
        if manifest.schema_version != crate::manifest::SCHEMA_VERSION
            || !manifest.required_features.is_empty()
        {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        if operation.project_id() != manifest.project_id {
            return Err(StoreError::Operation(
                "operation belongs to another project".into(),
            ));
        }
        let (state, existing_operations) = validate_inventory(&transaction, manifest.project_id)?;
        if let Some(existing) =
            row_for_id(&transaction, operation.operation_id(), manifest.project_id)?
        {
            let existing_wire = existing.operation.to_bytes().map_err(operation_error)?;
            if existing_wire == columns.wire
                && existing.operation.canonical_bytes() == columns.canonical
                && existing.operation.content_hash() == operation.content_hash()
            {
                let outcome = OperationAppendOutcome::Duplicate {
                    project_revision: ProjectVersion::new(existing.project_revision),
                    bundle_revision: manifest.revision,
                };
                transaction.commit()?;
                return Ok(outcome);
            }
            return Err(StoreError::Operation(
                "operation ID already stores different immutable bytes".into(),
            ));
        }
        // Exact duplicate detection precedes the optimistic precondition so a
        // retry after an uncertain commit remains idempotent even when the
        // caller still holds the pre-commit project revision.
        if let Some(expected) = expected_project_revision
            && expected != state.project_revision
        {
            return Err(StoreError::Invalid(format!(
                "stale operation project revision: expected {}, current {}",
                expected.value(),
                state.project_revision.value()
            )));
        }
        if existing_operations.len() >= MAX_OPERATION_COUNT {
            return Err(StoreError::Operation("operation_count".into()));
        }
        let mut candidate_operations = existing_operations;
        candidate_operations.push(operation.clone());
        let candidate_set =
            OperationSet::from_operations(candidate_operations).map_err(operation_error)?;
        validate_replay_admission(&candidate_set).map_err(|error| {
            StoreError::Operation(format!("operation replay is invalid: {error}"))
        })?;
        let next_project_revision = state
            .project_revision
            .value()
            .checked_add(1)
            .filter(|revision| *revision <= MAX_OPERATION_COUNT as u64)
            .ok_or_else(|| StoreError::Operation("project_revision".into()))?;
        let next_project_revision_i64 = i64::try_from(next_project_revision)
            .map_err(|_| StoreError::Operation("project_revision".into()))?;
        let (next_bundle_revision, next_bundle_revision_i64) =
            next_bundle_revision(manifest.revision)?;
        let mut next_manifest = manifest;
        next_manifest.revision = next_bundle_revision;
        let encoded_manifest = next_manifest.encode()?;
        transaction.execute(
            "INSERT INTO project_operations (operation_id,project_id,project_revision,logical_time,causal_depth,content_hash,canonical_bytes,wire_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                columns.operation_id,
                String::from(next_manifest.project_id),
                next_project_revision_i64,
                columns.logical_time,
                columns.causal_depth,
                columns.operation_hash,
                columns.canonical,
                columns.wire,
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE operation_log_state SET project_revision=?1 WHERE singleton=1",
            [next_project_revision_i64],
        )?;
        if changed != 1 {
            return Err(StoreError::Corrupt(
                "operation log state update did not affect exactly one row".into(),
            ));
        }
        let changed = transaction.execute(
            "UPDATE bundle_manifest SET revision=?1,body=?2 WHERE singleton=1",
            (next_bundle_revision_i64, &encoded_manifest),
        )?;
        if changed != 1 {
            return Err(StoreError::Corrupt(
                "operation manifest update did not affect exactly one row".into(),
            ));
        }
        let stored = row_for_id(
            &transaction,
            operation.operation_id(),
            next_manifest.project_id,
        )?
        .ok_or_else(|| StoreError::Corrupt("operation insert disappeared".into()))?;
        if stored.operation != operation || stored.project_revision != next_project_revision {
            return Err(StoreError::Corrupt(
                "operation insert failed authoritative readback".into(),
            ));
        }
        let (stored_state, _) = validate_inventory(&transaction, next_manifest.project_id)?;
        if stored_state.project_revision.value() != next_project_revision {
            return Err(StoreError::Corrupt(
                "operation state failed authoritative readback".into(),
            ));
        }
        let committed_manifest = load_manifest(&transaction)?;
        if committed_manifest != next_manifest {
            return Err(StoreError::Corrupt(
                "operation manifest failed authoritative readback".into(),
            ));
        }
        // The projection is redundant. Publishing it while the transaction is
        // open makes a projection failure roll back the SQLite append.
        atomic_projection(&self.root, &next_manifest)?;
        if fail_after_projection {
            return Err(StoreError::Operation(
                "test fault after projection before SQLite commit".into(),
            ));
        }
        transaction.commit()?;
        Ok(OperationAppendOutcome::Appended {
            project_revision: ProjectVersion::new(next_project_revision),
            bundle_revision: next_bundle_revision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_domain::identity::{ActorDeviceId, ActorId, Text};
    use kyberia_operation_log::{CausalDepth, LogicalTimestamp, Mutation};
    use std::fs;

    fn operation() -> Operation {
        Operation::try_apply(
            OperationId::from_bytes([2; 16]).unwrap(),
            ProjectId::from_bytes([1; 16]).unwrap(),
            ActorId::from_bytes([1; 16]).unwrap(),
            ActorDeviceId::from_bytes([1; 16]).unwrap(),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::set_project_name(Text::new("new").unwrap()),
            Mutation::set_project_name(Text::new("old").unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn projection_ahead_of_sqlite_is_detectable_and_recoverable() {
        let directory = tempfile::tempdir().unwrap().keep();
        let path = directory.join("project");
        let mut bundle = Bundle::create(
            &path,
            ProjectId::from_bytes([1; 16]).unwrap(),
            "Operation fault".into(),
            1,
        )
        .unwrap();
        let error = bundle.append_operation_after_projection_fault(operation());
        assert!(matches!(
            error,
            Err(StoreError::Operation(message))
                if message.contains("after projection before SQLite commit")
        ));
        let projected: crate::BundleManifest =
            serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(projected.revision, 1);
        assert_eq!(bundle.manifest().unwrap().revision, 0);
        drop(bundle);
        let reader = Bundle::open(&path, crate::OpenMode::ReadOnly).unwrap();
        let verification = reader.verify().unwrap();
        assert!(
            verification
                .failures
                .iter()
                .any(|failure| failure.contains("manifest.json projection is stale"))
        );
        drop(reader);
        let repaired = Bundle::open(&path, crate::OpenMode::ReadWrite).unwrap();
        repaired.recover_manifest().unwrap();
        assert!(repaired.verify().unwrap().failures.is_empty());
        assert_eq!(
            repaired.operation_store_state().unwrap().operation_count(),
            0
        );
    }
}
