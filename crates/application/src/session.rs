use crate::{
    command::SessionMode,
    error::ApplicationError,
    port::BundleProjectStore,
    query::{ProjectQuery, ProjectQueryResult, query_with_cancel},
};
use kyberia_resource_budget::{CancellationHook, ResourceBudgetError};

/// A typed application session. The storage adapter is intentionally private;
/// all reads cross the application query boundary.
pub struct ProjectSession {
    store: BundleProjectStore,
    mode: SessionMode,
}

impl ProjectSession {
    pub(crate) fn new(store: BundleProjectStore, mode: SessionMode) -> Self {
        Self { store, mode }
    }

    pub const fn mode(&self) -> SessionMode {
        self.mode
    }

    pub fn query(&self, query: ProjectQuery) -> Result<ProjectQueryResult, ApplicationError> {
        let mut cancel = NeverCancel;
        query_with_cancel(&self.store, query, &mut cancel)
    }

    pub fn query_with_cancel(
        &self,
        query: ProjectQuery,
        cancel: &mut impl CancellationHook,
    ) -> Result<ProjectQueryResult, ApplicationError> {
        query_with_cancel(&self.store, query, cancel)
    }
}

struct NeverCancel;

impl CancellationHook for NeverCancel {
    fn is_cancelled(&mut self) -> bool {
        false
    }
}

impl std::fmt::Debug for ProjectSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProjectSession")
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

impl From<ResourceBudgetError> for ApplicationError {
    fn from(error: ResourceBudgetError) -> Self {
        match error {
            ResourceBudgetError::Cancelled => Self::new(
                crate::ErrorKind::Cancelled,
                "project query resource budget cancelled",
            ),
            ResourceBudgetError::LimitExceeded(limit) => Self::new(
                crate::ErrorKind::ResourceLimit,
                format!("project query resource limit: {}", limit.kind().label()),
            ),
        }
    }
}
