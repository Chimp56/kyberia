//! Transactional publication of a validated causal materialized project.
//!
//! This module is the outer storage adapter for `kyberia-causal-materializer`.
//! It stores exact identity and project bytes as immutable artifacts, while
//! SQLite owns the publication index, current pointer, and bundle revision.
//! No storage type is imported by the domain or materializer.

use crate::bundle::{atomic_projection, load_manifest};
use crate::manifest::{MAX_ARTIFACT_BYTES, SCHEMA_VERSION};
use crate::operation_log::{
    ValidatedOperationInventory, validated_operation_set,
    validated_operation_set_at_revision_with_budget,
};
use crate::{
    ArtifactEntry, ArtifactKind, Bundle, BundleManifest, OpenMode, Result, StoreError,
    content_hash, sqlite_guard,
};
use kyberia_causal_materializer::{MAX_MATERIALIZATION_OPERATIONS, MaterializedProject};
use kyberia_domain::identity::{ContentHash, ProjectId};
use kyberia_domain::project::{Project, ProjectSchemaVersion};
use kyberia_materialization_identity::{
    BaselineIdentity, MaterializationIdentity, OperationSetIdentity,
};
use kyberia_operation_log::{OperationSet, ProjectVersion};
use kyberia_resource_budget::{
    BudgetKind, CancellationHook, ResourceBudget, ResourceBudgetError, ResourceLimits,
};
use rusqlite::{OptionalExtension, Row, Transaction, TransactionBehavior, params};
use std::fmt;

const PUBLICATION_SCHEMA_VERSION: i64 = 1;
const MATERIALIZER_PROTOCOL_VERSION: i64 = 1;
const MAX_PUBLICATIONS: usize = 8_192;
const PUBLICATION_ID_DOMAIN: &[u8] = b"KYBERIA\0MATERIALIZED-PROJECT\0";
const BASELINE_MEDIA_TYPE: &str = "application/vnd.kyberia.materialization-baseline;version=1";
const PROJECT_MEDIA_TYPE_V1: &str = "application/vnd.kyberia.project.materialized+json;version=1";
const PROJECT_MEDIA_TYPE_V2: &str = "application/vnd.kyberia.project.materialized+json;version=2";
const BASELINE_PROVENANCE: &str = "kyberia:materialization-baseline:v1";
const PROJECT_PROVENANCE: &str = "kyberia:materialized-project:v1";
const PROJECT_ID_TEXT_BYTES: usize = 32;
const CONTENT_HASH_TEXT_BYTES: usize = 64;
const PUBLICATION_ID_TEXT_BYTES: usize = 64;

const VERIFICATION_WORKING_SET_BYTES: usize = 512 * 1024 * 1024;
const fn verification_limit(value: u64) -> usize {
    let target_limit = usize::MAX / 4;
    if value > target_limit as u64 {
        target_limit
    } else {
        value as usize
    }
}

// Keep the cumulative defaults finite and valid on 32-bit targets as well as
// the supported 64-bit desktop targets. Per-job limits in the inner crates
// remain stricter than these aggregate ceilings.
const VERIFICATION_OPERATION_BYTES: usize = verification_limit(4 * 1024 * 1024 * 1024);
const VERIFICATION_PROJECT_COPY_BYTES: usize = verification_limit(4 * 1024 * 1024 * 1024);
const VERIFICATION_ANCESTRY_WORK: usize = verification_limit(2_000_000 * 8_192);
const VERIFICATION_WITNESS_WORK: usize = verification_limit(2_000_000 * 8_192);
const VERIFICATION_CONFLICT_CHECKS: usize = verification_limit(1_000_000 * 8_192);

fn default_materialization_verification_budget() -> ResourceBudget {
    ResourceBudget::new(ResourceLimits::new(
        VERIFICATION_ANCESTRY_WORK,
        VERIFICATION_WITNESS_WORK,
        VERIFICATION_CONFLICT_CHECKS,
        VERIFICATION_OPERATION_BYTES,
        VERIFICATION_PROJECT_COPY_BYTES,
        VERIFICATION_WORKING_SET_BYTES,
    ))
}

fn budget_error(error: ResourceBudgetError) -> StoreError {
    match error {
        ResourceBudgetError::Cancelled => StoreError::Cancelled,
        ResourceBudgetError::LimitExceeded(limit) => {
            StoreError::Materialization(PublicationError::ResourceLimit(limit.kind().label()))
        }
    }
}

