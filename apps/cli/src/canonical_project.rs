//! Read-only inspection of the verified canonical project state.
//!
//! The bundle manifest remains metadata owned by the storage adapter.  This
//! command reports it alongside the domain project and the publication receipt
//! so callers cannot confuse the bundle revision, operation revision, causal
//! depth, or aggregate revision.  A missing canonical baseline is reported as
//! an explicit legacy state; no project is reconstructed from manifest fields.

use kyberia_domain::{identity::ProjectId, project::Project};
use kyberia_project_store::{
    Bundle, LoadedMaterializedProject, MaterializationPublicationReceipt, OpenMode, StoreError,
};
use serde::Serialize;
use std::{fmt, path::Path};

const QUERY_SCHEMA: &str = "kyberia.canonical-project-query/1";

#[derive(Debug)]
pub enum CanonicalProjectError {
    Store(StoreError),
    ManifestProjectMismatch,
    ManifestNameMismatch,
}

impl fmt::Display for CanonicalProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::ManifestProjectMismatch => {
                formatter.write_str("canonical baseline project ID differs from bundle manifest")
            }
            Self::ManifestNameMismatch => {
                formatter.write_str("canonical baseline name differs from bundle manifest")
            }
        }
    }
}

impl std::error::Error for CanonicalProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::ManifestProjectMismatch | Self::ManifestNameMismatch => None,
        }
    }
}

impl From<StoreError> for CanonicalProjectError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CanonicalProjectState {
    MaterializedCurrent,
    BaselineOnly,
    LegacyAbsent,
}

#[derive(Debug, Serialize)]
pub(crate) struct ManifestView {
    project_id: ProjectId,
    name: String,
    /// Bundle metadata revision. This is independent of operation and project
    /// revisions reported in `publication` and `project`.
    bundle_revision: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct BaselineView {
    project_revision: u64,
    logical_time: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicationView {
    publication_id: String,
    baseline_identity_hash: String,
    operation_set_identity_hash: String,
    baseline_artifact_hash: String,
    materialized_artifact_hash: String,
    baseline_project_revision: u64,
    baseline_logical_time: u64,
    materialized_project_revision: u64,
    materialized_logical_time: u64,
    operation_count: usize,
    operation_project_revision: u64,
    operation_max_causal_depth: u64,
    protocol_version: u32,
    result_schema_version: kyberia_domain::project::ProjectSchemaVersion,
    publication_bundle_revision: u64,
    committed_utc_ms: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct CanonicalProjectReport {
    pub schema: &'static str,
    pub state: CanonicalProjectState,
    pub manifest: ManifestView,
    /// The verified baseline or current materialized project. This is absent
    /// for a legacy bundle without a registered canonical baseline.
    pub project: Option<Project>,
    pub baseline: Option<BaselineView>,
    pub publication: Option<PublicationView>,
}

impl From<&MaterializationPublicationReceipt> for PublicationView {
    fn from(receipt: &MaterializationPublicationReceipt) -> Self {
        Self {
            publication_id: receipt.publication_id().to_owned(),
            baseline_identity_hash: receipt.baseline_identity_hash().into(),
            operation_set_identity_hash: receipt.operation_set_identity_hash().into(),
            baseline_artifact_hash: receipt.baseline_artifact_hash().to_owned(),
            materialized_artifact_hash: receipt.materialized_artifact_hash().to_owned(),
            baseline_project_revision: receipt.baseline_project_revision(),
            baseline_logical_time: receipt.baseline_logical_time(),
            materialized_project_revision: receipt.materialized_project_revision(),
            materialized_logical_time: receipt.materialized_logical_time(),
            operation_count: receipt.operation_count(),
            operation_project_revision: receipt.operation_project_revision().value(),
            operation_max_causal_depth: receipt.operation_max_causal_depth(),
            protocol_version: receipt.protocol_version(),
            result_schema_version: receipt.result_schema_version(),
            publication_bundle_revision: receipt.bundle_revision(),
            committed_utc_ms: receipt.committed_utc_ms(),
        }
    }
}

fn baseline_view(project: &Project) -> BaselineView {
    BaselineView {
        project_revision: project.revision(),
        logical_time: project.logical_time(),
    }
}

fn validate_baseline_binding(
    project: &Project,
    project_id: ProjectId,
    manifest_name: &str,
) -> Result<(), CanonicalProjectError> {
    if project.id() != project_id {
        return Err(CanonicalProjectError::ManifestProjectMismatch);
    }
    if project.name().as_str() != manifest_name {
        return Err(CanonicalProjectError::ManifestNameMismatch);
    }
    Ok(())
}

/// Query canonical state without opening a writable handle or changing the
/// bundle. The storage APIs validate immutable artifact bytes, publication
/// rows, current pointers, and baseline identity before returning projects.
pub fn query(path: &Path) -> Result<CanonicalProjectReport, CanonicalProjectError> {
    let bundle = Bundle::open(path, OpenMode::ReadOnly)?;
    let manifest = bundle.manifest()?;
    let baseline = bundle.materialization_baseline()?;
    let current = bundle.materialized_project()?;

    if let Some(project) = baseline.as_ref() {
        validate_baseline_binding(project, manifest.project_id, &manifest.name)?;
    }

    let (state, project, baseline_view, publication) = match (baseline, current) {
        (Some(baseline), None) => (
            CanonicalProjectState::BaselineOnly,
            Some(baseline.clone()),
            Some(baseline_view(&baseline)),
            None,
        ),
        (Some(baseline), Some(loaded)) => {
            validate_current_binding(&loaded, manifest.project_id)?;
            (
                CanonicalProjectState::MaterializedCurrent,
                Some(loaded.project().clone()),
                Some(baseline_view(&baseline)),
                Some(PublicationView::from(loaded.receipt())),
            )
        }
        (None, None) => (CanonicalProjectState::LegacyAbsent, None, None, None),
        (None, Some(_)) => {
            return Err(CanonicalProjectError::Store(StoreError::Corrupt(
                "materialized publication exists without a canonical baseline".into(),
            )));
        }
    };

    Ok(CanonicalProjectReport {
        schema: QUERY_SCHEMA,
        state,
        manifest: ManifestView {
            project_id: manifest.project_id,
            name: manifest.name,
            bundle_revision: manifest.revision,
        },
        project,
        baseline: baseline_view,
        publication,
    })
}

fn validate_current_binding(
    loaded: &LoadedMaterializedProject,
    project_id: ProjectId,
) -> Result<(), CanonicalProjectError> {
    if loaded.project().id() != project_id || loaded.receipt().project_id() != project_id {
        return Err(CanonicalProjectError::ManifestProjectMismatch);
    }
    Ok(())
}
