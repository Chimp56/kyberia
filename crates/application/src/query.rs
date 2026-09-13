use crate::error::{ApplicationError, ErrorKind, map_budget_error};
use kyberia_domain::{identity::ProjectId, project::Project, project::ProjectSchemaVersion};
use kyberia_project_store::CanonicalProjectSnapshot;
use kyberia_resource_budget::{CancellationHook, ResourceBudget, ResourceLimits};

/// Aggregate limits for one application snapshot query. The same budget is
/// passed through publication-history verification so a long history cannot
/// reset replay work once per publication.
pub(crate) const fn default_query_limits() -> ResourceLimits {
    ResourceLimits::new(
        16_000_000,
        16_000_000,
        8_000_000,
        64 * 1024 * 1024,
        64 * 1024 * 1024,
        128 * 1024 * 1024,
    )
}

/// Queries are read-only and never mutate a project session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectQuery {
    CurrentSnapshot,
}

/// Result of a typed project query.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectQueryResult {
    CurrentSnapshot(CurrentProjectView),
}

/// Canonical lifecycle state observed by a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectState {
    MaterializedCurrent,
    BaselineOnly,
    LegacyAbsent,
}

/// Revision counters returned together because they came from one committed
/// store snapshot. Each counter has a distinct meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectRevision {
    bundle_revision: u64,
    project_revision: u64,
    logical_time: u64,
    operation_revision: Option<u64>,
    operation_count: Option<usize>,
    publication_bundle_revision: Option<u64>,
}

impl ProjectRevision {
    pub const fn bundle_revision(self) -> u64 {
        self.bundle_revision
    }
    pub const fn project_revision(self) -> u64 {
        self.project_revision
    }
    pub const fn logical_time(self) -> u64 {
        self.logical_time
    }
    pub const fn operation_revision(self) -> Option<u64> {
        self.operation_revision
    }
    pub const fn operation_count(self) -> Option<usize> {
        self.operation_count
    }
    pub const fn publication_bundle_revision(self) -> Option<u64> {
        self.publication_bundle_revision
    }
}

/// Immutable application projection of one canonical project view.
#[derive(Clone, Debug, PartialEq)]
pub struct CurrentProjectView {
    project_id: ProjectId,
    manifest_name: String,
    state: ProjectState,
    schema_version: Option<ProjectSchemaVersion>,
    project: Option<Project>,
    revision: Option<ProjectRevision>,
}

impl CurrentProjectView {
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub fn manifest_name(&self) -> &str {
        &self.manifest_name
    }

    pub const fn state(&self) -> ProjectState {
        self.state
    }

    pub const fn schema_version(&self) -> Option<ProjectSchemaVersion> {
        self.schema_version
    }

    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    pub const fn revision(&self) -> Option<ProjectRevision> {
        self.revision
    }
}

pub(crate) fn snapshot_to_view(
    snapshot: CanonicalProjectSnapshot,
) -> Result<CurrentProjectView, ApplicationError> {
    let CanonicalProjectSnapshot {
        manifest,
        baseline,
        current,
    } = snapshot;
    if manifest.schema_version != 1 || !manifest.required_features.is_empty() {
        return Err(ApplicationError::new(
            ErrorKind::UnsupportedVersion,
            format!(
                "unsupported project schema {} or required feature set",
                manifest.schema_version
            ),
        ));
    }
    if let Some(project) = baseline.as_ref()
        && (project.id() != manifest.project_id || project.name().as_str() != manifest.name)
    {
        return Err(ApplicationError::new(
            ErrorKind::CorruptProject,
            "canonical baseline does not match bundle identity or name",
        ));
    }

    let (state, project, schema_version, revision) = match (baseline, current) {
        (Some(baseline), None) => {
            let baseline_schema_version = baseline.schema_version();
            let revision = ProjectRevision {
                bundle_revision: manifest.revision,
                project_revision: baseline.revision(),
                logical_time: baseline.logical_time(),
                operation_revision: None,
                operation_count: None,
                publication_bundle_revision: None,
            };
            (
                ProjectState::BaselineOnly,
                Some(baseline),
                Some(baseline_schema_version),
                Some(revision),
            )
        }
        (Some(_baseline), Some(current)) => {
            let receipt = current.receipt();
            let current_schema_version = current.project().schema_version();
            if current.project().id() != manifest.project_id
                || receipt.project_id() != manifest.project_id
                || receipt.bundle_revision() > manifest.revision
                || current.project().revision() != receipt.materialized_project_revision()
                || current.project().logical_time() != receipt.materialized_logical_time()
            {
                return Err(ApplicationError::new(
                    ErrorKind::CorruptProject,
                    "current publication is inconsistent with the canonical snapshot",
                ));
            }
            let revision = ProjectRevision {
                bundle_revision: manifest.revision,
                project_revision: current.project().revision(),
                logical_time: current.project().logical_time(),
                operation_revision: Some(receipt.operation_project_revision().value()),
                operation_count: Some(receipt.operation_count()),
                publication_bundle_revision: Some(receipt.bundle_revision()),
            };
            (
                ProjectState::MaterializedCurrent,
                Some(current.into_project()),
                Some(current_schema_version),
                Some(revision),
            )
        }
        (None, None) => (ProjectState::LegacyAbsent, None, None, None),
        (None, Some(_)) => {
            return Err(ApplicationError::new(
                ErrorKind::CorruptProject,
                "current publication exists without a canonical baseline",
            ));
        }
    };

    Ok(CurrentProjectView {
        project_id: manifest.project_id,
        manifest_name: manifest.name,
        state,
        schema_version,
        project,
        revision,
    })
}

/// Run one query with one cumulative resource budget.
pub(crate) fn query_with_budget<T: crate::port::ProjectStorePort + ?Sized, H: CancellationHook>(
    store: &T,
    query: ProjectQuery,
    budget: &mut ResourceBudget<H>,
) -> Result<ProjectQueryResult, ApplicationError> {
    budget.check_cancelled().map_err(map_budget_error)?;
    let result = match query {
        ProjectQuery::CurrentSnapshot => {
            ProjectQueryResult::CurrentSnapshot(store.current_snapshot_with_budget(budget)?)
        }
    };
    budget.check_cancelled().map_err(map_budget_error)?;
    Ok(result)
}

/// A cancellation hook borrowed from the application caller. Storage never
/// owns the source; the budget only polls it at deterministic boundaries.
pub(crate) struct BorrowedCancellation<'a, H: CancellationHook> {
    hook: &'a mut H,
}

impl<H: CancellationHook> CancellationHook for BorrowedCancellation<'_, H> {
    fn is_cancelled(&mut self) -> bool {
        self.hook.is_cancelled()
    }
}

/// Check cancellation at the application boundary around one store query.
/// The hook is also carried by the cumulative verification budget, preserving
/// cancellation if the bounded store operation performs multiple checks.
pub(crate) fn query_with_cancel<T: crate::port::ProjectStorePort + ?Sized>(
    store: &T,
    query: ProjectQuery,
    cancel: &mut impl CancellationHook,
) -> Result<ProjectQueryResult, ApplicationError> {
    let mut budget = ResourceBudget::with_cancellation(
        default_query_limits(),
        BorrowedCancellation { hook: cancel },
    );
    query_with_budget(store, query, &mut budget)
}