fn decoded_working_set_bytes(bytes: usize, copies: usize, field: &'static str) -> Result<usize> {
    bytes
        .checked_mul(copies)
        .and_then(|value| value.checked_add(128))
        .ok_or_else(|| StoreError::Materialization(PublicationError::ResourceLimit(field)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicationError {
    Identity(String),
    WrongProject,
    InputIdentityMismatch,
    BaselineNotRegistered,
    StaleOperationRevision {
        expected: ProjectVersion,
        actual: ProjectVersion,
    },
    ConflictingCurrentPublication,
    Corrupt(String),
    ResourceLimit(&'static str),
    Invalid(&'static str),
    TestFault,
}

impl fmt::Display for PublicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PublicationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializationPublicationReceipt {
    publication_id: String,
    project_id: ProjectId,
    baseline_identity_hash: ContentHash,
    operation_set_identity_hash: ContentHash,
    baseline_artifact_hash: String,
    materialized_artifact_hash: String,
    baseline_project_revision: u64,
    baseline_logical_time: u64,
    materialized_project_revision: u64,
    materialized_logical_time: u64,
    operation_count: usize,
    operation_project_revision: ProjectVersion,
    operation_max_causal_depth: u64,
    protocol_version: u32,
    result_schema_version: ProjectSchemaVersion,
    bundle_revision: u64,
    committed_utc_ms: i64,
}

impl MaterializationPublicationReceipt {
    pub fn publication_id(&self) -> &str {
        &self.publication_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn baseline_identity_hash(&self) -> ContentHash {
        self.baseline_identity_hash
    }

    pub const fn operation_set_identity_hash(&self) -> ContentHash {
        self.operation_set_identity_hash
    }

    pub fn baseline_artifact_hash(&self) -> &str {
        &self.baseline_artifact_hash
    }

    pub fn materialized_artifact_hash(&self) -> &str {
        &self.materialized_artifact_hash
    }

    pub const fn baseline_project_revision(&self) -> u64 {
        self.baseline_project_revision
    }

    pub const fn baseline_logical_time(&self) -> u64 {
        self.baseline_logical_time
    }

    pub const fn materialized_project_revision(&self) -> u64 {
        self.materialized_project_revision
    }

    pub const fn materialized_logical_time(&self) -> u64 {
        self.materialized_logical_time
    }

    pub const fn operation_count(&self) -> usize {
        self.operation_count
    }

    pub const fn operation_project_revision(&self) -> ProjectVersion {
        self.operation_project_revision
    }

    /// The maximum [`CausalDepth`] in the operation set. It is DAG metadata,
    /// not the linear persisted operation or materialized project revision.
    pub const fn operation_max_causal_depth(&self) -> u64 {
        self.operation_max_causal_depth
    }

    pub const fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    pub const fn result_schema_version(&self) -> ProjectSchemaVersion {
        self.result_schema_version
    }

    pub const fn bundle_revision(&self) -> u64 {
        self.bundle_revision
    }

    pub const fn committed_utc_ms(&self) -> i64 {
        self.committed_utc_ms
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterializationPublicationOutcome {
    Published(MaterializationPublicationReceipt),
    Duplicate(MaterializationPublicationReceipt),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedMaterializedProject {
    project: Project,
    receipt: MaterializationPublicationReceipt,
}

impl LoadedMaterializedProject {
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn into_project(self) -> Project {
        self.project
    }

    pub const fn receipt(&self) -> &MaterializationPublicationReceipt {
        &self.receipt
    }
}

#[derive(Clone, Debug)]
struct PublicationInput {
    publication_id: String,
    project_id: ProjectId,
    baseline_identity: BaselineIdentity,
    operation_set_identity: OperationSetIdentity,
    materialization_identity: MaterializationIdentity,
    baseline_artifact_hash: String,
    materialized_artifact_hash: String,
    baseline_artifact_bytes: usize,
    materialized_artifact_bytes: usize,
    materialized_project_revision: u64,
    materialized_logical_time: u64,
    materialized_project_bytes: Vec<u8>,
    operation_count: usize,
    operation_max_causal_depth: u64,
    protocol_version: i64,
    result_schema_version: ProjectSchemaVersion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RawBaseline {
    baseline_identity_hash: String,
    project_id: String,
    artifact_hash: String,
    artifact_bytes: i64,
    protocol_version: i64,
    project_revision: i64,
    logical_time: i64,
    committed_utc_ms: i64,
}

/// Baseline evidence decoded once for a verification transaction. The
/// identity and project remain borrowed by every historical publication so a
/// long publication history cannot reset or multiply baseline parsing work.
#[derive(Debug)]
struct VerifiedBaseline {
    identity: BaselineIdentity,
    project: Project,
}

impl RawBaseline {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            baseline_identity_hash: row.get(0)?,
            project_id: row.get(1)?,
            artifact_hash: row.get(2)?,
            artifact_bytes: row.get(3)?,
            protocol_version: row.get(4)?,
            project_revision: row.get(5)?,
            logical_time: row.get(6)?,
            committed_utc_ms: row.get(7)?,
        })
    }

    fn validate_text_lengths(&self) -> Result<()> {
        validate_text_length(
            self.baseline_identity_hash.len(),
            CONTENT_HASH_TEXT_BYTES,
            "baseline identity hash",
        )?;
        validate_text_length(
            self.project_id.len(),
            PROJECT_ID_TEXT_BYTES,
            "baseline project id",
        )?;
        validate_text_length(
            self.artifact_hash.len(),
            CONTENT_HASH_TEXT_BYTES,
            "baseline artifact hash",
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RawPublication {
    publication_id: String,
    project_id: String,
    protocol_version: i64,
    result_schema_version: i64,
    baseline_identity_hash: String,
    baseline_artifact_hash: String,
    baseline_artifact_bytes: i64,
    operation_set_identity_hash: String,
    operation_count: i64,
    operation_project_revision: i64,
    operation_max_causal_depth: i64,
    baseline_project_revision: i64,
    baseline_logical_time: i64,
    materialized_artifact_hash: String,
    materialized_artifact_bytes: i64,
    materialized_project_revision: i64,
    materialized_logical_time: i64,
    bundle_revision: i64,
    committed_utc_ms: i64,
}

impl RawPublication {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            publication_id: row.get(0)?,
            project_id: row.get(1)?,
            protocol_version: row.get(2)?,
            result_schema_version: row.get(3)?,
            baseline_identity_hash: row.get(4)?,
            baseline_artifact_hash: row.get(5)?,
            baseline_artifact_bytes: row.get(6)?,
            operation_set_identity_hash: row.get(7)?,
            operation_count: row.get(8)?,
            operation_project_revision: row.get(9)?,
            operation_max_causal_depth: row.get(10)?,
            baseline_project_revision: row.get(11)?,
            baseline_logical_time: row.get(12)?,
            materialized_artifact_hash: row.get(13)?,
            materialized_artifact_bytes: row.get(14)?,
            materialized_project_revision: row.get(15)?,
            materialized_logical_time: row.get(16)?,
            bundle_revision: row.get(17)?,
            committed_utc_ms: row.get(18)?,
        })
    }

    fn validate_text_lengths(&self) -> Result<()> {
        validate_text_length(
            self.publication_id.len(),
            PUBLICATION_ID_TEXT_BYTES,
            "publication id",
        )?;
        validate_text_length(
            self.project_id.len(),
            PROJECT_ID_TEXT_BYTES,
            "publication project id",
        )?;
        validate_text_length(
            self.baseline_identity_hash.len(),
            CONTENT_HASH_TEXT_BYTES,
            "baseline identity hash",
        )?;
        validate_text_length(
            self.baseline_artifact_hash.len(),
            CONTENT_HASH_TEXT_BYTES,
            "baseline artifact hash",
        )?;
        validate_text_length(
            self.operation_set_identity_hash.len(),
            CONTENT_HASH_TEXT_BYTES,
            "operation set identity hash",
        )?;
        validate_text_length(
            self.materialized_artifact_hash.len(),
            CONTENT_HASH_TEXT_BYTES,
            "materialized artifact hash",
        )?;
        Ok(())
    }

    fn receipt(&self) -> Result<MaterializationPublicationReceipt> {
        Ok(MaterializationPublicationReceipt {
            publication_id: self.publication_id.clone(),
            project_id: parse_id(self.project_id.clone(), "publication project_id")?,
            protocol_version: u32::try_from(bounded_u64(
                self.protocol_version,
                MATERIALIZER_PROTOCOL_VERSION as u64,
                "materializer protocol version",
            )?)
            .map_err(|_| {
                StoreError::Materialization(PublicationError::Corrupt("protocol version".into()))
            })?,
            result_schema_version: match self.result_schema_version {
                1 => ProjectSchemaVersion::V1,
                2 => ProjectSchemaVersion::V2,
                _ => {
                    return Err(StoreError::Materialization(PublicationError::Corrupt(
                        "materialized result schema version".into(),
                    )));
                }
            },
            baseline_identity_hash: parse_hash(
                self.baseline_identity_hash.clone(),
                "baseline identity hash",
            )?,
            operation_set_identity_hash: parse_hash(
                self.operation_set_identity_hash.clone(),
                "operation set identity hash",
            )?,
            baseline_artifact_hash: self.baseline_artifact_hash.clone(),
            materialized_artifact_hash: self.materialized_artifact_hash.clone(),
            baseline_project_revision: nonnegative(
                self.baseline_project_revision,
                "baseline project revision",
            )?,
            baseline_logical_time: nonnegative(
                self.baseline_logical_time,
                "baseline logical time",
            )?,
            materialized_project_revision: nonnegative(
                self.materialized_project_revision,
                "materialized project revision",
            )?,
            materialized_logical_time: nonnegative(
                self.materialized_logical_time,
                "materialized logical time",
            )?,
            operation_count: bounded_usize(
                self.operation_count,
                MAX_MATERIALIZATION_OPERATIONS,
                "operation count",
            )?,
            operation_project_revision: ProjectVersion::new(bounded_u64(
                self.operation_project_revision,
                MAX_MATERIALIZATION_OPERATIONS as u64,
                "operation project revision",
            )?),
            operation_max_causal_depth: nonnegative(
                self.operation_max_causal_depth,
                "operation max causal depth",
            )?,
            bundle_revision: positive(self.bundle_revision, "publication bundle revision")?,
            committed_utc_ms: nonnegative_i64(self.committed_utc_ms, "publication timestamp")?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RawState {
    project_id: String,
    publication_id: String,
    operation_project_revision: i64,
    bundle_revision: i64,
}

impl RawState {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            project_id: row.get(0)?,
            publication_id: row.get(1)?,
            operation_project_revision: row.get(2)?,
            bundle_revision: row.get(3)?,
        })
    }

    fn validate_text_lengths(&self) -> Result<()> {
        validate_text_length(
            self.project_id.len(),
            PROJECT_ID_TEXT_BYTES,
            "state project id",
        )?;
        validate_text_length(
            self.publication_id.len(),
            PUBLICATION_ID_TEXT_BYTES,
            "state publication id",
        )?;
        Ok(())
    }
}

fn parse_id(raw: String, field: &str) -> Result<ProjectId> {
    ProjectId::try_from(raw)
        .map_err(|_| StoreError::Materialization(PublicationError::Corrupt(field.into())))
}

fn parse_hash(raw: String, field: &str) -> Result<ContentHash> {
    ContentHash::try_from(raw)
        .map_err(|_| StoreError::Materialization(PublicationError::Corrupt(field.into())))
}

fn nonnegative(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| StoreError::Materialization(PublicationError::Corrupt(field.into())))
}

fn nonnegative_i64(value: i64, field: &str) -> Result<i64> {
    if value < 0 {
        Err(StoreError::Materialization(PublicationError::Corrupt(
            field.into(),
        )))
    } else {
        Ok(value)
    }
}

fn positive(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| StoreError::Materialization(PublicationError::Corrupt(field.into())))
}

fn bounded_u64(value: i64, max: u64, field: &str) -> Result<u64> {
    let value = nonnegative(value, field)?;
    if value > max {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            field.into(),
        )));
    }
    Ok(value)
}

fn bounded_usize(value: i64, max: usize, field: &str) -> Result<usize> {
    let value = bounded_u64(value, max as u64, field)?;
    usize::try_from(value)
        .map_err(|_| StoreError::Materialization(PublicationError::Corrupt(field.into())))
}

fn checked_text_length(length: Option<i64>, maximum: usize, field: &str) -> Result<usize> {
    let length = length
        .map(usize::try_from)
        .transpose()
        .map_err(|_| {
            StoreError::Materialization(PublicationError::Corrupt(format!(
                "{field} has an invalid SQLite text length"
            )))
        })?
        .ok_or_else(|| {
            StoreError::Materialization(PublicationError::Corrupt(format!(
                "{field} has no SQLite text length"
            )))
        })?;
    validate_text_length(length, maximum, field)?;
    Ok(length)
}

fn validate_text_length(length: usize, maximum: usize, field: &str) -> Result<()> {
    if length > maximum {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            format!("{field} exceeds its SQLite text limit"),
        )));
    }
    Ok(())
}

fn checked_text_lengths(lengths: &[Option<i64>], fields: &[(&str, usize)]) -> Result<usize> {
    if lengths.len() != fields.len() {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "SQLite text preflight field mismatch".into(),
        )));
    }
    lengths
        .iter()
        .zip(fields)
        .try_fold(0usize, |total, (length, (field, maximum))| {
            let length = checked_text_length(*length, *maximum, field)?;
            total.checked_add(length).ok_or_else(|| {
                StoreError::Materialization(PublicationError::ResourceLimit("metadata_text_bytes"))
            })
        })
}

fn metadata_charge_bytes(text_bytes: usize, fixed_bytes: usize) -> Result<usize> {
    text_bytes.checked_add(fixed_bytes).ok_or_else(|| {
        StoreError::Materialization(PublicationError::ResourceLimit(
            "metadata_working_set_bytes",
        ))
    })
}

const fn result_schema_version_value(version: ProjectSchemaVersion) -> i64 {
    match version {
        ProjectSchemaVersion::V1 => 1,
        ProjectSchemaVersion::V2 => 2,
    }
}

fn validate_baseline_row(
    baseline: &BaselineIdentity,
    row: &RawBaseline,
    expected_artifact_hash: &str,
    expected_artifact_bytes: usize,
) -> Result<()> {
    if parse_hash(row.baseline_identity_hash.clone(), "baseline identity hash")?
        != baseline.content_hash()
        || parse_id(row.project_id.clone(), "baseline project_id")? != baseline.project_id()
        || row.artifact_hash != expected_artifact_hash
        || usize::try_from(row.artifact_bytes).ok() != Some(expected_artifact_bytes)
        || row.protocol_version != MATERIALIZER_PROTOCOL_VERSION
        || nonnegative(row.project_revision, "baseline project revision")? != baseline.revision()
        || nonnegative(row.logical_time, "baseline logical time")? != baseline.logical_time()
    {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "registered baseline does not match canonical identity".into(),
        )));
    }
    nonnegative_i64(row.committed_utc_ms, "baseline registration timestamp")?;
    Ok(())
}

fn publication_id(project_id: ProjectId, identity: &MaterializationIdentity) -> String {
    publication_id_from_hashes(
        project_id,
        identity.baseline_hash(),
        identity.operation_set_hash(),
    )
}

fn publication_id_from_hashes(
    project_id: ProjectId,
    baseline_hash: ContentHash,
    operation_hash: ContentHash,
) -> String {
    let mut bytes = Vec::with_capacity(PUBLICATION_ID_DOMAIN.len() + 1 + 16 + 32 + 32);
    bytes.extend_from_slice(PUBLICATION_ID_DOMAIN);
    bytes.push(PUBLICATION_SCHEMA_VERSION as u8);
    bytes.extend_from_slice(&project_id.bytes());
    bytes.extend_from_slice(&baseline_hash.bytes());
    bytes.extend_from_slice(&operation_hash.bytes());
    content_hash(&bytes)
}

