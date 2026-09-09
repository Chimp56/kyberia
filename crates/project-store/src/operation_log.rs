//! SQLite persistence for the canonical immutable operation DAG.
//!
//! The operation bytes are the authority for operation meaning. SQLite stores
//! the exact wire bytes, the exact unsigned canonical bytes, and their digest
//! in separate columns so a reopen can detect any substitution before replay.
//! The operation-only project revision is a local linear commit counter; it is
//! deliberately separate from each operation's causal depth.

use crate::bundle::{atomic_projection, load_manifest};
use crate::{Bundle, PublicationError, Result, StoreError, sqlite_guard};
use kyberia_domain::identity::{OperationId, ProjectId};
use kyberia_operation_log::{
    AppliedEffect, AppliedMutation, MAX_OPERATION_CANONICAL_BYTES, MAX_OPERATION_COUNT,
    MAX_OPERATION_WIRE_BYTES, MergeError, Operation, OperationError, OperationSet, ProjectVersion,
};
use kyberia_resource_budget::{BudgetKind, CancellationHook, ResourceBudget, ResourceBudgetError};
use rusqlite::{OptionalExtension, Row, Transaction, TransactionBehavior, params};

const OPERATION_ROW_LIMIT: i64 = MAX_OPERATION_COUNT as i64 + 1;
const OPERATION_ID_TEXT_BYTES: usize = 32;
const PROJECT_ID_TEXT_BYTES: usize = 32;
const CONTENT_HASH_TEXT_BYTES: usize = 64;

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
        self.validate_text_lengths()?;
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

    fn validate_text_lengths(&self) -> Result<()> {
        validate_text_length(
            self.operation_id.len(),
            OPERATION_ID_TEXT_BYTES,
            "operation_id",
        )?;
        validate_text_length(self.project_id.len(), PROJECT_ID_TEXT_BYTES, "project_id")?;
        validate_text_length(
            self.content_hash.len(),
            CONTENT_HASH_TEXT_BYTES,
            "content_hash",
        )?;
        Ok(())
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

fn checked_text_length(length: Option<i64>, maximum: usize, name: &str) -> Result<usize> {
    let length = length
        .ok_or_else(|| StoreError::Corrupt(format!("operation {name} has no SQLite length")))?;
    let length = usize::try_from(length)
        .map_err(|_| StoreError::Corrupt(format!("operation {name} has invalid length")))?;
    validate_text_length(length, maximum, name)?;
    Ok(length)
}

fn validate_text_length(length: usize, maximum: usize, name: &str) -> Result<()> {
    if length > maximum {
        return Err(StoreError::Corrupt(format!(
            "operation {name} exceeds its SQLite text limit"
        )));
    }
    Ok(())
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

fn validate_replay_admission_with_budget<H: CancellationHook>(
    set: &OperationSet,
    budget: &mut ResourceBudget<H>,
) -> std::result::Result<(), MergeError> {
    set.validate_replay_semantics_with_budget(budget)
}

fn budget_error(error: ResourceBudgetError) -> StoreError {
    match error {
        ResourceBudgetError::Cancelled => StoreError::Cancelled,
        ResourceBudgetError::LimitExceeded(limit) => {
            StoreError::Materialization(PublicationError::ResourceLimit(limit.kind().label()))
        }
    }
}

fn ensure_schema(bundle: &Bundle) -> Result<()> {
    if !sqlite_guard::has_operation_schema(&bundle.connection)? {
        return Err(StoreError::UnsupportedVersion(1));
    }
    Ok(())
}

fn read_state(transaction: &Transaction<'_>, project_id: ProjectId) -> Result<OperationStoreState> {
    let mut preflight = transaction.prepare(
        "SELECT singleton,length(CAST(project_id AS BLOB)) FROM operation_log_state LIMIT 2",
    )?;
    let mut preflight_rows = preflight.query([])?;
    while let Some(row) = preflight_rows.next()? {
        let _: i64 = row.get(0)?;
        checked_text_length(row.get(1)?, PROJECT_ID_TEXT_BYTES, "project_id")?;
    }
    drop(preflight_rows);
    drop(preflight);
    let mut statement = transaction
        .prepare("SELECT singleton,project_id,project_revision FROM operation_log_state LIMIT 2")?;
    let mut rows = statement.query([])?;
    let Some(row) = rows.next()? else {
        return Err(StoreError::Corrupt(
            "missing operation log state singleton".into(),
        ));
    };
    let singleton: i64 = row.get(0)?;
    let stored_project_raw: String = row.get(1)?;
    validate_text_length(
        stored_project_raw.len(),
        PROJECT_ID_TEXT_BYTES,
        "project_id",
    )?;
    let stored_project = parse_id::<ProjectId>(stored_project_raw, "project_id")?;
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
    read_rows_with_charge(transaction, project_id, |_| Ok(()))
}

fn read_rows_with_budget<H: CancellationHook>(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    budget: &mut ResourceBudget<H>,
) -> Result<Vec<StoredOperation>> {
    read_rows_with_charge(transaction, project_id, |amount| {
        budget
            .charge(BudgetKind::WorkingSetBytes, amount)
            .map_err(budget_error)
    })
}

fn read_rows_with_charge<F>(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    mut charge: F,
) -> Result<Vec<StoredOperation>>
where
    F: FnMut(usize) -> Result<()>,
{
    // Count rows and inspect all variable-length fields through scalar-only
    // queries before selecting any owned SQLite text or BLOB values. A hostile
    // row therefore fails its resource budget without materializing a large
    // value in Rust.
    let mut count_statement =
        transaction.prepare("SELECT project_revision FROM project_operations LIMIT ?1")?;
    let mut count_rows = count_statement.query([OPERATION_ROW_LIMIT])?;
    let mut row_count = 0usize;
    while let Some(row) = count_rows.next()? {
        // A zero charge still polls a caller-owned cancellation hook.
        charge(0)?;
        let _: i64 = row.get(0)?;
        row_count = row_count
            .checked_add(1)
            .ok_or_else(|| StoreError::Corrupt("operation row count overflow".into()))?;
    }
    drop(count_rows);
    drop(count_statement);
    if row_count > MAX_OPERATION_COUNT {
        return Err(StoreError::Corrupt(
            "operation inventory exceeds resource limit".into(),
        ));
    }
    let mut text_statement = transaction.prepare(
        "SELECT length(CAST(operation_id AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(content_hash AS BLOB)) FROM project_operations ORDER BY project_revision,operation_id LIMIT ?1",
    )?;
    let mut text_rows = text_statement.query([OPERATION_ROW_LIMIT])?;
    let mut text_row_count = 0usize;
    while let Some(row) = text_rows.next()? {
        charge(0)?;
        let operation_id =
            checked_text_length(row.get(0)?, OPERATION_ID_TEXT_BYTES, "operation_id")?;
        let project_id = checked_text_length(row.get(1)?, PROJECT_ID_TEXT_BYTES, "project_id")?;
        let content_hash =
            checked_text_length(row.get(2)?, CONTENT_HASH_TEXT_BYTES, "content_hash")?;
        let text_bytes = operation_id
            .checked_add(project_id)
            .and_then(|bytes| bytes.checked_add(content_hash))
            .and_then(|bytes| bytes.checked_add(128))
            .ok_or_else(|| StoreError::Corrupt("operation metadata accounting overflow".into()))?;
        charge(text_bytes)?;
        text_row_count = text_row_count
            .checked_add(1)
            .ok_or_else(|| StoreError::Corrupt("operation metadata row count overflow".into()))?;
    }
    drop(text_rows);
    drop(text_statement);
    if text_row_count != row_count {
        return Err(StoreError::Corrupt(
            "operation row count changed during text preflight".into(),
        ));
    }
    let mut statement = transaction.prepare(
        "SELECT operation_id,length(canonical_bytes),length(wire_bytes) FROM project_operations ORDER BY project_revision,operation_id LIMIT ?1",
    )?;
    let mut rows = statement.query([OPERATION_ROW_LIMIT])?;
    let mut metadata = Vec::with_capacity(row_count);
    let mut result = Vec::with_capacity(row_count);
    while let Some(row) = rows.next()? {
        charge(0)?;
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
        let canonical_length = row.canonical_length.ok_or_else(|| {
            StoreError::Corrupt("operation canonical bytes have no SQLite length".into())
        })? as usize;
        let wire_length = row.wire_length.ok_or_else(|| {
            StoreError::Corrupt("operation wire bytes have no SQLite length".into())
        })? as usize;
        // SQLite first owns the two BLOBs; Operation::from_bytes then owns a
        // decoded payload and performs a canonical re-encode. Charge those
        // known proportional copies before loading or decoding the row.
        let blob_bytes = canonical_length
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(wire_length.checked_mul(3)?))
            .and_then(|bytes| bytes.checked_add(256))
            .ok_or_else(|| StoreError::Corrupt("operation read accounting overflow".into()))?;
        // The scalar length query above is deliberately followed by this
        // admission charge before SQLite materializes either BLOB.
        charge(blob_bytes)?;
        let operation_id = row.operation_id;
        let loaded = transaction.query_row(
            "SELECT operation_id,project_id,project_revision,logical_time,causal_depth,content_hash,canonical_bytes,wire_bytes FROM project_operations WHERE operation_id=?1",
            [operation_id],
            RawOperationRow::from_row,
        )?;
        charge(0)?;
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

fn validate_inventory_with_budget<H: CancellationHook>(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    budget: &mut ResourceBudget<H>,
) -> Result<(OperationStoreState, Vec<Operation>)> {
    budget
        .charge(BudgetKind::WorkingSetBytes, 256)
        .map_err(budget_error)?;
    let mut state = read_state(transaction, project_id)?;
    let rows = read_rows_with_budget(transaction, project_id, budget)?;
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
        // Charge the retained clone inputs before the iterator can clone them.
        // The constructor charges its own canonical set representation as well;
        // that intentional overestimate covers both storage inventory and set
        // ownership in the same cumulative transaction budget.
        for operation in &operations {
            let bytes = operation
                .canonical_bytes()
                .len()
                .checked_add(operation.wire_bytes_len().map_err(operation_error)?)
                .and_then(|bytes| bytes.checked_add(128))
                .ok_or_else(|| StoreError::Corrupt("operation copy accounting overflow".into()))?;
            budget
                .charge(BudgetKind::WorkingSetBytes, bytes)
                .map_err(budget_error)?;
        }
        let set = OperationSet::from_operations_with_budget(operations.iter().cloned(), budget)
            .map_err(|error| match error {
                OperationError::Cancelled => StoreError::Cancelled,
                OperationError::ResourceLimit(reason) => {
                    StoreError::Materialization(PublicationError::ResourceLimit(reason))
                }
                other => StoreError::Corrupt(format!("stored operation graph is invalid: {other}")),
            })?;
        validate_replay_admission_with_budget(&set, budget).map_err(|error| match error {
            MergeError::Cancelled => StoreError::Cancelled,
            MergeError::ResourceLimit(reason) => {
                StoreError::Materialization(PublicationError::ResourceLimit(reason))
            }
            other => StoreError::Corrupt(format!("stored operation replay is invalid: {other}")),
        })?;
    }
    state.operation_count = operations.len();
    Ok((state, operations))
}

pub(crate) fn validated_operation_set(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
) -> Result<(OperationStoreState, OperationSet)> {
    let (state, set) = validated_operation_set_at_revision(
        transaction,
        project_id,
        ProjectVersion::new(state_revision(transaction, project_id)?),
    )?;
    Ok((state, set))
}

/// Reconstruct the immutable operation prefix represented by a materialization
/// publication. The linear operation revision is the only storage ordering;
/// causal depth remains an operation field and is never used as a revision.
pub(crate) fn validated_operation_set_at_revision(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    revision: ProjectVersion,
) -> Result<(OperationStoreState, OperationSet)> {
    ValidatedOperationInventory::load(transaction, project_id)?.prefix(revision)
}

pub(crate) fn validated_operation_set_at_revision_with_budget<H: CancellationHook>(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    revision: ProjectVersion,
    budget: &mut ResourceBudget<H>,
) -> Result<(OperationStoreState, OperationSet)> {
    ValidatedOperationInventory::load_with_budget(transaction, project_id, budget)?
        .prefix_with_budget(revision, budget)
}

/// Transaction-scoped decoded evidence, reusable across historical publications.
pub(crate) struct ValidatedOperationInventory {
    state: OperationStoreState,
    operations: Vec<Operation>,
}

impl ValidatedOperationInventory {
    pub(crate) fn load(transaction: &Transaction<'_>, project_id: ProjectId) -> Result<Self> {
        let (state, operations) = validate_inventory(transaction, project_id)?;
        Ok(Self { state, operations })
    }

    pub(crate) fn load_with_budget<H: CancellationHook>(
        transaction: &Transaction<'_>,
        project_id: ProjectId,
        budget: &mut ResourceBudget<H>,
    ) -> Result<Self> {
        let (state, operations) = validate_inventory_with_budget(transaction, project_id, budget)?;
        Ok(Self { state, operations })
    }

    pub(crate) fn prefix(
        &self,
        revision: ProjectVersion,
    ) -> Result<(OperationStoreState, OperationSet)> {
        if revision.value() > self.state.project_revision.value() {
            return Err(StoreError::Corrupt(
                "requested operation history revision is ahead of operation log".into(),
            ));
        }
        let operations = self.operations[..revision.value() as usize].to_vec();
        let set = if operations.is_empty() {
            OperationSet::empty(self.state.project_id)
        } else {
            OperationSet::from_operations(operations).map_err(operation_error)?
        };
        Ok((
            OperationStoreState {
                project_id: self.state.project_id,
                project_revision: revision,
                operation_count: revision.value() as usize,
            },
            set,
        ))
    }

    pub(crate) fn prefix_with_budget<H: CancellationHook>(
        &self,
        revision: ProjectVersion,
        budget: &mut ResourceBudget<H>,
    ) -> Result<(OperationStoreState, OperationSet)> {
        if revision.value() > self.state.project_revision.value() {
            return Err(StoreError::Corrupt(
                "requested operation history revision is ahead of operation log".into(),
            ));
        }
        let source = &self.operations[..revision.value() as usize];
        let mut copy_bytes = 0usize;
        for operation in source {
            let wire_bytes = operation.wire_bytes_len().map_err(operation_error)?;
            copy_bytes = copy_bytes
                .checked_add(operation.canonical_bytes().len())
                .and_then(|bytes| bytes.checked_add(wire_bytes))
                .and_then(|bytes| bytes.checked_add(128))
                .ok_or_else(|| {
                    StoreError::Corrupt("operation prefix accounting overflow".into())
                })?;
        }
        budget
            .charge(BudgetKind::WorkingSetBytes, copy_bytes)
            .map_err(budget_error)?;
        let operations = source.to_vec();
        let set = if operations.is_empty() {
            OperationSet::empty(self.state.project_id)
        } else {
            OperationSet::from_operations_with_budget(operations, budget).map_err(|error| {
                match error {
                    OperationError::Cancelled => StoreError::Cancelled,
                    OperationError::ResourceLimit(reason) => {
                        StoreError::Materialization(PublicationError::ResourceLimit(reason))
                    }
                    other => operation_error(other),
                }
            })?
        };
        Ok((
            OperationStoreState {
                project_id: self.state.project_id,
                project_revision: revision,
                operation_count: revision.value() as usize,
            },
            set,
        ))
    }
}

fn state_revision(transaction: &Transaction<'_>, project_id: ProjectId) -> Result<u64> {
    Ok(read_state(transaction, project_id)?
        .project_revision
        .value())
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
            "SELECT length(CAST(operation_id AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(content_hash AS BLOB)),length(canonical_bytes),length(wire_bytes) FROM project_operations WHERE operation_id=?1",
            [&id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((
        operation_id_length,
        project_id_length,
        content_hash_length,
        canonical_length,
        wire_length,
    )) = metadata
    else {
        return Ok(None);
    };
    checked_text_length(operation_id_length, OPERATION_ID_TEXT_BYTES, "operation_id")?;
    checked_text_length(project_id_length, PROJECT_ID_TEXT_BYTES, "project_id")?;
    checked_text_length(content_hash_length, CONTENT_HASH_TEXT_BYTES, "content_hash")?;
    let canonical_length =
        checked_blob_length(canonical_length, MAX_OPERATION_CANONICAL_BYTES, "canonical")?;
    let wire_length = checked_blob_length(wire_length, MAX_OPERATION_WIRE_BYTES, "wire")?;
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
    row.validate_text_lengths()?;
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

    #[test]
    fn operation_scalar_preflight_rejects_oversized_text_before_blob_load() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE project_operations (operation_id TEXT, project_id TEXT, project_revision INTEGER, logical_time INTEGER, causal_depth INTEGER, content_hash TEXT, canonical_bytes BLOB, wire_bytes BLOB)",
            )
            .unwrap();
        let project_id = ProjectId::from_bytes([1; 16]).unwrap();
        let oversized_operation_id = "x".repeat(OPERATION_ID_TEXT_BYTES + 1_024);
        connection
            .execute(
                "INSERT INTO project_operations VALUES (?1,?2,1,1,0,?3,?4,?5)",
                rusqlite::params![
                    oversized_operation_id,
                    String::from(project_id),
                    "a".repeat(CONTENT_HASH_TEXT_BYTES),
                    vec![1_u8],
                    vec![1_u8],
                ],
            )
            .unwrap();
        let transaction = rusqlite::Transaction::new_unchecked(
            &connection,
            rusqlite::TransactionBehavior::Deferred,
        )
        .unwrap();
        let error = match read_rows(&transaction, project_id) {
            Ok(_) => panic!("oversized operation text was accepted"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            StoreError::Corrupt(message) if message.contains("operation_id exceeds its SQLite text limit")
        ));
    }

    #[test]
    fn operation_state_scalar_preflight_rejects_oversized_project_before_decode() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE operation_log_state (singleton INTEGER, project_id TEXT, project_revision INTEGER)",
            )
            .unwrap();
        let project_id = ProjectId::from_bytes([1; 16]).unwrap();
        connection
            .execute(
                "INSERT INTO operation_log_state VALUES (1,?1,0)",
                ["x".repeat(PROJECT_ID_TEXT_BYTES + 1_024)],
            )
            .unwrap();
        let transaction = rusqlite::Transaction::new_unchecked(
            &connection,
            rusqlite::TransactionBehavior::Deferred,
        )
        .unwrap();
        let error = match read_state(&transaction, project_id) {
            Ok(_) => panic!("oversized operation state text was accepted"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            StoreError::Corrupt(message) if message.contains("operation project_id exceeds its SQLite text limit")
        ));
    }
}
