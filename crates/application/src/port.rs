use crate::{
    command::SessionMode,
    error::{ApplicationError, StoreContext, map_budget_error, map_store_error},
    map_asset::{AdmittedMapAsset, PNG_MEDIA_TYPE},
    map_mutation::{CalibrateMapRequest, ImportMapRequest, MapMutationReceipt},
    query::{CurrentProjectView, snapshot_to_view},
};
use kyberia_domain::{
    evidence::ArtifactReference,
    identity::{ContentHash, ProjectId, Text},
    project::{MapAsset, MapAssetData, Project},
};
use kyberia_operation_log::{
    InversePrior, Mutation, Operation, OperationError, OperationSet, ProjectVersion,
};
use kyberia_project_store::{
    ArtifactEntry, ArtifactKind, Bundle, CanonicalProjectSnapshot, OpenMode, OperationAppendOutcome,
};
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

    pub(crate) fn commit_operation<H: CancellationHook>(
        &mut self,
        operation: Operation,
        expected_revision: ProjectVersion,
        utc_ms: i64,
        content_hash: ContentHash,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MapMutationReceipt, ApplicationError> {
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
        let operations = self
            .bundle
            .operation_set_with_budget(budget)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?;
        let materialized =
            kyberia_causal_materializer::materialize_with_budget(&baseline, &operations, budget)
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
        let current = self
            .bundle
            .canonical_project_snapshot()
            .map_err(|error| map_store_error(StoreContext::Mutation, error))?
            .current
            .ok_or_else(|| {
                ApplicationError::new(crate::ErrorKind::Storage, "publication readback missing")
            })?;
        if current.project() != materialized.project() {
            return Err(ApplicationError::new(
                crate::ErrorKind::Storage,
                "publication readback mismatch",
            ));
        }
        Ok(MapMutationReceipt {
            operation_id,
            project_revision: revision,
            content_hash,
        })
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

    fn project_id(&self) -> Result<ProjectId, ApplicationError> {
        self.bundle
            .manifest()
            .map(|manifest| manifest.project_id)
            .map_err(|error| map_store_error(StoreContext::Mutation, error))
    }
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