const BASELINE_TEXT_FIELDS: [(&str, usize); 3] = [
    ("baseline identity hash", CONTENT_HASH_TEXT_BYTES),
    ("baseline project id", PROJECT_ID_TEXT_BYTES),
    ("baseline artifact hash", CONTENT_HASH_TEXT_BYTES),
];
const PUBLICATION_TEXT_FIELDS: [(&str, usize); 6] = [
    ("publication id", PUBLICATION_ID_TEXT_BYTES),
    ("publication project id", PROJECT_ID_TEXT_BYTES),
    ("baseline identity hash", CONTENT_HASH_TEXT_BYTES),
    ("baseline artifact hash", CONTENT_HASH_TEXT_BYTES),
    ("operation set identity hash", CONTENT_HASH_TEXT_BYTES),
    ("materialized artifact hash", CONTENT_HASH_TEXT_BYTES),
];
const STATE_TEXT_FIELDS: [(&str, usize); 2] = [
    ("state project id", PROJECT_ID_TEXT_BYTES),
    ("state publication id", PUBLICATION_ID_TEXT_BYTES),
];

fn baseline_text_lengths(row: &Row<'_>) -> rusqlite::Result<[Option<i64>; 3]> {
    Ok([row.get(0)?, row.get(1)?, row.get(2)?])
}

fn publication_text_lengths(row: &Row<'_>) -> rusqlite::Result<[Option<i64>; 6]> {
    Ok([
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ])
}

fn state_text_lengths(row: &Row<'_>) -> rusqlite::Result<[Option<i64>; 2]> {
    Ok([row.get(0)?, row.get(1)?])
}

fn preflight_publication_by_id(
    transaction: &Transaction<'_>,
    publication_id: &str,
) -> Result<Option<usize>> {
    transaction
        .query_row(
            "SELECT length(CAST(publication_id AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(baseline_identity_hash AS BLOB)),length(CAST(baseline_artifact_hash AS BLOB)),length(CAST(operation_set_identity_hash AS BLOB)),length(CAST(materialized_artifact_hash AS BLOB)) FROM materialized_project_publications WHERE publication_id=?1",
            [publication_id],
            publication_text_lengths,
        )
        .optional()?
        .map(|lengths| checked_text_lengths(&lengths, &PUBLICATION_TEXT_FIELDS))
        .transpose()
}

fn preflight_publications<H: CancellationHook>(
    transaction: &Transaction<'_>,
    budget: &mut ResourceBudget<H>,
) -> Result<usize> {
    let mut statement = transaction.prepare(
        "SELECT length(CAST(publication_id AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(baseline_identity_hash AS BLOB)),length(CAST(baseline_artifact_hash AS BLOB)),length(CAST(operation_set_identity_hash AS BLOB)),length(CAST(materialized_artifact_hash AS BLOB)) FROM materialized_project_publications ORDER BY publication_id LIMIT ?1",
    )?;
    let mut rows = statement.query([i64::try_from(MAX_PUBLICATIONS + 1).unwrap()])?;
    let mut count = 0usize;
    while let Some(row) = rows.next()? {
        if count >= MAX_PUBLICATIONS {
            return Err(StoreError::Materialization(
                PublicationError::ResourceLimit("materialized_publications"),
            ));
        }
        budget.check_cancelled().map_err(budget_error)?;
        let text_bytes =
            checked_text_lengths(&publication_text_lengths(row)?, &PUBLICATION_TEXT_FIELDS)?;
        budget
            .charge(
                BudgetKind::WorkingSetBytes,
                metadata_charge_bytes(text_bytes, 256)?,
            )
            .map_err(budget_error)?;
        count = count.checked_add(1).ok_or_else(|| {
            StoreError::Materialization(PublicationError::ResourceLimit(
                "materialized_publications",
            ))
        })?;
    }
    Ok(count)
}

fn preflight_baseline_by_hash(
    transaction: &Transaction<'_>,
    baseline_identity_hash: &str,
) -> Result<Option<usize>> {
    transaction
        .query_row(
            "SELECT length(CAST(baseline_identity_hash AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(artifact_hash AS BLOB)) FROM materialization_baselines WHERE baseline_identity_hash=?1",
            [baseline_identity_hash],
            baseline_text_lengths,
        )
        .optional()?
        .map(|lengths| checked_text_lengths(&lengths, &BASELINE_TEXT_FIELDS))
        .transpose()
}

fn preflight_baselines_by_project(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
) -> Result<(usize, usize)> {
    let mut statement = transaction.prepare(
        "SELECT length(CAST(baseline_identity_hash AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(artifact_hash AS BLOB)) FROM materialization_baselines WHERE project_id=?1 ORDER BY baseline_identity_hash LIMIT 2",
    )?;
    let mut rows = statement.query([String::from(project_id)])?;
    let mut count = 0usize;
    let mut text_bytes = 0usize;
    while let Some(row) = rows.next()? {
        text_bytes = text_bytes
            .checked_add(checked_text_lengths(
                &baseline_text_lengths(row)?,
                &BASELINE_TEXT_FIELDS,
            )?)
            .ok_or_else(|| {
                StoreError::Materialization(PublicationError::ResourceLimit("metadata_text_bytes"))
            })?;
        count = count.checked_add(1).ok_or_else(|| {
            StoreError::Materialization(PublicationError::ResourceLimit(
                "materialization_baselines",
            ))
        })?;
    }
    Ok((count, text_bytes))
}

fn preflight_baseline_inventory(transaction: &Transaction<'_>) -> Result<(usize, usize)> {
    let mut statement = transaction.prepare(
        "SELECT length(CAST(baseline_identity_hash AS BLOB)),length(CAST(project_id AS BLOB)),length(CAST(artifact_hash AS BLOB)) FROM materialization_baselines LIMIT 2",
    )?;
    let mut rows = statement.query([])?;
    let mut count = 0usize;
    let mut text_bytes = 0usize;
    while let Some(row) = rows.next()? {
        text_bytes = text_bytes
            .checked_add(checked_text_lengths(
                &baseline_text_lengths(row)?,
                &BASELINE_TEXT_FIELDS,
            )?)
            .ok_or_else(|| {
                StoreError::Materialization(PublicationError::ResourceLimit("metadata_text_bytes"))
            })?;
        count = count.checked_add(1).ok_or_else(|| {
            StoreError::Materialization(PublicationError::ResourceLimit(
                "materialization_baselines",
            ))
        })?;
    }
    Ok((count, text_bytes))
}

fn preflight_state_query(transaction: &Transaction<'_>) -> Result<(usize, usize)> {
    let mut statement = transaction.prepare(
        "SELECT length(CAST(project_id AS BLOB)),length(CAST(publication_id AS BLOB)) FROM materialized_project_state LIMIT 2",
    )?;
    let mut rows = statement.query([])?;
    let mut count = 0usize;
    let mut text_bytes = 0usize;
    while let Some(row) = rows.next()? {
        text_bytes = text_bytes
            .checked_add(checked_text_lengths(
                &state_text_lengths(row)?,
                &STATE_TEXT_FIELDS,
            )?)
            .ok_or_else(|| {
                StoreError::Materialization(PublicationError::ResourceLimit("metadata_text_bytes"))
            })?;
        count = count.checked_add(1).ok_or_else(|| {
            StoreError::Materialization(PublicationError::ResourceLimit("materialization_state"))
        })?;
    }
    Ok((count, text_bytes))
}

fn publication_query(
    transaction: &Transaction<'_>,
    publication_id: &str,
) -> Result<Option<RawPublication>> {
    preflight_publication_by_id(transaction, publication_id)?;
    let result = transaction
        .query_row(
            "SELECT publication_id,project_id,protocol_version,result_schema_version,baseline_identity_hash,baseline_artifact_hash,baseline_artifact_bytes,operation_set_identity_hash,operation_count,operation_project_revision,operation_max_causal_depth,baseline_project_revision,baseline_logical_time,materialized_artifact_hash,materialized_artifact_bytes,materialized_project_revision,materialized_logical_time,bundle_revision,committed_utc_ms FROM materialized_project_publications WHERE publication_id=?1",
            [publication_id],
            RawPublication::from_row,
        )
        .optional()
        .map_err(StoreError::from)?;
    if let Some(ref publication) = result {
        publication.validate_text_lengths()?;
    }
    Ok(result)
}

fn baseline_query(
    transaction: &Transaction<'_>,
    baseline_identity_hash: &str,
) -> Result<Option<RawBaseline>> {
    preflight_baseline_by_hash(transaction, baseline_identity_hash)?;
    let result = transaction
        .query_row(
            "SELECT baseline_identity_hash,project_id,artifact_hash,artifact_bytes,protocol_version,project_revision,logical_time,committed_utc_ms FROM materialization_baselines WHERE baseline_identity_hash=?1",
            [baseline_identity_hash],
            RawBaseline::from_row,
        )
        .optional()
        .map_err(StoreError::from)?;
    if let Some(ref baseline) = result {
        baseline.validate_text_lengths()?;
    }
    Ok(result)
}

fn project_baseline_query(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
) -> Result<Option<RawBaseline>> {
    let (count, _) = preflight_baselines_by_project(transaction, project_id)?;
    if count == 0 {
        return Ok(None);
    }
    let mut statement = transaction.prepare(
        "SELECT baseline_identity_hash,project_id,artifact_hash,artifact_bytes,protocol_version,project_revision,logical_time,committed_utc_ms FROM materialization_baselines WHERE project_id=?1 ORDER BY baseline_identity_hash LIMIT 2",
    )?;
    let mut rows = statement.query([String::from(project_id)])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let result = RawBaseline::from_row(row)?;
    result.validate_text_lengths()?;
    if rows.next()?.is_some() {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "project has multiple registered materialization baselines".into(),
        )));
    }
    Ok(Some(result))
}

fn state_query(transaction: &Transaction<'_>) -> Result<Option<RawState>> {
    let (count, _) = preflight_state_query(transaction)?;
    if count == 0 {
        return Ok(None);
    }
    let mut statement = transaction.prepare(
        "SELECT project_id,publication_id,operation_project_revision,bundle_revision FROM materialized_project_state LIMIT 2",
    )?;
    let mut rows = statement.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let state = RawState::from_row(row)?;
    state.validate_text_lengths()?;
    if rows.next()?.is_some() {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "materialization state requires one singleton".into(),
        )));
    }
    Ok(Some(state))
}

