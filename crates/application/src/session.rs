use crate::{
    command::SessionMode,
    error::ApplicationError,
    port::BundleProjectStore,
    query::{
        ProjectQuery, ProjectQueryResult, default_query_limits, query_with_budget,
        query_with_cancel,
    },
};
use kyberia_resource_budget::{CancellationHook, ResourceBudget};

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
        let mut budget = ResourceBudget::new(default_query_limits());
        query_with_budget(&self.store, query, &mut budget)
    }

    /// Run a query with a caller-owned cumulative resource budget. The budget
    /// covers the store's complete publication verification pass and remains
    /// available for future application query work in the same operation.
    pub fn query_with_budget<H: CancellationHook>(
        &self,
        query: ProjectQuery,
        budget: &mut ResourceBudget<H>,
    ) -> Result<ProjectQueryResult, ApplicationError> {
        query_with_budget(&self.store, query, budget)
    }

    pub fn query_with_cancel(
        &self,
        query: ProjectQuery,
        cancel: &mut impl CancellationHook,
    ) -> Result<ProjectQueryResult, ApplicationError> {
        query_with_cancel(&self.store, query, cancel)
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
