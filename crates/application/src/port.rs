use crate::{
    command::SessionMode,
    error::{ApplicationError, StoreContext, map_budget_error, map_store_error},
    query::{CurrentProjectView, snapshot_to_view},
};
use kyberia_domain::{identity::ProjectId, project::Project};
use kyberia_project_store::{Bundle, CanonicalProjectSnapshot, OpenMode};
use kyberia_resource_budget::{CancellationHook, ResourceBudget};
use std::{fs, path::Path};

/// Inward port used by query/session orchestration. It returns only
/// application-owned canonical views, so adapter schemas cannot leak outward.
pub trait ProjectStorePort {
    fn current_snapshot_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<CurrentProjectView, ApplicationError>;
}

/// Production adapter for the reviewed `kyberia-project-store` APIs. This type
/// is crate-private; callers obtain it only through typed lifecycle commands.
pub(crate) struct BundleProjectStore {
    bundle: Bundle,
}

impl BundleProjectStore {
    pub(crate) fn create(
        path: &Path,
        project_id: ProjectId,
        name: String,
        utc_ms: i64,
    ) -> Result<Self, ApplicationError> {
        Bundle::create(path, project_id, name, utc_ms)
            .map(|bundle| Self { bundle })
            .map_err(|error| {
                if matches!(
                    &error,
                    kyberia_project_store::StoreError::Io(io_error)
                        if io_error.kind() == std::io::ErrorKind::AlreadyExists
                ) {
                    ApplicationError::already_exists(path)
                } else {
                    map_store_error(StoreContext::Create, error)
                }
            })
    }

    pub(crate) fn open(path: &Path, mode: SessionMode) -> Result<Self, ApplicationError> {
        match fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ApplicationError::missing_project(path));
            }
            Err(error) => {
                return Err(map_store_error(StoreContext::Open, error.into()));
            }
        }
        let open_mode = match mode {
            SessionMode::ReadOnly => OpenMode::ReadOnly,
            SessionMode::ReadWrite => OpenMode::ReadWrite,
        };
        let bundle = Bundle::open(path, open_mode)
            .map_err(|error| map_store_error(StoreContext::Open, error))?;
        let manifest = bundle
            .manifest()
            .map_err(|error| map_store_error(StoreContext::Open, error))?;
        if manifest.schema_version != 1 || !manifest.required_features.is_empty() {
            return Err(ApplicationError::new(
                crate::ErrorKind::UnsupportedVersion,
                format!(
                    "unsupported project schema {} or required feature set",
                    manifest.schema_version
                ),
            ));
        }
        Ok(Self { bundle })
    }

    pub(crate) fn register_baseline(
        &mut self,
        baseline: &Project,
        utc_ms: i64,
    ) -> Result<(), ApplicationError> {
        self.bundle
            .register_materialization_baseline(baseline, utc_ms)
            .map(|_| ())
            .map_err(|error| map_store_error(StoreContext::Baseline, error))
    }
}

impl ProjectStorePort for BundleProjectStore {
    fn current_snapshot_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<CurrentProjectView, ApplicationError> {
        budget.check_cancelled().map_err(map_budget_error)?;
        // A true legacy read-only bundle can omit the additive materialization
        // table group. The canonical snapshot API already represents that
        // shape as `baseline=None,current=None`; do not force the verifier,
        // whose contract requires those tables, to reject it.
        let snapshot = self
            .bundle
            .canonical_project_snapshot()
            .map_err(|error| map_store_error(StoreContext::Query, error))?;
        if snapshot.baseline.is_none() && snapshot.current.is_none() {
            return snapshot_to_view(snapshot);
        }
        self.bundle
            .verify_materialized_project_publication_with_budget(budget)
            .map_err(|error| map_store_error(StoreContext::Query, error))?;
        let snapshot: CanonicalProjectSnapshot = self
            .bundle
            .canonical_project_snapshot()
            .map_err(|error| map_store_error(StoreContext::Query, error))?;
        snapshot_to_view(snapshot)
    }
}