fn publication_values_match(input: &PublicationInput, row: &RawPublication) -> Result<bool> {
    Ok(row.publication_id == input.publication_id
        && parse_id(row.project_id.clone(), "publication project_id")? == input.project_id
        && row.protocol_version == input.protocol_version
        && row.result_schema_version == result_schema_version_value(input.result_schema_version)
        && parse_hash(row.baseline_identity_hash.clone(), "baseline identity hash")?
            == input.materialization_identity.baseline_hash()
        && row.baseline_artifact_hash == input.baseline_artifact_hash
        && usize::try_from(row.baseline_artifact_bytes).ok() == Some(input.baseline_artifact_bytes)
        && parse_hash(
            row.operation_set_identity_hash.clone(),
            "operation set identity hash",
        )? == input.materialization_identity.operation_set_hash()
        && bounded_usize(
            row.operation_count,
            MAX_MATERIALIZATION_OPERATIONS,
            "operation count",
        )? == input.operation_count
        && nonnegative(row.operation_max_causal_depth, "operation max causal depth")?
            == input.operation_max_causal_depth
        && nonnegative(row.baseline_project_revision, "baseline project revision")?
            == input.baseline_identity.revision()
        && nonnegative(row.baseline_logical_time, "baseline logical time")?
            == input.baseline_identity.logical_time()
        && row.materialized_artifact_hash == input.materialized_artifact_hash
        && usize::try_from(row.materialized_artifact_bytes).ok()
            == Some(input.materialized_artifact_bytes)
        && nonnegative(
            row.materialized_project_revision,
            "materialized project revision",
        )? == input.materialized_project_revision
        && nonnegative(row.materialized_logical_time, "materialized logical time")?
            == input.materialized_logical_time)
}

fn artifact_entry(
    manifest: &BundleManifest,
    hash: &str,
    expected_kind: ArtifactKind,
    expected_media_type: &str,
    expected_bytes: usize,
) -> Result<ArtifactEntry> {
    let entry = manifest.artifacts.get(hash).ok_or_else(|| {
        StoreError::Materialization(PublicationError::Corrupt(
            "publication artifact is not registered".into(),
        ))
    })?;
    if entry.kind != expected_kind
        || entry.media_type != expected_media_type
        || entry.bytes != expected_bytes as u64
        || entry.provenance_id
            != match expected_kind {
                ArtifactKind::MaterializationBaseline => BASELINE_PROVENANCE,
                ArtifactKind::MaterializedProject => PROJECT_PROVENANCE,
                _ => "",
            }
    {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "publication artifact registration mismatch".into(),
        )));
    }
    Ok(entry.clone())
}

fn validate_artifacts(
    inventory: Option<&ValidatedOperationInventory>,
    transaction: &Transaction<'_>,
    bundle: &Bundle,
    manifest: &BundleManifest,
    row: &RawPublication,
    baseline_row: &RawBaseline,
) -> Result<Project> {
    let mut budget = default_materialization_verification_budget();
    validate_artifacts_with_budget(
        inventory,
        transaction,
        bundle,
        manifest,
        row,
        baseline_row,
        None,
        &mut budget,
    )
}

// The arguments deliberately keep storage handles, the immutable row, and the
// optional transaction-verified baseline separate at this adapter boundary.
#[allow(clippy::too_many_arguments)]
fn validate_artifacts_with_budget<H: CancellationHook>(
    inventory: Option<&ValidatedOperationInventory>,
    transaction: &Transaction<'_>,
    bundle: &Bundle,
    manifest: &BundleManifest,
    row: &RawPublication,
    baseline_row: &RawBaseline,
    verified_baseline: Option<&VerifiedBaseline>,
    budget: &mut ResourceBudget<H>,
) -> Result<Project> {
    budget.check_cancelled().map_err(budget_error)?;
    if positive(row.bundle_revision, "publication bundle revision")? > manifest.revision {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "publication revision exceeds committed manifest".into(),
        )));
    }
    if row.protocol_version != MATERIALIZER_PROTOCOL_VERSION {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "unsupported materializer protocol version".into(),
        )));
    }
    let project_id = parse_id(row.project_id.clone(), "publication project_id")?;
    let baseline_hash = parse_hash(row.baseline_identity_hash.clone(), "baseline identity hash")?;
    let operation_hash = parse_hash(
        row.operation_set_identity_hash.clone(),
        "operation set identity hash",
    )?;
    let baseline_bytes = usize::try_from(row.baseline_artifact_bytes).map_err(|_| {
        StoreError::Materialization(PublicationError::Corrupt("baseline artifact length".into()))
    })?;
    if row.publication_id != publication_id_from_hashes(project_id, baseline_hash, operation_hash) {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "publication id is not derived from its identities".into(),
        )));
    }
    let baseline_entry = artifact_entry(
        manifest,
        &row.baseline_artifact_hash,
        ArtifactKind::MaterializationBaseline,
        BASELINE_MEDIA_TYPE,
        baseline_bytes,
    )?;
    let baseline_owned = if let Some(verified) = verified_baseline {
        if verified.identity.content_hash() != baseline_hash {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "publication baseline differs from transaction-verified baseline".into(),
            )));
        }
        None
    } else {
        budget
            .charge(
                BudgetKind::WorkingSetBytes,
                baseline_bytes.checked_add(128).ok_or_else(|| {
                    StoreError::Materialization(PublicationError::ResourceLimit(
                        "baseline_artifact_bytes",
                    ))
                })?,
            )
            .map_err(budget_error)?;
        let baseline_artifact =
            bundle.read_registered_artifact(&row.baseline_artifact_hash, &baseline_entry)?;
        budget
            // The strict decoder allocates a Project, canonical JSON, a
            // retained project-byte copy, and the final identity encoding.
            // Charge all known proportional copies before invoking it.
            .charge(
                BudgetKind::WorkingSetBytes,
                decoded_working_set_bytes(
                    baseline_artifact.len(),
                    4,
                    "baseline_identity_decode_bytes",
                )?,
            )
            .map_err(budget_error)?;
        let baseline = BaselineIdentity::from_canonical_bytes(&baseline_artifact)
            .map_err(identity_validation_error)?;
        Some(baseline)
    };
    let baseline = verified_baseline
        .map(|verified| &verified.identity)
        .or(baseline_owned.as_ref())
        .ok_or_else(|| {
            StoreError::Materialization(PublicationError::Corrupt(
                "missing verified baseline".into(),
            ))
        })?;
    if baseline.content_hash() != baseline_hash
        || baseline.project_id() != project_id
        || baseline.revision() != nonnegative(row.baseline_project_revision, "baseline revision")?
        || baseline.logical_time()
            != nonnegative(row.baseline_logical_time, "baseline logical time")?
    {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "baseline artifact does not match publication row".into(),
        )));
    }
    validate_baseline_row(
        baseline,
        baseline_row,
        &row.baseline_artifact_hash,
        baseline_bytes,
    )?;

    let project_bytes = usize::try_from(row.materialized_artifact_bytes).map_err(|_| {
        StoreError::Materialization(PublicationError::Corrupt(
            "materialized artifact length".into(),
        ))
    })?;
    let media_type = match row.result_schema_version {
        1 => PROJECT_MEDIA_TYPE_V1,
        2 => PROJECT_MEDIA_TYPE_V2,
        _ => {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "unsupported materialized result schema version".into(),
            )));
        }
    };
    let project_entry = artifact_entry(
        manifest,
        &row.materialized_artifact_hash,
        ArtifactKind::MaterializedProject,
        media_type,
        project_bytes,
    )?;
    budget
        .charge(
            BudgetKind::WorkingSetBytes,
            project_bytes.checked_add(128).ok_or_else(|| {
                StoreError::Materialization(PublicationError::ResourceLimit(
                    "materialized_artifact_bytes",
                ))
            })?,
        )
        .map_err(budget_error)?;
    let project_artifact =
        bundle.read_registered_artifact(&row.materialized_artifact_hash, &project_entry)?;
    budget
        // Project decoding allocates the domain value and canonical JSON
        // readback bytes; charge both before serde or canonicalization runs.
        .charge(
            BudgetKind::WorkingSetBytes,
            decoded_working_set_bytes(
                project_artifact.len(),
                3,
                "materialized_project_decode_bytes",
            )?,
        )
        .map_err(budget_error)?;
    let project: Project = serde_json::from_slice(&project_artifact).map_err(|error| {
        StoreError::Materialization(PublicationError::Corrupt(format!(
            "materialized project decode failed: {error}"
        )))
    })?;
    if serde_json::to_vec(&project)? != project_artifact
        || project.id() != project_id
        || project.revision()
            != nonnegative(row.materialized_project_revision, "materialized revision")?
        || project.logical_time()
            != nonnegative(row.materialized_logical_time, "materialized logical time")?
        || content_hash(&project_artifact) != row.materialized_artifact_hash
        || result_schema_version_value(project.schema_version()) != row.result_schema_version
    {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "materialized project artifact does not match publication row".into(),
        )));
    }
    let baseline_project_owned = if verified_baseline.is_some() {
        None
    } else {
        budget
            .charge(BudgetKind::WorkingSetBytes, baseline.project_bytes().len())
            .map_err(budget_error)?;
        Some(serde_json::from_slice(baseline.project_bytes())?)
    };
    let baseline_project = verified_baseline
        .map(|verified| &verified.project)
        .or(baseline_project_owned.as_ref())
        .ok_or_else(|| {
            StoreError::Materialization(PublicationError::Corrupt(
                "missing baseline project".into(),
            ))
        })?;
    let operations =
        validate_historical_operations_with_budget(inventory, transaction, row, budget)?;
    let replayed =
        kyberia_causal_materializer::materialize_with_budget(baseline_project, &operations, budget)
            .map_err(replay_validation_error)?;
    if replayed.project() != &project {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "stored result differs from canonical replay".into(),
        )));
    }
    Ok(project)
}

fn validate_historical_operations_with_budget<H: CancellationHook>(
    inventory: Option<&ValidatedOperationInventory>,
    transaction: &Transaction<'_>,
    row: &RawPublication,
    budget: &mut ResourceBudget<H>,
) -> Result<OperationSet> {
    budget.check_cancelled().map_err(budget_error)?;
    let project_id = parse_id(row.project_id.clone(), "publication project_id")?;
    let revision = ProjectVersion::new(bounded_u64(
        row.operation_project_revision,
        MAX_MATERIALIZATION_OPERATIONS as u64,
        "operation project revision",
    )?);
    let (state, operations) = match inventory {
        Some(inventory) => inventory.prefix_with_budget(revision, budget)?,
        None => validated_operation_set_at_revision_with_budget(
            transaction,
            project_id,
            revision,
            budget,
        )?,
    };
    let identity = OperationSetIdentity::from_operation_set_with_budget(&operations, budget)
        .map_err(identity_validation_error)?;
    let expected_hash = parse_hash(
        row.operation_set_identity_hash.clone(),
        "operation set identity hash",
    )?;
    let expected_count = bounded_usize(
        row.operation_count,
        MAX_MATERIALIZATION_OPERATIONS,
        "operation count",
    )?;
    if state.project_revision() != revision
        || state.operation_count() != expected_count
        || identity.content_hash() != expected_hash
    {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "publication operation history does not match persisted operation prefix".into(),
        )));
    }
    let max_depth = operations
        .operations()
        .map(|operation| operation.causal_depth().value())
        .max()
        .unwrap_or(0);
    if max_depth != nonnegative(row.operation_max_causal_depth, "operation max causal depth")? {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "publication causal depth summary does not match operation prefix".into(),
        )));
    }
    Ok(operations)
}

