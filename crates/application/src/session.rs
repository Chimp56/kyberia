use crate::{
    command::SessionMode,
    error::ApplicationError,
    map_asset::admit_map_asset,
    map_mutation::{CalibrateMapRequest, ImportMapRequest, MapMutationReceipt},
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

    /// Admit, content-address and operation-publish one immutable PNG map.
    pub fn import_map_with_budget<H: CancellationHook>(
        &mut self,
        request: ImportMapRequest,
        bytes: &[u8],
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationReceipt, ApplicationError> {
        if self.mode != SessionMode::ReadWrite {
            return Err(ApplicationError::new(
                crate::ErrorKind::ReadOnly,
                "project session is read-only",
            ));
        }
        let admitted = admit_map_asset(bytes, budget)?;
        self.store.import_map(request, admitted, bytes, budget)
    }

    /// Append and publish a full two-point calibration operation.
    pub fn calibrate_map_with_budget<H: CancellationHook>(
        &mut self,
        request: CalibrateMapRequest,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationReceipt, ApplicationError> {
        if self.mode != SessionMode::ReadWrite {
            return Err(ApplicationError::new(
                crate::ErrorKind::ReadOnly,
                "project session is read-only",
            ));
        }
        self.store.calibrate_map(request, budget)
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
