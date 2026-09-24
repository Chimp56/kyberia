use crate::{
    command::SessionMode,
    error::ApplicationError,
    map_asset::admit_map_asset,
    map_mutation::{
        CalibrateMapIntent, CalibrateMapRequest, ImportMapIntent, ImportMapRequest,
        MapMutationOutcome, MapMutationReceipt,
    },
    port::BundleProjectStore,
    query::{
        ProjectQuery, ProjectQueryResult, default_query_limits, query_with_budget,
        query_with_cancel,
    },
    survey_snapshot::{
        LoadedPointSurveySnapshot, PointSurveySnapshotHistoryCursor,
        PointSurveySnapshotHistoryPage, PointSurveySnapshotHistoryPageLimits,
        PointSurveySnapshotReceipt, PointSurveySnapshotRequest,
    },
};
use kyberia_domain::identity::{SessionId, SnapshotId};
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

    /// Persist one immutable, validated point-survey snapshot.
    pub fn save_point_survey_snapshot(
        &mut self,
        request: PointSurveySnapshotRequest,
    ) -> Result<PointSurveySnapshotReceipt, ApplicationError> {
        if self.mode != SessionMode::ReadWrite {
            return Err(ApplicationError::new(
                crate::ErrorKind::ReadOnly,
                "project session is read-only",
            ));
        }
        self.store.save_point_survey_snapshot(request)
    }

    /// Load and replay-validate one snapshot, optionally requiring its survey
    /// session identity to match the caller's expected session.
    pub fn load_point_survey_snapshot(
        &self,
        snapshot_id: SnapshotId,
        expected_session: Option<SessionId>,
    ) -> Result<LoadedPointSurveySnapshot, ApplicationError> {
        self.store
            .load_point_survey_snapshot(snapshot_id, expected_session)
    }

    /// Return the complete snapshot history when it fits in one default page.
    ///
    /// This compatibility convenience is strictly bounded: if another page
    /// exists it returns `ResourceLimit` rather than exposing partial
    /// history. Use `list_point_survey_snapshot_history_page` to traverse
    /// larger histories.
    pub fn list_point_survey_snapshot_history(
        &self,
        session_id: Option<SessionId>,
    ) -> Result<Vec<crate::PointSurveySnapshotHistoryEntry>, ApplicationError> {
        let mut cancel = kyberia_resource_budget::NeverCancel;
        let page = self.list_point_survey_snapshot_history_page(
            session_id,
            None,
            PointSurveySnapshotHistoryPageLimits::default(),
            &mut cancel,
        )?;
        if page.next_cursor().is_some() {
            return Err(ApplicationError::new(
                crate::ErrorKind::ResourceLimit,
                "survey snapshot history exceeds the single-page compatibility limit; use cursor pages",
            ));
        }
        Ok(page.entries().to_vec())
    }

    /// Return one bounded, replay-validated history page with cooperative
    /// cancellation. Continue by passing the returned cursor and same session
    /// filter; the cursor pins traversal to the first page's bundle revision.
    pub fn list_point_survey_snapshot_history_page<H: CancellationHook>(
        &self,
        session_id: Option<SessionId>,
        cursor: Option<PointSurveySnapshotHistoryCursor>,
        limits: PointSurveySnapshotHistoryPageLimits,
        cancel: &mut H,
    ) -> Result<PointSurveySnapshotHistoryPage, ApplicationError> {
        self.store
            .list_point_survey_snapshot_history_page(session_id, cursor, limits, cancel)
    }

    /// Admit and durably import one PNG using application-derived causal
    /// metadata, then attempt canonical readback without losing the receipt.
    pub fn import_map_intent_with_budget<H: CancellationHook>(
        &mut self,
        intent: ImportMapIntent,
        bytes: &[u8],
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationOutcome, ApplicationError> {
        if self.mode != SessionMode::ReadWrite {
            return Err(ApplicationError::new(
                crate::ErrorKind::ReadOnly,
                "project session is read-only",
            ));
        }
        let admitted = admit_map_asset(bytes, budget)?;
        self.store
            .import_map_intent(intent, admitted, bytes, budget)
    }

    /// Persist a two-point calibration using application-derived causal
    /// metadata, then return its receipt even if canonical readback fails.
    pub fn calibrate_map_intent_with_budget<H: CancellationHook>(
        &mut self,
        intent: CalibrateMapIntent,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationOutcome, ApplicationError> {
        if self.mode != SessionMode::ReadWrite {
            return Err(ApplicationError::new(
                crate::ErrorKind::ReadOnly,
                "project session is read-only",
            ));
        }
        self.store.calibrate_map_intent(intent, budget)
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