fn validate_state(manifest: &BundleManifest, state: &RawState, row: &RawPublication) -> Result<()> {
    let project_id = parse_id(state.project_id.clone(), "materialization state project_id")?;
    if project_id != manifest.project_id
        || state.publication_id != row.publication_id
        || state.project_id != row.project_id
        || state.operation_project_revision != row.operation_project_revision
        || state.bundle_revision != row.bundle_revision
        || positive(
            state.bundle_revision,
            "materialization state bundle revision",
        )? > manifest.revision
    {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "materialization current pointer does not match publication".into(),
        )));
    }
    Ok(())
}

fn register_artifact(
    manifest: &mut BundleManifest,
    hash: String,
    entry: ArtifactEntry,
) -> Result<()> {
    entry.validate()?;
    if let Some(existing) = manifest.artifacts.get(&hash) {
        if existing != &entry {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "immutable publication artifact has different registration".into(),
            )));
        }
    } else {
        manifest.artifacts.insert(hash, entry);
    }
    Ok(())
}

fn input(
    baseline: &Project,
    operations: &OperationSet,
    materialized: &MaterializedProject,
) -> Result<PublicationInput> {
    let baseline_identity =
        BaselineIdentity::from_project(baseline).map_err(identity_input_error)?;
    let operation_set_identity =
        OperationSetIdentity::from_operation_set(operations).map_err(identity_input_error)?;
    let identity = MaterializationIdentity::bind(&baseline_identity, &operation_set_identity)
        .map_err(identity_input_error)?;
    if materialized.identity() != &identity {
        return Err(StoreError::Materialization(
            PublicationError::InputIdentityMismatch,
        ));
    }
    if baseline.id() != operations.project_id() || materialized.project().id() != baseline.id() {
        return Err(StoreError::Materialization(PublicationError::WrongProject));
    }
    let materialized_project_bytes = serde_json::to_vec(materialized.project())?;
    if materialized_project_bytes.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err(StoreError::Materialization(
            PublicationError::ResourceLimit("materialized_project_bytes"),
        ));
    }
    let publication_id = publication_id(baseline.id(), &identity);
    let operation_max_causal_depth = operations
        .operations()
        .map(|operation| operation.causal_depth().value())
        .max()
        .unwrap_or(0);
    Ok(PublicationInput {
        publication_id,
        project_id: baseline.id(),
        baseline_artifact_hash: content_hash(baseline_identity.canonical_bytes()),
        materialized_artifact_hash: content_hash(&materialized_project_bytes),
        baseline_artifact_bytes: baseline_identity.canonical_bytes().len(),
        materialized_artifact_bytes: materialized_project_bytes.len(),
        materialized_project_revision: materialized.project().revision(),
        materialized_logical_time: materialized.project().logical_time(),
        operation_count: identity.operation_count(),
        baseline_identity,
        operation_set_identity,
        materialization_identity: identity,
        materialized_project_bytes,
        operation_max_causal_depth,
        protocol_version: MATERIALIZER_PROTOCOL_VERSION,
        result_schema_version: materialized.project().schema_version(),
    })
}

fn public_receipt(row: &RawPublication) -> Result<MaterializationPublicationReceipt> {
    row.receipt()
}

fn validate_existing_duplicate(
    transaction: &Transaction<'_>,
    bundle: &Bundle,
    manifest: &BundleManifest,
    input: &PublicationInput,
    row: &RawPublication,
    baseline_row: &RawBaseline,
) -> Result<MaterializationPublicationReceipt> {
    if !publication_values_match(input, row)? {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "publication identity already stores different immutable inputs".into(),
        )));
    }
    let _project = validate_artifacts(None, transaction, bundle, manifest, row, baseline_row)?;
    public_receipt(row)
}

fn ensure_publication_schema(bundle: &Bundle) -> Result<()> {
    if !sqlite_guard::has_materialized_project_schema(&bundle.connection)? {
        return Err(StoreError::UnsupportedVersion(SCHEMA_VERSION));
    }
    Ok(())
}

impl Bundle {
    /// Persist the canonical starting project before publishing derived state.
    /// Registration is immutable for this project; exact retries are idempotent.
    pub fn register_materialization_baseline(
        &mut self,
        baseline: &Project,
        utc_ms: i64,
    ) -> Result<ContentHash> {
        self.register_materialization_baseline_inner(baseline, utc_ms, false)
    }

