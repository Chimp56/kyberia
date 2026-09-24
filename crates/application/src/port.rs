use crate::{
    command::SessionMode,
    error::{ApplicationError, StoreContext, map_budget_error, map_store_error},
    map_asset::{AdmittedMapAsset, PNG_MEDIA_TYPE},
    map_mutation::{
        CalibrateMapIntent, CalibrateMapRequest, ImportMapIntent, ImportMapRequest,
        MapIntentAuthority, MapMutationOutcome, MapMutationReceipt, MapOperationContext,
    },
    query::{CurrentProjectView, snapshot_to_view},
    survey_snapshot::{
        LoadedPointSurveySnapshot, PointSurveySnapshotHistoryEntry, PointSurveySnapshotReceipt,
        PointSurveySnapshotRequest, history_from_store, loaded_from_store, receipt_from_store,
    },
};
use kyberia_domain::{
    evidence::ArtifactReference,
    identity::{ContentHash, ProjectId, SessionId, SnapshotId, Text},
    project::{MapAsset, MapAssetData, Project},
};
use kyberia_operation_log::{
    CausalDepth, InverseMetadata, InversePrior, LogicalTimestamp, MAX_PARENTS, Mutation, Operation,
    OperationError, OperationPayload, OperationSet, ProjectVersion,
};
use kyberia_project_store::{
    ArtifactEntry, ArtifactKind, Bundle, CanonicalProjectSnapshot, OpenMode,
    OperationAppendOutcome, OperationStoreState,
};
use kyberia_resource_budget::{CancellationHook, ResourceBudget};
use std::{collections::BTreeSet, fs, path::Path};

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

    pub(crate) fn import_map<H: CancellationHook>(
        &mut self,
        request: ImportMapRequest,
        admitted: AdmittedMapAsset,
        bytes: &[u8],
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationReceipt, ApplicationError> {
        budget.check_cancelled().map_err(map_budget_error)?;
        crate::map_asset::validate_map_provenance(request.provenance.as_str())?;
        if request.context.committed_utc_ms < 0 {
            return Err(ApplicationError::new(
                crate::ErrorKind::InvalidRequest,
                "negative mutation timestamp",
            ));
        }
        let source = ArtifactReference {
            sha256: admitted.content_hash(),
            media_type: Text::new(PNG_MEDIA_TYPE).expect("static media type is valid"),
            byte_length: bytes.len() as u64,
        };
        let map = MapAsset::new(MapAssetData {
            id: request.map_id,
            floor_id: request.floor_id,
            name: request.name,
            image_frame: request.image_frame,
            width: admitted.width(),
            height: admitted.height(),
            source,
            provenance: request.provenance.clone(),
        })
        .map_err(|error| {
            ApplicationError::new(crate::ErrorKind::InvalidRequest, error.to_string())
        })?;
        let operation = Operation::try_apply_v3(
            request.context.operation_id,
            self.project_id()?,
            request.context.actor_id,
            request.context.device_id,
            request.context.logical_time,
            request.context.causal_depth,
            request.context.parents,
            Mutation::import_map(map),
            InversePrior::MapAbsent {
                map_id: request.map_id,
            },
        )
        .map_err(map_operation_input)?;

        let stored_hash = self
            .bundle
            .put_artifact(
                bytes,
                ArtifactEntry {
                    kind: ArtifactKind::MapSource,
                    bytes: bytes.len() as u64,
                    media_type: PNG_MEDIA_TYPE.into(),
                    provenance_id: request.provenance.as_str().into(),
                },
                request.context.committed_utc_ms,
            )
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        if stored_hash != String::from(admitted.content_hash()) {
            return Err(ApplicationError::new(
                crate::ErrorKind::Storage,
                "stored map hash mismatch",
            ));
        }
        budget.check_cancelled().map_err(map_budget_error)?;
        self.commit_operation(
            operation,
            request.context.expected_project_revision,
            request.context.committed_utc_ms,
            admitted.content_hash(),
            budget,
        )
    }

    pub(crate) fn import_map_intent<H: CancellationHook>(
        &mut self,
        intent: ImportMapIntent,
        admitted: AdmittedMapAsset,
        bytes: &[u8],
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationOutcome, ApplicationError> {
        budget.check_cancelled().map_err(map_budget_error)?;
        crate::map_asset::validate_map_provenance(intent.provenance.as_str())?;
        if intent.authority.committed_utc_ms < 0 {
            return Err(ApplicationError::new(
                crate::ErrorKind::InvalidRequest,
                "negative mutation timestamp",
            ));
        }
        let (project, operations, state) = self.canonical_map_state(budget)?;
        let map = admitted_map(&intent, admitted, bytes.len())?;
        if let Some(existing) = operations.operation(intent.authority.operation_id) {
            if !existing_import_matches(existing, project.id(), &intent, &map) {
                return Err(ApplicationError::new(
                    crate::ErrorKind::Conflict,
                    "operation identity already belongs to different immutable map intent bytes",
                ));
            }
            let stored_bytes = self
                .bundle
                .read_artifact(&String::from(admitted.content_hash()))
                .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
            if stored_bytes != bytes {
                return Err(ApplicationError::new(
                    crate::ErrorKind::Conflict,
                    "map retry bytes differ from the immutable artifact already imported",
                ));
            }
            let operation = existing.clone();
            return self.exact_duplicate_outcome(operation, admitted.content_hash(), budget);
        }
        let context = derive_map_context_from_set(
            &operations,
            &intent.authority,
            state.project_revision(),
            budget,
        )?;
        let mut request = ImportMapRequest {
            context,
            map_id: intent.map_id,
            floor_id: intent.floor_id,
            name: intent.name,
            image_frame: intent.image_frame,
            provenance: intent.provenance,
        };
        if project.floor(request.floor_id).is_none() {
            return Err(ApplicationError::new(
                crate::ErrorKind::Prerequisite,
                "map import floor does not exist in the canonical project",
            ));
        }
        let operation = build_import_operation(project.id(), &request, map)?;
        let stored_hash = self
            .bundle
            .put_artifact(
                bytes,
                ArtifactEntry {
                    kind: ArtifactKind::MapSource,
                    bytes: bytes.len() as u64,
                    media_type: PNG_MEDIA_TYPE.into(),
                    provenance_id: request.provenance.as_str().into(),
                },
                request.context.committed_utc_ms,
            )
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        if stored_hash != String::from(admitted.content_hash()) {
            return Err(ApplicationError::new(
                crate::ErrorKind::Storage,
                "stored map hash mismatch",
            ));
        }
        request.context.expected_project_revision = state.project_revision();
        self.commit_operation_with_outcome(
            operation,
            request.context.expected_project_revision,
            request.context.committed_utc_ms,
            admitted.content_hash(),
            budget,
        )
    }

    pub(crate) fn commit_operation<H: CancellationHook>(
        &mut self,
        operation: Operation,
        expected_revision: ProjectVersion,
        utc_ms: i64,
        content_hash: ContentHash,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationReceipt, ApplicationError> {
        let outcome = self.commit_operation_with_outcome(
            operation,
            expected_revision,
            utc_ms,
            content_hash,
            budget,
        )?;
        outcome.current?;
        Ok(outcome.receipt)
    }

    fn commit_operation_with_outcome<H: CancellationHook>(
        &mut self,
        operation: Operation,
        expected_revision: ProjectVersion,
        utc_ms: i64,
        content_hash: ContentHash,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationOutcome, ApplicationError> {
        budget.check_cancelled().map_err(map_budget_error)?;
        let baseline = self
            .bundle
            .materialization_baseline_with_budget(budget)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?
            .ok_or_else(|| {
                ApplicationError::new(
                    crate::ErrorKind::CorruptProject,
                    "missing materialization baseline",
                )
            })?;
        let operation_id = operation.operation_id();
        // Validate the candidate against the exact persisted DAG before any
        // operation append. Domain failures (including evidence-locked scale
        // changes) therefore cannot poison the immutable operation history.
        let existing = self
            .bundle
            .operation_set_with_budget(budget)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        let candidate = OperationSet::from_operations_with_budget(
            existing
                .operations()
                .cloned()
                .chain(std::iter::once(operation.clone())),
            budget,
        )
        .map_err(map_operation_input)?;
        kyberia_causal_materializer::materialize_with_budget(&baseline, &candidate, budget)
            .map_err(map_materialization_error)?;
        budget.check_cancelled().map_err(map_budget_error)?;
        let append = self
            .bundle
            .append_operation_if_revision_with_budget(operation, Some(expected_revision), budget)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        let revision = match append {
            OperationAppendOutcome::Appended {
                project_revision, ..
            }
            | OperationAppendOutcome::Duplicate {
                project_revision, ..
            } => project_revision,
        };
        let receipt = MapMutationReceipt {
            operation_id,
            project_revision: revision,
            content_hash,
        };
        let current = (|| {
            let operations = self
                .bundle
                .operation_set_with_budget(budget)
                .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
            let materialized = kyberia_causal_materializer::materialize_with_budget(
                &baseline,
                &operations,
                budget,
            )
            .map_err(map_materialization_error)?;
            budget.check_cancelled().map_err(map_budget_error)?;
            self.bundle
                .publish_materialized_project_with_budget(
                    &baseline,
                    &operations,
                    &materialized,
                    revision,
                    utc_ms,
                    budget,
                )
                .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
            let current = <Self as ProjectStorePort>::current_snapshot_with_budget(self, budget)?;
            if current.project() != Some(materialized.project()) {
                return Err(ApplicationError::new(
                    crate::ErrorKind::Storage,
                    "publication readback mismatch",
                ));
            }
            Ok(current)
        })();
        Ok(MapMutationOutcome { receipt, current })
    }

    pub(crate) fn calibrate_map<H: CancellationHook>(
        &mut self,
        request: CalibrateMapRequest,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationReceipt, ApplicationError> {
        budget.check_cancelled().map_err(map_budget_error)?;
        if request.context.committed_utc_ms < 0 {
            return Err(ApplicationError::new(
                crate::ErrorKind::InvalidRequest,
                "negative mutation timestamp",
            ));
        }
        let calibration_id = request.calibration.id;
        let map_id = request.calibration.map_id;
        let operation = Operation::try_apply_v3(
            request.context.operation_id,
            self.project_id()?,
            request.context.actor_id,
            request.context.device_id,
            request.context.logical_time,
            request.context.causal_depth,
            request.context.parents,
            Mutation::calibrate_map(request.calibration),
            InversePrior::CalibrationAbsent {
                calibration_id,
                map_id,
                active: request.prior_active,
            },
        )
        .map_err(map_operation_input)?;
        let content_hash = operation.content_hash();
        self.commit_operation(
            operation,
            request.context.expected_project_revision,
            request.context.committed_utc_ms,
            content_hash,
            budget,
        )
    }

    pub(crate) fn calibrate_map_intent<H: CancellationHook>(
        &mut self,
        intent: CalibrateMapIntent,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationOutcome, ApplicationError> {
        budget.check_cancelled().map_err(map_budget_error)?;
        if intent.authority.committed_utc_ms < 0 {
            return Err(ApplicationError::new(
                crate::ErrorKind::InvalidRequest,
                "negative mutation timestamp",
            ));
        }
        let (project, operations, state) = self.canonical_map_state(budget)?;
        let calibration_id = intent.calibration.id;
        let map_id = intent.calibration.map_id;
        if let Some(map) = project.map(map_id) {
            let controls = intent.calibration.transform.controls();
            let inside = |point: kyberia_domain::spatial::PixelPoint| {
                point.x.get() >= 0.0
                    && point.y.get() >= 0.0
                    && point.x.get() < f64::from(map.data().width.get())
                    && point.y.get() < f64::from(map.data().height.get())
            };
            if controls.source_frame != map.data().image_frame.id
                || project
                    .floor(map.data().floor_id)
                    .is_none_or(|floor| controls.target_frame != floor.data().frame.id)
                || !inside(controls.image_first)
                || !inside(controls.image_second)
            {
                return Err(ApplicationError::new(
                    crate::ErrorKind::InvalidRequest,
                    "calibration controls must use the imported map/floor frames and lie strictly within the admitted image bounds",
                ));
            }
        }
        let existing = operations.operation(intent.authority.operation_id);
        let prior_active = match existing {
            Some(existing) => {
                if !existing_calibration_matches(existing, project.id(), &intent) {
                    return Err(ApplicationError::new(
                        crate::ErrorKind::Conflict,
                        "operation identity already belongs to a different map mutation",
                    ));
                }
                match existing.inverse() {
                    InverseMetadata::ApplyV2 {
                        prior:
                            InversePrior::CalibrationAbsent {
                                calibration_id: prior_calibration_id,
                                map_id: prior_map_id,
                                active,
                            },
                    } if *prior_calibration_id == calibration_id && *prior_map_id == map_id => {
                        active.clone()
                    }
                    _ => {
                        return Err(ApplicationError::new(
                            crate::ErrorKind::Conflict,
                            "operation identity already belongs to a different map mutation",
                        ));
                    }
                }
            }
            None => {
                if project.map(map_id).is_none() {
                    return Err(ApplicationError::new(
                        crate::ErrorKind::Prerequisite,
                        "map calibration requires an imported canonical map",
                    ));
                }
                project.active_calibration(map_id).ok_or_else(|| {
                    ApplicationError::new(
                        crate::ErrorKind::CorruptProject,
                        "canonical map is missing its active calibration state",
                    )
                })?
            }
        };
        if let Some(existing) = existing {
            return self.exact_duplicate_outcome(existing.clone(), existing.content_hash(), budget);
        }
        let context = derive_map_context_from_set(
            &operations,
            &intent.authority,
            state.project_revision(),
            budget,
        )?;
        let request = CalibrateMapRequest {
            context,
            calibration: intent.calibration,
            prior_active,
        };
        let operation = build_calibration_operation(project.id(), &request)?;
        self.commit_operation_with_outcome(
            operation.clone(),
            request.context.expected_project_revision,
            request.context.committed_utc_ms,
            operation.content_hash(),
            budget,
        )
    }

    fn canonical_map_state<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<(Project, OperationSet, OperationStoreState), ApplicationError> {
        let baseline = self
            .bundle
            .materialization_baseline_with_budget(budget)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?
            .ok_or_else(|| {
                ApplicationError::new(
                    crate::ErrorKind::Prerequisite,
                    "map mutation requires a canonical project baseline",
                )
            })?;
        let operations = self
            .bundle
            .operation_set_with_budget(budget)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        let state = self
            .bundle
            .operation_store_state()
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        if state.project_id() != baseline.id()
            || operations.project_id() != baseline.id()
            || state.operation_count() != operations.operations().count()
            || state.project_revision().value() != state.operation_count() as u64
        {
            return Err(ApplicationError::new(
                crate::ErrorKind::Conflict,
                "canonical operation state changed while deriving map mutation context",
            ));
        }
        let materialized =
            kyberia_causal_materializer::materialize_with_budget(&baseline, &operations, budget)
                .map_err(map_materialization_error)?;
        if materialized.project().revision() != state.project_revision().value() {
            return Err(ApplicationError::new(
                crate::ErrorKind::CorruptProject,
                "canonical operation revision does not match materialized project",
            ));
        }
        Ok((materialized.into_project(), operations, state))
    }

    fn exact_duplicate_outcome<H: CancellationHook>(
        &mut self,
        operation: Operation,
        content_hash: ContentHash,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationOutcome, ApplicationError> {
        let operation_id = operation.operation_id();
        let append = self
            .bundle
            .append_operation_if_revision_with_budget(operation, None, budget)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        let project_revision = match append {
            OperationAppendOutcome::Duplicate {
                project_revision, ..
            } => project_revision,
            OperationAppendOutcome::Appended { .. } => {
                return Err(ApplicationError::new(
                    crate::ErrorKind::Conflict,
                    "map retry operation disappeared from the canonical set",
                ));
            }
        };
        let receipt = MapMutationReceipt {
            operation_id,
            project_revision,
            content_hash,
        };
        let current = <Self as ProjectStorePort>::current_snapshot_with_budget(self, budget);
        Ok(MapMutationOutcome { receipt, current })
    }

    fn project_id(&self) -> Result<ProjectId, ApplicationError> {
        self.bundle
            .manifest()
            .map(|manifest| manifest.project_id)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))
    }

    pub(crate) fn save_point_survey_snapshot(
        &mut self,
        request: PointSurveySnapshotRequest,
    ) -> Result<PointSurveySnapshotReceipt, ApplicationError> {
        if request.committed_utc_ms < 0 {
            return Err(ApplicationError::new(
                crate::ErrorKind::InvalidRequest,
                "committed_utc_ms must be nonnegative",
            ));
        }
        let record = self
            .bundle
            .save_survey_snapshot_if_revision(
                request.snapshot_id,
                &request.survey,
                request.committed_utc_ms,
                request.expected_bundle_revision,
            )
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        receipt_from_store(record)
    }

    pub(crate) fn load_point_survey_snapshot(
        &self,
        snapshot_id: SnapshotId,
        expected_session: Option<SessionId>,
    ) -> Result<LoadedPointSurveySnapshot, ApplicationError> {
        let loaded = self
            .bundle
            .load_survey_snapshot_for_session(snapshot_id, expected_session)
            .map_err(map_snapshot_read_error)?;
        loaded_from_store(loaded)
    }

    pub(crate) fn list_point_survey_snapshot_history(
        &self,
        session_id: Option<SessionId>,
    ) -> Result<Vec<PointSurveySnapshotHistoryEntry>, ApplicationError> {
        self.bundle
            .list_survey_snapshot_history(session_id)
            .map_err(|error| map_store_error(StoreContext::Query, error))?
            .into_iter()
            .map(history_from_store)
            .collect()
    }
}

fn map_snapshot_read_error(error: kyberia_project_store::StoreError) -> ApplicationError {
    if let kyberia_project_store::StoreError::Invalid(message) = &error
        && matches!(
            message.as_str(),
            "survey snapshot is not registered" | "survey snapshot session mismatch"
        )
    {
        return ApplicationError::new(crate::ErrorKind::InvalidRequest, message.clone());
    }
    map_store_error(StoreContext::Query, error)
}

fn map_operation_input(error: OperationError) -> ApplicationError {
    match error {
        OperationError::Cancelled => {
            ApplicationError::new(crate::ErrorKind::Cancelled, "project operation cancelled")
        }
        OperationError::ResourceLimit(reason) => {
            ApplicationError::new(crate::ErrorKind::ResourceLimit, reason)
        }
        other => ApplicationError::new(crate::ErrorKind::InvalidRequest, other.to_string()),
    }
}

fn map_materialization_error(
    error: kyberia_causal_materializer::MaterializationError,
) -> ApplicationError {
    match error {
        kyberia_causal_materializer::MaterializationError::Cancelled => {
            ApplicationError::new(crate::ErrorKind::Cancelled, "project operation cancelled")
        }
        kyberia_causal_materializer::MaterializationError::ResourceLimit(reason) => {
            ApplicationError::new(crate::ErrorKind::ResourceLimit, reason)
        }
        other => ApplicationError::new(crate::ErrorKind::InvalidRequest, other.to_string()),
    }
}

fn admitted_map(
    intent: &ImportMapIntent,
    admitted: AdmittedMapAsset,
    byte_length: usize,
) -> Result<MapAsset, ApplicationError> {
    let byte_length = u64::try_from(byte_length).map_err(|_| {
        ApplicationError::new(
            crate::ErrorKind::ResourceLimit,
            "map source length overflow",
        )
    })?;
    MapAsset::new(MapAssetData {
        id: intent.map_id,
        floor_id: intent.floor_id,
        name: intent.name.clone(),
        image_frame: intent.image_frame.clone(),
        width: admitted.width(),
        height: admitted.height(),
        source: ArtifactReference {
            sha256: admitted.content_hash(),
            media_type: Text::new(PNG_MEDIA_TYPE).expect("static media type is valid"),
            byte_length,
        },
        provenance: intent.provenance.clone(),
    })
    .map_err(|error| ApplicationError::new(crate::ErrorKind::InvalidRequest, error.to_string()))
}

fn build_import_operation(
    project_id: ProjectId,
    request: &ImportMapRequest,
    map: MapAsset,
) -> Result<Operation, ApplicationError> {
    Operation::try_apply_v3(
        request.context.operation_id,
        project_id,
        request.context.actor_id,
        request.context.device_id,
        request.context.logical_time,
        request.context.causal_depth,
        request.context.parents.clone(),
        Mutation::import_map(map),
        InversePrior::MapAbsent {
            map_id: request.map_id,
        },
    )
    .map_err(map_operation_input)
}

fn build_calibration_operation(
    project_id: ProjectId,
    request: &CalibrateMapRequest,
) -> Result<Operation, ApplicationError> {
    let calibration_id = request.calibration.id;
    let map_id = request.calibration.map_id;
    Operation::try_apply_v3(
        request.context.operation_id,
        project_id,
        request.context.actor_id,
        request.context.device_id,
        request.context.logical_time,
        request.context.causal_depth,
        request.context.parents.clone(),
        Mutation::calibrate_map(request.calibration.clone()),
        InversePrior::CalibrationAbsent {
            calibration_id,
            map_id,
            active: request.prior_active.clone(),
        },
    )
    .map_err(map_operation_input)
}

fn derive_map_context_from_set<H: CancellationHook>(
    operations: &OperationSet,
    authority: &MapIntentAuthority,
    expected_project_revision: ProjectVersion,
    budget: &mut ResourceBudget<H>,
) -> Result<MapOperationContext, ApplicationError> {
    let mut non_heads = BTreeSet::new();
    let mut maximum_logical_time = 0_u64;
    for operation in operations.operations() {
        budget.check_cancelled().map_err(map_budget_error)?;
        maximum_logical_time = maximum_logical_time.max(operation.logical_time().value());
        non_heads.extend(operation.parents().iter().copied());
    }
    let mut heads = Vec::new();
    let mut maximum_parent_depth = None;
    for operation in operations.operations() {
        budget.check_cancelled().map_err(map_budget_error)?;
        if !non_heads.contains(&operation.operation_id()) {
            heads.push(operation);
            maximum_parent_depth = Some(
                maximum_parent_depth
                    .unwrap_or(0_u64)
                    .max(operation.causal_depth().value()),
            );
        }
    }
    if heads.len() > MAX_PARENTS {
        return Err(ApplicationError::new(
            crate::ErrorKind::Conflict,
            "canonical operation frontier exceeds the map mutation parent bound",
        ));
    }
    let logical_time = maximum_logical_time.checked_add(1).ok_or_else(|| {
        ApplicationError::new(crate::ErrorKind::Conflict, "logical timestamp exhausted")
    })?;
    let causal_depth = maximum_parent_depth
        .map(|depth| depth.checked_add(1))
        .unwrap_or(Some(0))
        .ok_or_else(|| {
            ApplicationError::new(crate::ErrorKind::Conflict, "causal depth exhausted")
        })?;
    Ok(MapOperationContext {
        operation_id: authority.operation_id,
        actor_id: authority.actor_id,
        device_id: authority.device_id,
        logical_time: LogicalTimestamp::new(logical_time).map_err(map_operation_input)?,
        causal_depth: CausalDepth::new(causal_depth),
        parents: heads
            .into_iter()
            .map(|operation| operation.operation_id())
            .collect(),
        expected_project_revision,
        committed_utc_ms: authority.committed_utc_ms,
    })
}

fn existing_import_matches(
    operation: &Operation,
    project_id: ProjectId,
    intent: &ImportMapIntent,
    map: &MapAsset,
) -> bool {
    operation.operation_id() == intent.authority.operation_id
        && operation.project_id() == project_id
        && operation.actor_id() == intent.authority.actor_id
        && operation.device_id() == intent.authority.device_id
        && operation.schema_version() == kyberia_operation_log::OperationSchemaVersion::V3
        && matches!(
            operation.payload(),
            OperationPayload::Apply { mutation }
                if mutation == &Mutation::import_map(map.clone())
        )
        && matches!(
            operation.inverse(),
            InverseMetadata::ApplyV2 {
                prior: InversePrior::MapAbsent { map_id },
            } if *map_id == intent.map_id
        )
}

fn existing_calibration_matches(
    operation: &Operation,
    project_id: ProjectId,
    intent: &CalibrateMapIntent,
) -> bool {
    operation.project_id() == project_id
        && operation.actor_id() == intent.authority.actor_id
        && operation.device_id() == intent.authority.device_id
        && operation.schema_version() == kyberia_operation_log::OperationSchemaVersion::V3
        && matches!(
            operation.payload(),
            OperationPayload::Apply { mutation }
                if mutation == &Mutation::calibrate_map(intent.calibration.clone())
        )
        && matches!(
            operation.inverse(),
            InverseMetadata::ApplyV2 {
                prior: InversePrior::CalibrationAbsent {
                    calibration_id,
                    map_id,
                    ..
                },
            } if *calibration_id == intent.calibration.id
                && *map_id == intent.calibration.map_id
        )
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
