use crate::{
    command::SessionMode,
    error::ApplicationError,
    query::{CurrentProjectView, snapshot_to_view},
};
use kyberia_domain::{identity::ProjectId, project::Project};
use kyberia_project_store::{Bundle, CanonicalProjectSnapshot, OpenMode};
use std::path::Path;

/// Inward port used by query/session orchestration. It returns only
/// application-owned canonical views, so adapter schemas cannot leak outward.
pub trait ProjectStorePort {
    fn current_snapshot(&self) -> Result<CurrentProjectView, ApplicationError>;
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
                    error.into()
                }
            })
    }

    pub(crate) fn open(path: &Path, mode: SessionMode) -> Result<Self, ApplicationError> {
        let open_mode = match mode {
            SessionMode::ReadOnly => OpenMode::ReadOnly,
            SessionMode::ReadWrite => OpenMode::ReadWrite,
        };
        let bundle = Bundle::open(path, open_mode).map_err(|error| {
            if matches!(
                &error,
                kyberia_project_store::StoreError::Io(io_error)
                    if io_error.kind() == std::io::ErrorKind::NotFound
            ) {
                ApplicationError::missing_project(path)
            } else {
                error.into()
            }
        })?;
        let manifest = bundle.manifest().map_err(ApplicationError::from)?;
        if manifest.schema_version != 1 || !manifest.required_features.is_empty() {
            return Err(ApplicationError::new(
                crate::ErrorKind::UnsupportedVersion,
                format!("unsupported project schema {}", manifest.schema_version),
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
            .map_err(Into::into)
    }
}

impl ProjectStorePort for BundleProjectStore {
    fn current_snapshot(&self) -> Result<CurrentProjectView, ApplicationError> {
        let snapshot: CanonicalProjectSnapshot = self
            .bundle
            .canonical_project_snapshot()
            .map_err(ApplicationError::from)?;
        snapshot_to_view(snapshot)
    }
}