    fn register_materialization_baseline_inner(
        &mut self,
        baseline: &Project,
        utc_ms: i64,
        fail_after_projection: bool,
    ) -> Result<ContentHash> {
        if self.mode == OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        nonnegative_i64(utc_ms, "baseline registration timestamp")?;
        let identity = BaselineIdentity::from_project(baseline).map_err(identity_input_error)?;
        ensure_publication_schema(self)?;
        let hash = self.write_artifact_file(identity.canonical_bytes())?;
        self.start_operation()?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let mut manifest = load_manifest(&transaction)?;
        if manifest.project_id != identity.project_id() {
            return Err(StoreError::Materialization(PublicationError::WrongProject));
        }
        if let Some(row) = project_baseline_query(&transaction, identity.project_id())? {
            if row.baseline_identity_hash != String::from(identity.content_hash()) {
                return Err(StoreError::Materialization(
                    PublicationError::InputIdentityMismatch,
                ));
            }
            validate_baseline_row(&identity, &row, &hash, identity.canonical_bytes().len())?;
            let entry = artifact_entry(
                &manifest,
                &hash,
                ArtifactKind::MaterializationBaseline,
                BASELINE_MEDIA_TYPE,
                identity.canonical_bytes().len(),
            )?;
            if self.read_registered_artifact(&hash, &entry)? != identity.canonical_bytes() {
                return Err(StoreError::Materialization(PublicationError::Corrupt(
                    "registered baseline bytes differ from retry".into(),
                )));
            }
            transaction.commit()?;
            return Ok(identity.content_hash());
        }
        register_artifact(
            &mut manifest,
            hash.clone(),
            ArtifactEntry {
                kind: ArtifactKind::MaterializationBaseline,
                bytes: identity.canonical_bytes().len() as u64,
                media_type: BASELINE_MEDIA_TYPE.into(),
                provenance_id: BASELINE_PROVENANCE.into(),
            },
        )?;
        transaction.execute(
            "INSERT INTO materialization_baselines (baseline_identity_hash,project_id,artifact_hash,artifact_bytes,protocol_version,project_revision,logical_time,committed_utc_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![String::from(identity.content_hash()), String::from(identity.project_id()), hash,
                identity.canonical_bytes().len() as i64, MATERIALIZER_PROTOCOL_VERSION,
                i64::try_from(identity.revision()).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("baseline revision")))?,
                i64::try_from(identity.logical_time()).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("baseline logical time")))?, utc_ms],
        )?;
        manifest.revision = manifest
            .revision
            .checked_add(1)
            .ok_or(StoreError::Materialization(
                PublicationError::ResourceLimit("bundle revision"),
            ))?;
        let revision = i64::try_from(manifest.revision).map_err(|_| {
            StoreError::Materialization(PublicationError::ResourceLimit("bundle revision"))
        })?;
        manifest.updated_utc_ms = manifest.updated_utc_ms.max(utc_ms);
        let changed = transaction.execute(
            "UPDATE bundle_manifest SET revision=?1,body=?2 WHERE singleton=1",
            params![revision, manifest.encode()?],
        )?;
        if changed != 1 || load_manifest(&transaction)? != manifest {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "baseline registration manifest readback mismatch".into(),
            )));
        }
        let stored = read_baseline_project(self, &transaction, &manifest)?;
        if stored.as_ref() != Some(baseline) {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "baseline registration readback mismatch".into(),
            )));
        }
        atomic_projection(&self.root, &manifest)?;
        if fail_after_projection {
            return Err(StoreError::Materialization(PublicationError::TestFault));
        }
        transaction.commit()?;
        Ok(identity.content_hash())
    }

    /// Atomically publish a validated causal materialization and its exact
    /// baseline identity. The operation revision is the operation-store
    /// counter, while the returned bundle revision is the metadata revision.
    pub fn publish_materialized_project(
        &mut self,
        baseline: &Project,
        operations: &OperationSet,
        materialized: &MaterializedProject,
        expected_operation_revision: ProjectVersion,
        utc_ms: i64,
    ) -> Result<MaterializationPublicationOutcome> {
        self.publish_materialized_project_inner(
            baseline,
            operations,
            materialized,
            expected_operation_revision,
            utc_ms,
            false,
        )
    }

    #[cfg(test)]
    pub(crate) fn publish_materialized_project_after_projection_fault(
        &mut self,
        baseline: &Project,
        operations: &OperationSet,
        materialized: &MaterializedProject,
        expected_operation_revision: ProjectVersion,
        utc_ms: i64,
    ) -> Result<MaterializationPublicationOutcome> {
        self.publish_materialized_project_inner(
            baseline,
            operations,
            materialized,
            expected_operation_revision,
            utc_ms,
            true,
        )
    }

    fn publish_materialized_project_inner(
        &mut self,
        baseline: &Project,
        operations: &OperationSet,
        materialized: &MaterializedProject,
        expected_operation_revision: ProjectVersion,
        utc_ms: i64,
        fail_after_projection: bool,
    ) -> Result<MaterializationPublicationOutcome> {
        if self.mode == OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        if utc_ms < 0 {
            return Err(StoreError::Materialization(PublicationError::Invalid(
                "publication timestamp must be nonnegative",
            )));
        }
        ensure_publication_schema(self)?;
        let input = input(baseline, operations, materialized)?;
        let baseline_artifact_hash =
            self.write_artifact_file(input.baseline_identity.canonical_bytes())?;
        let materialized_artifact_hash =
            self.write_artifact_file(&input.materialized_project_bytes)?;
        if baseline_artifact_hash != input.baseline_artifact_hash
            || materialized_artifact_hash != input.materialized_artifact_hash
        {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "publication artifact hash changed during write".into(),
            )));
        }

        self.start_operation()?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let manifest = load_manifest(&transaction)?;
        if manifest.schema_version != SCHEMA_VERSION || !manifest.required_features.is_empty() {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        if manifest.project_id != input.project_id {
            return Err(StoreError::Materialization(PublicationError::WrongProject));
        }
        let (operation_state, persisted_operations) =
            validated_operation_set(&transaction, manifest.project_id)?;
        // Resolve the exact immutable publication first. This is deliberately
        // before the optimistic current-revision check so a lost response can
        // be retried with its original expected revision.
        let existing = publication_query(&transaction, &input.publication_id)?;
        if let Some(row) = existing {
            let baseline_row = baseline_query(&transaction, &row.baseline_identity_hash)?
                .ok_or_else(|| {
                    StoreError::Materialization(PublicationError::Corrupt(
                        "publication points to missing baseline registration".into(),
                    ))
                })?;
            let receipt = validate_existing_duplicate(
                &transaction,
                self,
                &manifest,
                &input,
                &row,
                &baseline_row,
            )?;
            transaction.commit()?;
            return Ok(MaterializationPublicationOutcome::Duplicate(receipt));
        }
        let persisted_identity = OperationSetIdentity::from_operation_set(&persisted_operations)
            .map_err(identity_validation_error)?;
        if persisted_identity != input.operation_set_identity {
            return Err(StoreError::Materialization(
                PublicationError::StaleOperationRevision {
                    expected: expected_operation_revision,
                    actual: operation_state.project_revision(),
                },
            ));
        }
        if expected_operation_revision != operation_state.project_revision() {
            return Err(StoreError::Materialization(
                PublicationError::StaleOperationRevision {
                    expected: expected_operation_revision,
                    actual: operation_state.project_revision(),
                },
            ));
        }
        let registered_baseline = baseline_query(
            &transaction,
            &String::from(input.materialization_identity.baseline_hash()),
        )?;
        if let Some(row) = registered_baseline.as_ref() {
            validate_baseline_row(
                &input.baseline_identity,
                row,
                &input.baseline_artifact_hash,
                input.baseline_artifact_bytes,
            )?;
        } else if project_baseline_query(&transaction, input.project_id)?.is_some() {
            return Err(StoreError::Materialization(
                PublicationError::InputIdentityMismatch,
            ));
        } else {
            return Err(StoreError::Materialization(
                PublicationError::BaselineNotRegistered,
            ));
        }
        let current = state_query(&transaction)?;
        if let Some(state) = &current {
            let current_row =
                publication_query(&transaction, &state.publication_id)?.ok_or_else(|| {
                    StoreError::Materialization(PublicationError::Corrupt(
                        "materialization state points to missing publication".into(),
                    ))
                })?;
            validate_state(&manifest, state, &current_row)?;
            if state.operation_project_revision
                > i64::try_from(operation_state.project_revision().value()).unwrap_or(i64::MAX)
            {
                return Err(StoreError::Materialization(PublicationError::Corrupt(
                    "materialization state is ahead of operation log".into(),
                )));
            }
            if state.operation_project_revision
                == i64::try_from(operation_state.project_revision().value()).unwrap_or(i64::MAX)
            {
                return Err(StoreError::Materialization(
                    PublicationError::ConflictingCurrentPublication,
                ));
            }
        }
        let (next_bundle_revision, next_bundle_revision_i64) = {
            let next = manifest.revision.checked_add(1).ok_or_else(|| {
                StoreError::Materialization(PublicationError::ResourceLimit("bundle_revision"))
            })?;
            let sql = i64::try_from(next).map_err(|_| {
                StoreError::Materialization(PublicationError::ResourceLimit("bundle_revision"))
            })?;
            (next, sql)
        };
        let baseline_entry = ArtifactEntry {
            kind: ArtifactKind::MaterializationBaseline,
            bytes: input.baseline_artifact_bytes as u64,
            media_type: BASELINE_MEDIA_TYPE.to_owned(),
            provenance_id: BASELINE_PROVENANCE.to_owned(),
        };
        let project_media_type = match input.result_schema_version {
            ProjectSchemaVersion::V1 => PROJECT_MEDIA_TYPE_V1,
            ProjectSchemaVersion::V2 => PROJECT_MEDIA_TYPE_V2,
        };
        let project_entry = ArtifactEntry {
            kind: ArtifactKind::MaterializedProject,
            bytes: input.materialized_artifact_bytes as u64,
            media_type: project_media_type.to_owned(),
            provenance_id: PROJECT_PROVENANCE.to_owned(),
        };
        let mut next_manifest = manifest.clone();
        register_artifact(
            &mut next_manifest,
            input.baseline_artifact_hash.clone(),
            baseline_entry,
        )?;
        register_artifact(
            &mut next_manifest,
            input.materialized_artifact_hash.clone(),
            project_entry,
        )?;
        next_manifest.revision = next_bundle_revision;
        next_manifest.updated_utc_ms = next_manifest.updated_utc_ms.max(utc_ms);
        let encoded_manifest = next_manifest.encode()?;
        transaction.execute(
            "INSERT INTO materialized_project_publications (publication_id,project_id,protocol_version,result_schema_version,baseline_identity_hash,baseline_artifact_hash,baseline_artifact_bytes,operation_set_identity_hash,operation_count,operation_project_revision,operation_max_causal_depth,baseline_project_revision,baseline_logical_time,materialized_artifact_hash,materialized_artifact_bytes,materialized_project_revision,materialized_logical_time,bundle_revision,committed_utc_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![
                input.publication_id,
                String::from(input.project_id),
                input.protocol_version,
                result_schema_version_value(input.result_schema_version),
                String::from(input.materialization_identity.baseline_hash()),
                input.baseline_artifact_hash,
                i64::try_from(input.baseline_artifact_bytes).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("baseline_artifact_bytes")))?,
                String::from(input.materialization_identity.operation_set_hash()),
                i64::try_from(input.operation_count).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("operation_count")))?,
                i64::try_from(operation_state.project_revision().value()).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("operation_project_revision")))?,
                i64::try_from(input.operation_max_causal_depth).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("operation_max_causal_depth")))?,
                i64::try_from(input.baseline_identity.revision()).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("baseline_project_revision")))?,
                i64::try_from(input.baseline_identity.logical_time()).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("baseline_logical_time")))?,
                input.materialized_artifact_hash,
                i64::try_from(input.materialized_artifact_bytes).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("materialized_artifact_bytes")))?,
                i64::try_from(input.materialized_project_revision).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("materialized_project_revision")))?,
                i64::try_from(input.materialized_logical_time).map_err(|_| StoreError::Materialization(PublicationError::ResourceLimit("materialized_logical_time")))?,
                next_bundle_revision_i64,
                utc_ms,
            ],
        )?;
        let operation_revision_i64 = i64::try_from(operation_state.project_revision().value())
            .map_err(|_| {
                StoreError::Materialization(PublicationError::ResourceLimit(
                    "operation_project_revision",
                ))
            })?;
        if let Some(state) = current {
            let changed = transaction.execute(
                "UPDATE materialized_project_state SET project_id=?1,publication_id=?2,operation_project_revision=?3,bundle_revision=?4 WHERE singleton=1",
                params![
                    String::from(input.project_id),
                    publication_id(input.project_id, &input.materialization_identity),
                    operation_revision_i64,
                    next_bundle_revision_i64,
                ],
            )?;
            if changed != 1 || state.project_id != String::from(input.project_id) {
                return Err(StoreError::Materialization(PublicationError::Corrupt(
                    "materialization state update did not affect one row".into(),
                )));
            }
        } else {
            transaction.execute(
                "INSERT INTO materialized_project_state (singleton,project_id,publication_id,operation_project_revision,bundle_revision) VALUES (1,?1,?2,?3,?4)",
                params![
                    String::from(input.project_id),
                    publication_id(input.project_id, &input.materialization_identity),
                    operation_revision_i64,
                    next_bundle_revision_i64,
                ],
            )?;
        }
        let changed = transaction.execute(
            "UPDATE bundle_manifest SET revision=?1,body=?2 WHERE singleton=1",
            (next_bundle_revision_i64, &encoded_manifest),
        )?;
        if changed != 1 {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "materialization manifest update did not affect one row".into(),
            )));
        }
        let stored = publication_query(
            &transaction,
            &publication_id(input.project_id, &input.materialization_identity),
        )?
        .ok_or_else(|| {
            StoreError::Materialization(PublicationError::Corrupt(
                "publication insert disappeared".into(),
            ))
        })?;
        if !publication_values_match(&input, &stored)?
            || stored.operation_project_revision != operation_revision_i64
            || stored.bundle_revision != next_bundle_revision_i64
        {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "materialization publication readback mismatch".into(),
            )));
        }
        let stored_state = state_query(&transaction)?.ok_or_else(|| {
            StoreError::Materialization(PublicationError::Corrupt(
                "materialization state insert disappeared".into(),
            ))
        })?;
        validate_state(&next_manifest, &stored_state, &stored)?;
        if load_manifest(&transaction)? != next_manifest {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "materialization manifest readback mismatch".into(),
            )));
        }
        atomic_projection(&self.root, &next_manifest)?;
        if fail_after_projection {
            return Err(StoreError::Materialization(PublicationError::TestFault));
        }
        transaction.commit()?;
        Ok(MaterializationPublicationOutcome::Published(
            public_receipt(&stored)?,
        ))
    }

    /// Load the registered canonical starting project, independently of publication.
    /// Legacy and unregistered bundles return explicit absence.
    pub fn materialization_baseline(&self) -> Result<Option<Project>> {
        self.start_operation()?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        let baseline = if sqlite_guard::has_materialized_project_schema(&transaction)? {
            read_baseline_project(self, &transaction, &manifest)?
        } else {
            None
        };
        transaction.commit()?;
        Ok(baseline)
    }

    /// Read the current materialized project after validating its immutable
    /// artifacts, publication row, current pointer, and manifest relationship.
    pub fn materialized_project(&self) -> Result<Option<LoadedMaterializedProject>> {
        self.start_operation()?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        let manifest = load_manifest(&transaction)?;
        if !sqlite_guard::has_materialized_project_schema(&transaction)? {
            transaction.commit()?;
            return Ok(None);
        }
        let Some(state) = state_query(&transaction)? else {
            transaction.commit()?;
            return Ok(None);
        };
        let row = publication_query(&transaction, &state.publication_id)?.ok_or_else(|| {
            StoreError::Materialization(PublicationError::Corrupt(
                "materialization state points to missing publication".into(),
            ))
        })?;
        let baseline_row =
            baseline_query(&transaction, &row.baseline_identity_hash)?.ok_or_else(|| {
                StoreError::Materialization(PublicationError::Corrupt(
                    "current publication points to missing baseline registration".into(),
                ))
            })?;
        validate_state(&manifest, &state, &row)?;
        let project = validate_artifacts(None, &transaction, self, &manifest, &row, &baseline_row)?;
        let receipt = public_receipt(&row)?;
        transaction.commit()?;
        Ok(Some(LoadedMaterializedProject { project, receipt }))
    }

    pub(crate) fn verify_materialized_project_publication(&self) -> Result<()> {
        let mut budget = default_materialization_verification_budget();
        self.verify_materialized_project_publication_with_budget(&mut budget)
    }

    /// Verify every immutable materialization publication in one transaction
    /// using one cumulative budget. Reusing a budget here is essential: a
    /// publication history must not evade limits by resetting replay work for
    /// every row. Publication rows are read in deterministic ID order and no
    /// row is skipped when a limit or cancellation is reached.
    pub fn verify_materialized_project_publication_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<()> {
        budget.check_cancelled().map_err(budget_error)?;
        ensure_publication_schema(self)?;
        self.start_operation()?;
        let transaction =
            Transaction::new_unchecked(&self.connection, TransactionBehavior::Deferred)?;
        charge_manifest_decode(&transaction, budget)?;
        let manifest = load_manifest(&transaction)?;
        let verified_baseline = read_verified_baseline(self, &transaction, &manifest, budget)?;
        let publication_count = preflight_publications(&transaction, budget)?;
        let mut statement = transaction.prepare(
            "SELECT publication_id,project_id,protocol_version,result_schema_version,baseline_identity_hash,baseline_artifact_hash,baseline_artifact_bytes,operation_set_identity_hash,operation_count,operation_project_revision,operation_max_causal_depth,baseline_project_revision,baseline_logical_time,materialized_artifact_hash,materialized_artifact_bytes,materialized_project_revision,materialized_logical_time,bundle_revision,committed_utc_ms FROM materialized_project_publications ORDER BY publication_id LIMIT ?1",
        )?;
        let mut rows = statement.query([i64::try_from(MAX_PUBLICATIONS + 1).unwrap()])?;
        let mut publications = Vec::with_capacity(publication_count);
        while let Some(row) = rows.next()? {
            if publications.len() >= MAX_PUBLICATIONS {
                return Err(StoreError::Materialization(
                    PublicationError::ResourceLimit("materialized_publications"),
                ));
            }
            let publication = RawPublication::from_row(row)?;
            publication.validate_text_lengths()?;
            publications.push(publication);
        }
        drop(rows);
        drop(statement);
        let inventory = if publications.is_empty() {
            None
        } else {
            Some(ValidatedOperationInventory::load_with_budget(
                &transaction,
                manifest.project_id,
                budget,
            )?)
        };
        for row in &publications {
            if let Some(text_bytes) =
                preflight_baseline_by_hash(&transaction, &row.baseline_identity_hash)?
            {
                budget
                    .charge(
                        BudgetKind::WorkingSetBytes,
                        metadata_charge_bytes(text_bytes, 256)?,
                    )
                    .map_err(budget_error)?;
            }
            let baseline_row = baseline_query(&transaction, &row.baseline_identity_hash)?
                .ok_or_else(|| {
                    StoreError::Materialization(PublicationError::Corrupt(
                        "publication points to missing baseline registration".into(),
                    ))
                })?;
            let _ = validate_artifacts_with_budget(
                inventory.as_ref(),
                &transaction,
                self,
                &manifest,
                row,
                &baseline_row,
                verified_baseline.as_ref(),
                budget,
            )?;
        }
        let (state_count, state_text_bytes) = preflight_state_query(&transaction)?;
        if state_count == 1 {
            budget
                .charge(
                    BudgetKind::WorkingSetBytes,
                    metadata_charge_bytes(state_text_bytes, 128)?,
                )
                .map_err(budget_error)?;
        }
        if let Some(state) = state_query(&transaction)? {
            let row = publications
                .iter()
                .find(|row| row.publication_id == state.publication_id)
                .ok_or_else(|| {
                    StoreError::Materialization(PublicationError::Corrupt(
                        "materialization state publication is absent".into(),
                    ))
                })?;
            validate_state(&manifest, &state, row)?;
            if publications.iter().any(|publication| {
                publication.operation_project_revision > state.operation_project_revision
            }) {
                return Err(StoreError::Materialization(PublicationError::Corrupt(
                    "materialization current pointer precedes published operation history".into(),
                )));
            }
        } else if !publications.is_empty() {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "materialized publication history has no current pointer".into(),
            )));
        }
        budget.check_cancelled().map_err(budget_error)?;
        transaction.commit()?;
        Ok(())
    }
}

fn identity_input_error(error: kyberia_materialization_identity::IdentityError) -> StoreError {
    use kyberia_materialization_identity::IdentityError;
    let error = match error {
        IdentityError::Cancelled
        | IdentityError::Operation(kyberia_operation_log::OperationError::Cancelled) => {
            return StoreError::Cancelled;
        }
        IdentityError::ResourceLimit(reason)
        | IdentityError::Operation(kyberia_operation_log::OperationError::ResourceLimit(reason)) => {
            PublicationError::ResourceLimit(reason)
        }
        other => PublicationError::Identity(other.to_string()),
    };
    StoreError::Materialization(error)
}

fn identity_validation_error(error: kyberia_materialization_identity::IdentityError) -> StoreError {
    use kyberia_materialization_identity::IdentityError;
    let error = match error {
        IdentityError::Cancelled
        | IdentityError::Operation(kyberia_operation_log::OperationError::Cancelled) => {
            return StoreError::Cancelled;
        }
        IdentityError::ResourceLimit(reason)
        | IdentityError::Operation(kyberia_operation_log::OperationError::ResourceLimit(reason)) => {
            PublicationError::ResourceLimit(reason)
        }
        other => PublicationError::Corrupt(other.to_string()),
    };
    StoreError::Materialization(error)
}

fn replay_validation_error(error: kyberia_causal_materializer::MaterializationError) -> StoreError {
    use kyberia_causal_materializer::MaterializationError as M;
    use kyberia_materialization_identity::IdentityError as I;
    use kyberia_operation_log::{MergeError as G, OperationError as O};
    let publication_error = match error {
        M::Cancelled
        | M::Identity(I::Cancelled)
        | M::Identity(I::Operation(O::Cancelled))
        | M::Merge(G::Cancelled)
        | M::Merge(G::Operation(O::Cancelled)) => {
            return StoreError::Cancelled;
        }
        M::ResourceLimit(reason)
        | M::Identity(I::ResourceLimit(reason))
        | M::Identity(I::Operation(O::ResourceLimit(reason)))
        | M::Merge(G::ResourceLimit(reason))
        | M::Merge(G::Operation(O::ResourceLimit(reason))) => {
            PublicationError::ResourceLimit(reason)
        }
        other => PublicationError::Corrupt(format!(
            "stored materialization cannot be replayed: {other:?}"
        )),
    };
    StoreError::Materialization(publication_error)
}

fn read_baseline_project(
    bundle: &Bundle,
    transaction: &Transaction<'_>,
    manifest: &BundleManifest,
) -> Result<Option<Project>> {
    let mut budget = default_materialization_verification_budget();
    Ok(
        read_verified_baseline(bundle, transaction, manifest, &mut budget)?
            .map(|baseline| baseline.project),
    )
}

fn charge_manifest_decode<H: CancellationHook>(
    transaction: &Transaction<'_>,
    budget: &mut ResourceBudget<H>,
) -> Result<()> {
    let length: i64 = transaction.query_row(
        "SELECT length(body) FROM bundle_manifest WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    let length = usize::try_from(length)
        .map_err(|_| StoreError::Corrupt("negative manifest length".into()))?;
    budget
        .charge(
            BudgetKind::WorkingSetBytes,
            decoded_working_set_bytes(length, 4, "manifest_decode_bytes")?,
        )
        .map_err(budget_error)
}

fn read_verified_baseline<H: CancellationHook>(
    bundle: &Bundle,
    transaction: &Transaction<'_>,
    manifest: &BundleManifest,
    budget: &mut ResourceBudget<H>,
) -> Result<Option<VerifiedBaseline>> {
    budget.check_cancelled().map_err(budget_error)?;
    let (baseline_count, baseline_text_bytes) = preflight_baseline_inventory(transaction)?;
    budget
        .charge(
            BudgetKind::WorkingSetBytes,
            metadata_charge_bytes(
                if baseline_count == 1 {
                    baseline_text_bytes
                } else {
                    0
                },
                256,
            )?,
        )
        .map_err(budget_error)?;
    if baseline_count > 1 {
        return Err(StoreError::Materialization(PublicationError::Corrupt(
            "baseline inventory must contain at most one registration for this project".into(),
        )));
    }
    let mut project = None;
    let mut baseline_statement = transaction.prepare(
            "SELECT baseline_identity_hash,project_id,artifact_hash,artifact_bytes,protocol_version,project_revision,logical_time,committed_utc_ms FROM materialization_baselines LIMIT 2",
        )?;
    let mut baseline_rows = baseline_statement.query([])?;
    if let Some(raw) = baseline_rows.next()? {
        budget.check_cancelled().map_err(budget_error)?;
        let row = RawBaseline::from_row(raw)?;
        row.validate_text_lengths()?;
        if baseline_rows.next()?.is_some()
            || parse_id(row.project_id.clone(), "baseline project id")? != manifest.project_id
        {
            return Err(StoreError::Materialization(PublicationError::Corrupt(
                "baseline inventory must contain at most one registration for this project".into(),
            )));
        }
        parse_hash(row.artifact_hash.clone(), "baseline artifact hash")?;
        let bytes = bounded_usize(
            row.artifact_bytes,
            MAX_ARTIFACT_BYTES as usize,
            "baseline bytes",
        )?;
        let entry = artifact_entry(
            manifest,
            &row.artifact_hash,
            ArtifactKind::MaterializationBaseline,
            BASELINE_MEDIA_TYPE,
            bytes,
        )?;
        budget
            .charge(
                BudgetKind::WorkingSetBytes,
                bytes.checked_add(128).ok_or_else(|| {
                    StoreError::Materialization(PublicationError::ResourceLimit(
                        "baseline_artifact_bytes",
                    ))
                })?,
            )
            .map_err(budget_error)?;
        let content = bundle.read_registered_artifact(&row.artifact_hash, &entry)?;
        budget
            .charge(
                BudgetKind::WorkingSetBytes,
                decoded_working_set_bytes(content.len(), 4, "baseline_identity_decode_bytes")?,
            )
            .map_err(budget_error)?;
        let identity =
            BaselineIdentity::from_canonical_bytes(&content).map_err(identity_validation_error)?;
        validate_baseline_row(&identity, &row, &row.artifact_hash, bytes)?;
        budget
            .charge(BudgetKind::WorkingSetBytes, identity.project_bytes().len())
            .map_err(budget_error)?;
        project = Some(VerifiedBaseline {
            project: serde_json::from_slice(identity.project_bytes())?,
            identity,
        });
    }
    drop(baseline_rows);
    drop(baseline_statement);
    Ok(project)
}

#[cfg(test)]
mod publication_recovery_tests {
    use super::*;
    use kyberia_domain::identity::Text;

    #[test]
    fn baseline_registration_projection_fault_preserves_unregistered_state() {
        let retained = tempfile::tempdir().unwrap().keep();
        let root = retained.join("project");
        let baseline = Project::new(
            ProjectId::from_bytes([15; 16]).unwrap(),
            Text::new("Baseline recovery").unwrap(),
        );
        let mut bundle =
            Bundle::create(&root, baseline.id(), "Baseline recovery".into(), 1).unwrap();
        let before = bundle.manifest().unwrap();
        assert!(matches!(
            bundle.register_materialization_baseline_inner(&baseline, 2, true),
            Err(StoreError::Materialization(PublicationError::TestFault))
        ));
        drop(bundle);
        let mut reopened = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
        assert_eq!(reopened.manifest().unwrap(), before);
        assert!(reopened.materialization_baseline().unwrap().is_none());
        assert!(!reopened.verify().unwrap().projection_current);
        reopened.recover_manifest().unwrap();
        reopened
            .register_materialization_baseline(&baseline, 3)
            .unwrap();
        assert_eq!(reopened.materialization_baseline().unwrap(), Some(baseline));
        assert!(reopened.verify().unwrap().failures.is_empty());
    }

    #[test]
    fn projection_fault_does_not_publish_uncommitted_project() {
        let retained = tempfile::tempdir().unwrap().keep();
        let root = retained.join("project");
        let baseline = Project::new(
            ProjectId::from_bytes([9; 16]).unwrap(),
            Text::new("Recovery").unwrap(),
        );
        let operations = OperationSet::empty(baseline.id());
        let result = kyberia_causal_materializer::materialize(&baseline, &operations).unwrap();
        let mut bundle = Bundle::create(&root, baseline.id(), "Recovery".into(), 1).unwrap();
        bundle
            .register_materialization_baseline(&baseline, 2)
            .unwrap();
        let before = bundle.manifest().unwrap();
        assert!(matches!(
            bundle.publish_materialized_project_after_projection_fault(
                &baseline,
                &operations,
                &result,
                ProjectVersion::new(0),
                3
            ),
            Err(StoreError::Materialization(PublicationError::TestFault))
        ));
        drop(bundle);
        let mut reopened = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
        assert_eq!(reopened.manifest().unwrap(), before);
        assert!(reopened.materialized_project().unwrap().is_none());
        assert!(!reopened.verify().unwrap().projection_current);
        reopened.recover_manifest().unwrap();
        assert!(reopened.verify().unwrap().failures.is_empty());
        assert!(matches!(
            reopened
                .publish_materialized_project(
                    &baseline,
                    &operations,
                    &result,
                    ProjectVersion::new(0),
                    4
                )
                .unwrap(),
            MaterializationPublicationOutcome::Published(_)
        ));
        assert_eq!(
            reopened.materialized_project().unwrap().unwrap().project(),
            &baseline
        );
    }
}

#[cfg(test)]
mod identity_admission_tests {
    use super::*;

    #[test]
    fn declared_baseline_size_exhaustion_is_not_reported_as_corruption() {
        let id = kyberia_domain::identity::ProjectId::from_bytes([42; 16]).unwrap();
        let project = Project::new(id, kyberia_domain::identity::Text::new("Budget").unwrap());
        let identity = BaselineIdentity::from_project(&project).unwrap();
        let mut bytes = identity.canonical_bytes().to_vec();
        let length_offset = b"KYBERIA\0PROJECT-BASELINE\0".len() + 1 + 16 + 8 + 8;
        bytes[length_offset..length_offset + 8].copy_from_slice(&u64::MAX.to_be_bytes());
        let error = BaselineIdentity::from_canonical_bytes(&bytes).unwrap_err();
        assert!(matches!(
            identity_validation_error(error),
            StoreError::Materialization(PublicationError::ResourceLimit(_))
        ));
        assert!(matches!(
            identity_validation_error(
                BaselineIdentity::from_canonical_bytes(b"invalid").unwrap_err()
            ),
            StoreError::Materialization(PublicationError::Corrupt(_))
        ));
    }
}

#[cfg(test)]
mod identity_error_category_tests {
    use super::*;
    use kyberia_materialization_identity::IdentityError;
    use kyberia_operation_log::OperationError;

    #[test]
    fn identity_boundaries_preserve_nested_and_direct_resource_errors() {
        for nested in [false, true] {
            let error = || {
                if nested {
                    IdentityError::Operation(OperationError::ResourceLimit("test_budget"))
                } else {
                    IdentityError::ResourceLimit("test_budget")
                }
            };
            for mapped in [
                identity_input_error(error()),
                identity_validation_error(error()),
            ] {
                assert!(matches!(
                    mapped,
                    StoreError::Materialization(PublicationError::ResourceLimit("test_budget"))
                ));
            }
        }
        assert!(matches!(
            identity_input_error(IdentityError::NonCanonicalEncoding),
            StoreError::Materialization(PublicationError::Identity(_))
        ));
        assert!(matches!(
            identity_validation_error(IdentityError::NonCanonicalEncoding),
            StoreError::Materialization(PublicationError::Corrupt(_))
        ));
    }
}

#[cfg(test)]
mod metadata_preflight_tests {
    use super::*;
    use kyberia_resource_budget::{ResourceBudget, ResourceLimits};
    use rusqlite::{Connection, Transaction, TransactionBehavior, params};

    fn unlimited_budget() -> ResourceBudget {
        ResourceBudget::new(ResourceLimits::new(
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
        ))
    }

    #[test]
    fn publication_scalar_preflight_rejects_oversized_text_before_row_decode() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(sqlite_guard::CREATE_MATERIALIZED_PROJECT_PUBLICATIONS)
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        let publication_id = "a".repeat(PUBLICATION_ID_TEXT_BYTES);
        // SQLite length(TEXT) counts characters, so this remains within the
        // legacy character check while exceeding the byte contract enforced by
        // the preflight query.
        let oversized_project_id = "é".repeat(PROJECT_ID_TEXT_BYTES);
        let hash = "b".repeat(CONTENT_HASH_TEXT_BYTES);
        connection
            .execute(
                "INSERT INTO materialized_project_publications VALUES (?1,?2,1,1,?3,?3,1,?3,0,0,0,0,0,?3,1,0,0,1,1)",
                params![publication_id, oversized_project_id, hash],
            )
            .unwrap();
        let transaction =
            Transaction::new_unchecked(&connection, TransactionBehavior::Deferred).unwrap();
        let error = preflight_publication_by_id(&transaction, &publication_id).unwrap_err();
        assert!(matches!(
            error,
            StoreError::Materialization(PublicationError::Corrupt(message))
                if message.contains("publication project id exceeds")
        ));
    }

    #[test]
    fn baseline_and_state_scalar_preflights_reject_oversized_text_before_row_decode() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(sqlite_guard::CREATE_MATERIALIZATION_BASELINES)
            .unwrap();
        connection
            .execute_batch(sqlite_guard::CREATE_MATERIALIZED_PROJECT_STATE)
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        let hash = "c".repeat(CONTENT_HASH_TEXT_BYTES);
        let oversized_project_id = "x".repeat(PROJECT_ID_TEXT_BYTES + 1_024);
        connection
            .execute(
                "INSERT INTO materialization_baselines VALUES (?1,?2,?3,1,1,0,0,1)",
                params![hash, oversized_project_id, hash],
            )
            .unwrap();
        let oversized_state_project_id = "y".repeat(PROJECT_ID_TEXT_BYTES + 1_024);
        let publication_id = "d".repeat(PUBLICATION_ID_TEXT_BYTES);
        connection
            .execute(
                "INSERT INTO materialized_project_state VALUES (1,?1,?2,0,1)",
                params![oversized_state_project_id, publication_id],
            )
            .unwrap();
        let transaction =
            Transaction::new_unchecked(&connection, TransactionBehavior::Deferred).unwrap();
        let baseline_error = preflight_baseline_by_hash(&transaction, &hash).unwrap_err();
        assert!(matches!(
            baseline_error,
            StoreError::Materialization(PublicationError::Corrupt(message))
                if message.contains("baseline project id exceeds")
        ));
        let state_error = preflight_state_query(&transaction).unwrap_err();
        assert!(matches!(
            state_error,
            StoreError::Materialization(PublicationError::Corrupt(message))
                if message.contains("state project id exceeds")
        ));
    }

    #[test]
    fn publication_inventory_preflight_charges_only_scalar_metadata() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(sqlite_guard::CREATE_MATERIALIZED_PROJECT_PUBLICATIONS)
            .unwrap();
        let publication_id = "e".repeat(PUBLICATION_ID_TEXT_BYTES);
        let project_id = "f".repeat(PROJECT_ID_TEXT_BYTES);
        let hash = "1".repeat(CONTENT_HASH_TEXT_BYTES);
        connection
            .execute(
                "INSERT INTO materialized_project_publications VALUES (?1,?2,1,1,?3,?3,1,?3,0,0,0,0,0,?3,1,0,0,1,1)",
                params![publication_id, project_id, hash],
            )
            .unwrap();
        let transaction =
            Transaction::new_unchecked(&connection, TransactionBehavior::Deferred).unwrap();
        let mut budget = unlimited_budget();
        assert_eq!(
            preflight_publications(&transaction, &mut budget).unwrap(),
            1
        );
        assert!(budget.usage().working_set_bytes() >= 256);
    }
}
