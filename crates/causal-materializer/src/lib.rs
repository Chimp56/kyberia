//! Apply a validated immutable operation DAG to a canonical project.
//!
//! This is a pure application boundary.  It owns neither storage nor a
//! generic command envelope.  The legacy linear project receipt API remains
//! available; this crate uses the domain's separate V2 materialized-project
//! entry point so equal-Lamport independent branches remain representable.

use kyberia_domain::{
    evidence::Evidence,
    identity::{CalibrationId, FloorId, MapAssetId, OperationId, Text},
    project::{Project, ProjectCommand, ProjectError, ProjectSchemaVersion},
};
use kyberia_materialization_identity::{
    BaselineIdentity, IdentityError, MaterializationIdentity, OperationSetIdentity,
};
use kyberia_operation_log::{
    AppliedEffect, FieldKey, InverseMetadata, InversePrior, MergeError, Mutation, Operation,
    OperationPayload, OperationReference, OperationSchemaVersion, OperationSet, ResolutionValue,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Effect {
    operation_id: OperationId,
    value: EffectValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EffectValue {
    Mutation(Mutation),
    Calibration {
        map_id: MapAssetId,
        calibration: Evidence<CalibrationId>,
    },
}

impl EffectValue {
    /// Compare the semantic field value without cloning typed payloads.
    ///
    /// The typed replay boundary deliberately preserves a known calibration's
    /// evidence class, while the legacy mutation boundary represents the same
    /// known value as `ActivateCalibration`.  Those two encodings must not
    /// manufacture an ambiguity during causal-prior validation. Explicit
    /// unknown calibration remains typed because it has no mutation equivalent.
    fn semantically_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Mutation(left), Self::Mutation(right)) => left == right,
            (
                Self::Calibration {
                    map_id: left_map,
                    calibration: Evidence::Known(left_calibration),
                },
                Self::Mutation(Mutation::ActivateCalibration {
                    map_id: right_map,
                    calibration_id: right_calibration,
                }),
            )
            | (
                Self::Mutation(Mutation::ActivateCalibration {
                    map_id: left_map,
                    calibration_id: left_calibration,
                }),
                Self::Calibration {
                    map_id: right_map,
                    calibration: Evidence::Known(right_calibration),
                },
            ) => left_map == right_map && left_calibration == right_calibration,
            (
                Self::Calibration {
                    map_id: left_map,
                    calibration: left_calibration,
                },
                Self::Calibration {
                    map_id: right_map,
                    calibration: right_calibration,
                },
            ) => left_map == right_map && left_calibration == right_calibration,
            _ => false,
        }
    }
}

/// A conservative bound for one pure materialization.  The operation-log
/// format admits larger sets for storage/merge, but retaining per-operation
/// causal project witnesses is intentionally bounded until a persistent
/// structural state representation is introduced.
pub const MAX_MATERIALIZATION_OPERATIONS: usize = 8_192;
/// Maximum ancestor-operation visits used while constructing causal witnesses.
pub const MAX_CAUSAL_WITNESS_WORK: usize = 2_000_000;
const MAX_AGGREGATE_CONFLICT_CHECKS: usize = 1_000_000;
/// Maximum cumulative estimated bytes copied by project clones during one
/// materialization. Causal witnesses reconstruct ancestor prefixes, so a
/// chain can copy the aggregate quadratically even though peak live state is
/// linear. The estimate includes the baseline bytes and operation payload
/// bytes and is intentionally conservative.
const MAX_CAUSAL_COPY_BYTES: usize = 256 * 1024 * 1024;
/// Maximum estimated size of one project state before another domain clone.
/// Baseline identity validation bounds the serialized baseline; operation
/// payload growth is charged here too.
const MAX_ESTIMATED_PROJECT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterializationError {
    Identity(IdentityError),
    Merge(MergeError),
    Domain(ProjectError),
    WrongProject,
    BaselineLogicalTimeConflict {
        operation_id: OperationId,
        baseline: u64,
        operation: u64,
    },
    BaselineOperationCollision(OperationId),
    CausalPriorMismatch {
        operation_id: OperationId,
        field: FieldKey,
    },
    AmbiguousCausalState {
        operation_id: OperationId,
        field: FieldKey,
    },
    AggregateConflict {
        floor_id: FloorId,
        evidence_operation: OperationId,
        calibration_operation: OperationId,
    },
    UnsupportedOperation {
        operation_id: OperationId,
        reason: &'static str,
    },
    ResourceLimit(&'static str),
}

impl std::fmt::Display for MaterializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MaterializationError {}

impl From<IdentityError> for MaterializationError {
    fn from(value: IdentityError) -> Self {
        Self::Identity(value)
    }
}

impl From<MergeError> for MaterializationError {
    fn from(value: MergeError) -> Self {
        Self::Merge(value)
    }
}

impl From<ProjectError> for MaterializationError {
    fn from(value: ProjectError) -> Self {
        Self::Domain(value)
    }
}

/// A successful pure materialization and the exact identities used to bind
/// it.  Storage publication is a separate outer transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedProject {
    project: Project,
    identity: MaterializationIdentity,
}

impl MaterializedProject {
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn into_project(self) -> Project {
        self.project
    }

    pub const fn identity(&self) -> &MaterializationIdentity {
        &self.identity
    }
}

/// Materialize `operations` against exactly `baseline`.
///
/// Every inverse prior is checked against the state formed by that operation's
/// causal ancestors.  A deterministic total order is used only to present
/// independent effects after causal admission; it is never used to choose a
/// concurrent winner.  Failures occur before the caller-visible project is
/// returned and inputs are borrowed immutably throughout.
pub fn materialize(
    baseline: &Project,
    operations: &OperationSet,
) -> Result<MaterializedProject, MaterializationError> {
    if baseline.id() != operations.project_id() {
        return Err(MaterializationError::WrongProject);
    }
    let operation_count = operations.operations().count();
    if operation_count > MAX_MATERIALIZATION_OPERATIONS {
        return Err(MaterializationError::ResourceLimit(
            "materialization_operations",
        ));
    }
    for operation in operations.operations() {
        if baseline.has_applied_operation(operation.operation_id()) {
            return Err(MaterializationError::BaselineOperationCollision(
                operation.operation_id(),
            ));
        }
        if operation.parents().is_empty()
            && operation.logical_time().value() <= baseline.logical_time()
        {
            return Err(MaterializationError::BaselineLogicalTimeConflict {
                operation_id: operation.operation_id(),
                baseline: baseline.logical_time(),
                operation: operation.logical_time().value(),
            });
        }
    }

    let baseline_identity = BaselineIdentity::from_project(baseline)?;
    let operation_identity = OperationSetIdentity::from_operation_set(operations)?;
    let identity = MaterializationIdentity::bind(&baseline_identity, &operation_identity)?;
    let mut copy_budget = CopyBudget::new(baseline_identity.project_bytes().len())?;

    // This checks unresolved semantic conflicts and sequentially repeated
    // toggles before any domain application.  The typed path retains unknown
    // calibration effects.
    operations.validate_replay_semantics()?;
    let effects = operations.replay_effects()?;
    reject_cross_field_aggregate_conflicts(baseline, operations, &effects)?;

    let ordered = operations.ordered().map_err(MergeError::Operation)?;
    let resolution_refs: BTreeSet<_> = ordered
        .iter()
        .filter_map(|operation| operation.resolution_references())
        .flat_map(|(left, right)| [left.operation_id(), right.operation_id()])
        .collect();
    let mut witness_ids = BTreeSet::new();
    let mut witness_work = 0usize;
    for operation in &ordered {
        let causal = causal_before(
            baseline,
            operations,
            &ordered,
            operation,
            &mut witness_work,
            &mut copy_budget,
        )?;
        if resolution_refs.contains(&operation.operation_id()) {
            if witness_ids.len() >= MAX_MATERIALIZATION_OPERATIONS / 2 {
                return Err(MaterializationError::ResourceLimit("causal_witness_ids"));
            }
            witness_ids.insert(operation.operation_id());
        }
        validate_operation_prior(
            operation,
            baseline,
            &causal,
            &witness_ids,
            operations,
            &ordered,
            &mut witness_work,
            &mut copy_budget,
        )?;
    }

    let mut project = copy_budget.clone_baseline(baseline)?;
    let mut estimated_project_bytes = copy_budget.baseline_project_bytes;
    for effect in &effects {
        charge(&mut witness_work, 1, "materialization_work")?;
        let operation = operations.operation(effect.operation_id()).ok_or(
            MaterializationError::UnsupportedOperation {
                operation_id: effect.operation_id(),
                reason: "effect operation is absent from operation set",
            },
        )?;
        let next_estimated_project_bytes =
            copy_budget.before_apply(estimated_project_bytes, operation)?;
        project = apply_effect(&project, operation, effect)?;
        estimated_project_bytes = next_estimated_project_bytes;
    }
    if project.schema_version() != ProjectSchemaVersion::V2 && !effects.is_empty() {
        return Err(MaterializationError::Domain(ProjectError::InvalidReceipt));
    }
    Ok(MaterializedProject { project, identity })
}

struct CopyBudget {
    copied_bytes: usize,
    baseline_project_bytes: usize,
}

impl CopyBudget {
    fn new(baseline_project_bytes: usize) -> Result<Self, MaterializationError> {
        if baseline_project_bytes > MAX_ESTIMATED_PROJECT_BYTES {
            return Err(MaterializationError::ResourceLimit("causal_project_bytes"));
        }
        Ok(Self {
            copied_bytes: 0,
            baseline_project_bytes,
        })
    }

    fn clone_baseline(&mut self, baseline: &Project) -> Result<Project, MaterializationError> {
        self.charge_clone(self.baseline_project_bytes)?;
        Ok(baseline.clone())
    }

    fn before_apply(
        &mut self,
        estimated_project_bytes: usize,
        operation: &Operation,
    ) -> Result<usize, MaterializationError> {
        let next = estimated_project_bytes
            .checked_add(operation.canonical_bytes().len())
            .ok_or(MaterializationError::ResourceLimit("causal_project_bytes"))?;
        if next > MAX_ESTIMATED_PROJECT_BYTES {
            return Err(MaterializationError::ResourceLimit("causal_project_bytes"));
        }
        self.charge_clone(estimated_project_bytes)?;
        Ok(next)
    }

    fn charge_clone(&mut self, estimated_project_bytes: usize) -> Result<(), MaterializationError> {
        self.copied_bytes = self
            .copied_bytes
            .checked_add(estimated_project_bytes)
            .ok_or(MaterializationError::ResourceLimit("causal_copy_bytes"))?;
        if self.copied_bytes > MAX_CAUSAL_COPY_BYTES {
            return Err(MaterializationError::ResourceLimit("causal_copy_bytes"));
        }
        Ok(())
    }
}

fn charge(
    work: &mut usize,
    amount: usize,
    label: &'static str,
) -> Result<(), MaterializationError> {
    *work = work
        .checked_add(amount)
        .ok_or(MaterializationError::ResourceLimit(label))?;
    if *work > MAX_CAUSAL_WITNESS_WORK {
        return Err(MaterializationError::ResourceLimit(label));
    }
    Ok(())
}

fn causal_before(
    baseline: &Project,
    operations: &OperationSet,
    ordered: &[&Operation],
    operation: &Operation,
    work: &mut usize,
    copy_budget: &mut CopyBudget,
) -> Result<CausalReplay, MaterializationError> {
    if operation.parents().is_empty() {
        return Ok(CausalReplay::new(
            copy_budget.clone_baseline(baseline)?,
            copy_budget.baseline_project_bytes,
        ));
    }

    let ancestors = ancestor_ids(operations, operation, work)?;
    let mut replay = CausalReplay::new(
        copy_budget.clone_baseline(baseline)?,
        copy_budget.baseline_project_bytes,
    );
    for candidate in ordered {
        charge(work, 1, "causal_witness_work")?;
        if ancestors.contains(&candidate.operation_id()) {
            replay.apply(candidate, operations, copy_budget)?;
        }
    }
    Ok(replay)
}

fn ancestor_ids(
    operations: &OperationSet,
    operation: &Operation,
    work: &mut usize,
) -> Result<BTreeSet<OperationId>, MaterializationError> {
    let mut result = BTreeSet::new();
    let mut stack = operation.parents().to_vec();
    while let Some(id) = stack.pop() {
        if !result.insert(id) {
            continue;
        }
        *work = work
            .checked_add(1)
            .ok_or(MaterializationError::ResourceLimit("causal_ancestor_work"))?;
        if *work > MAX_CAUSAL_WITNESS_WORK {
            return Err(MaterializationError::ResourceLimit("causal_ancestor_work"));
        }
        let parent =
            operations
                .operation(id)
                .ok_or(MaterializationError::UnsupportedOperation {
                    operation_id: id,
                    reason: "causal parent is absent",
                })?;
        stack.extend(parent.parents());
    }
    Ok(result)
}

fn is_ancestor(
    operations: &OperationSet,
    ancestor: OperationId,
    descendant: OperationId,
    checks: &mut usize,
) -> Result<bool, MaterializationError> {
    if ancestor == descendant {
        return Ok(false);
    }
    let mut visited = BTreeSet::new();
    let mut stack = vec![descendant];
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        *checks = checks
            .checked_add(1)
            .ok_or(MaterializationError::ResourceLimit("causal_relation_work"))?;
        if *checks > MAX_CAUSAL_WITNESS_WORK {
            return Err(MaterializationError::ResourceLimit("causal_relation_work"));
        }
        let operation =
            operations
                .operation(id)
                .ok_or(MaterializationError::UnsupportedOperation {
                    operation_id: id,
                    reason: "causal operation is absent",
                })?;
        for parent in operation.parents() {
            if *parent == ancestor {
                return Ok(true);
            }
            stack.push(*parent);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn common_causal_state(
    operations: &OperationSet,
    ordered: &[&Operation],
    witness_ids: &BTreeSet<OperationId>,
    left: OperationReference,
    right: OperationReference,
    operation_id: OperationId,
    work: &mut usize,
    baseline: &Project,
    copy_budget: &mut CopyBudget,
) -> Result<CausalReplay, MaterializationError> {
    if !witness_ids.contains(&left.operation_id()) || !witness_ids.contains(&right.operation_id()) {
        return Err(MaterializationError::UnsupportedOperation {
            operation_id,
            reason: "resolution reference is absent from causal witnesses",
        });
    }
    let left_operation = operations.operation(left.operation_id()).ok_or(
        MaterializationError::UnsupportedOperation {
            operation_id,
            reason: "resolution left operation is absent",
        },
    )?;
    let right_operation = operations.operation(right.operation_id()).ok_or(
        MaterializationError::UnsupportedOperation {
            operation_id,
            reason: "resolution right operation is absent",
        },
    )?;
    let left_ancestors = ancestor_ids(operations, left_operation, work)?;
    let right_ancestors = ancestor_ids(operations, right_operation, work)?;
    let common: BTreeSet<_> = left_ancestors
        .intersection(&right_ancestors)
        .copied()
        .collect();
    let mut replay = CausalReplay::new(
        copy_budget.clone_baseline(baseline)?,
        copy_budget.baseline_project_bytes,
    );
    for candidate in ordered {
        charge(work, 1, "causal_witness_work")?;
        if common.contains(&candidate.operation_id()) {
            replay.apply(candidate, operations, copy_budget)?;
        }
    }
    Ok(replay)
}

fn reject_ambiguous_prior(
    operation: &Operation,
    events: &[CausalEvent],
    field: FieldKey,
    operations: &OperationSet,
    work: &mut usize,
) -> Result<(), MaterializationError> {
    let relevant: Vec<_> = events.iter().filter(|event| event.field == field).collect();
    let mut maximal = Vec::new();
    for (index, candidate) in relevant.iter().enumerate() {
        let mut superseded = false;
        for (other_index, other) in relevant.iter().enumerate() {
            if index != other_index
                && is_ancestor(operations, candidate.operation_id, other.operation_id, work)?
            {
                superseded = true;
                break;
            }
        }
        if !superseded {
            maximal.push(*candidate);
        }
    }
    for (index, left) in maximal.iter().enumerate() {
        for right in maximal.iter().skip(index + 1) {
            if !left.value.semantically_equal(&right.value) {
                return Err(MaterializationError::AmbiguousCausalState {
                    operation_id: operation.operation_id(),
                    field,
                });
            }
        }
    }
    Ok(())
}

struct CausalReplay {
    project: Project,
    estimated_project_bytes: usize,
    active: BTreeMap<OperationId, bool>,
    events: Vec<CausalEvent>,
}

struct CausalEvent {
    operation_id: OperationId,
    field: FieldKey,
    value: EffectValue,
}

impl CausalReplay {
    fn new(project: Project, estimated_project_bytes: usize) -> Self {
        Self {
            project,
            estimated_project_bytes,
            active: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    fn apply(
        &mut self,
        operation: &Operation,
        operations: &OperationSet,
        copy_budget: &mut CopyBudget,
    ) -> Result<(), MaterializationError> {
        match operation.payload() {
            OperationPayload::Apply { mutation } => {
                self.active.insert(operation.operation_id(), true);
                self.apply_effect(
                    operation,
                    direct_mutation_effect(operation, mutation),
                    copy_budget,
                )
            }
            OperationPayload::Undo { target } => {
                if self.active.get(&target.operation_id()) != Some(&true) {
                    return Ok(());
                }
                let target_operation = operations.operation(target.operation_id()).ok_or(
                    MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "toggle target is absent",
                    },
                )?;
                self.active.insert(target.operation_id(), false);
                self.apply_effect(
                    operation,
                    inverse_effect(target_operation, operation.operation_id())?,
                    copy_budget,
                )
            }
            OperationPayload::Redo { target } => {
                if self.active.get(&target.operation_id()) != Some(&false) {
                    return Ok(());
                }
                let target_operation = operations.operation(target.operation_id()).ok_or(
                    MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "redo target is absent",
                    },
                )?;
                let OperationPayload::Apply { mutation } = target_operation.payload() else {
                    return Err(MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "redo target is not an apply",
                    });
                };
                self.active.insert(target.operation_id(), true);
                self.apply_effect(
                    operation,
                    direct_mutation_effect_by_id(operation.operation_id(), mutation),
                    copy_budget,
                )
            }
            OperationPayload::Resolve { mutation, .. } => self.apply_effect(
                operation,
                direct_mutation_effect(operation, mutation),
                copy_budget,
            ),
            OperationPayload::ResolveV2 { value, .. } => self.apply_effect(
                operation,
                resolution_effect(operation.operation_id(), value),
                copy_budget,
            ),
        }
    }

    fn apply_effect(
        &mut self,
        operation: &Operation,
        effect: Effect,
        copy_budget: &mut CopyBudget,
    ) -> Result<(), MaterializationError> {
        let event = CausalEvent {
            operation_id: effect.operation_id,
            field: effect_field(&effect),
            value: effect.value.clone(),
        };
        let next_estimated_project_bytes =
            copy_budget.before_apply(self.estimated_project_bytes, operation)?;
        self.project = apply_local_effect(&self.project, operation, &effect)?;
        self.estimated_project_bytes = next_estimated_project_bytes;
        self.events.push(event);
        Ok(())
    }
}

fn direct_mutation_effect(operation: &Operation, mutation: &Mutation) -> Effect {
    Effect {
        operation_id: operation.operation_id(),
        value: EffectValue::Mutation(mutation.clone()),
    }
}

fn inverse_effect(
    operation: &Operation,
    operation_id: OperationId,
) -> Result<Effect, MaterializationError> {
    match operation.inverse() {
        InverseMetadata::Apply { mutation } => {
            Ok(direct_mutation_effect_by_id(operation_id, mutation))
        }
        InverseMetadata::ApplyV2 { prior } => match prior {
            InversePrior::MapCalibration {
                map_id,
                calibration,
            } => Ok(resolution_effect(
                operation_id,
                &ResolutionValue::Calibration {
                    map_id: *map_id,
                    calibration: calibration.clone(),
                },
            )),
            InversePrior::ProjectName { name } => Ok(direct_mutation_effect_by_id(
                operation_id,
                &Mutation::SetProjectName { name: name.clone() },
            )),
            InversePrior::SiteName { site_id, name } => Ok(direct_mutation_effect_by_id(
                operation_id,
                &Mutation::SetSiteName {
                    site_id: *site_id,
                    name: name.clone(),
                },
            )),
        },
        InverseMetadata::NonReversible { .. } => Err(MaterializationError::UnsupportedOperation {
            operation_id,
            reason: "non-reversible operation was used as an inverse",
        }),
        InverseMetadata::Toggle { .. } => Err(MaterializationError::UnsupportedOperation {
            operation_id,
            reason: "toggle metadata cannot be a domain effect",
        }),
    }
}

fn direct_mutation_effect_by_id(operation_id: OperationId, mutation: &Mutation) -> Effect {
    Effect {
        operation_id,
        value: EffectValue::Mutation(mutation.clone()),
    }
}

fn resolution_effect(operation_id: OperationId, value: &ResolutionValue) -> Effect {
    match value {
        ResolutionValue::Mutation(mutation) => direct_mutation_effect_by_id(operation_id, mutation),
        ResolutionValue::Calibration {
            map_id,
            calibration,
        } => Effect {
            operation_id,
            value: EffectValue::Calibration {
                map_id: *map_id,
                calibration: calibration.clone(),
            },
        },
    }
}

fn local_effect(applied: &AppliedEffect) -> Effect {
    match applied {
        AppliedEffect::Mutation(value) => Effect {
            operation_id: value.operation_id(),
            value: EffectValue::Mutation(value.mutation().clone()),
        },
        AppliedEffect::Calibration {
            operation_id,
            map_id,
            calibration,
        } => Effect {
            operation_id: *operation_id,
            value: EffectValue::Calibration {
                map_id: *map_id,
                calibration: calibration.clone(),
            },
        },
    }
}

fn effect_field(effect: &Effect) -> FieldKey {
    match &effect.value {
        EffectValue::Mutation(mutation) => mutation.field_key(),
        EffectValue::Calibration { map_id, .. } => FieldKey::MapCalibration(*map_id),
    }
}

fn apply_effect(
    project: &Project,
    operation: &Operation,
    applied: &AppliedEffect,
) -> Result<Project, MaterializationError> {
    apply_local_effect(project, operation, &local_effect(applied))
}

fn apply_local_effect(
    project: &Project,
    operation: &Operation,
    effect: &Effect,
) -> Result<Project, MaterializationError> {
    let command = match &effect.value {
        EffectValue::Mutation(mutation) => match mutation {
            Mutation::SetProjectName { name } => {
                ProjectCommand::SetProjectName { name: name.clone() }
            }
            Mutation::SetSiteName { site_id, name } => ProjectCommand::SetSiteName {
                site_id: *site_id,
                name: name.clone(),
            },
            Mutation::ActivateCalibration {
                map_id,
                calibration_id,
            } => ProjectCommand::ActivateCalibration {
                map_id: *map_id,
                calibration: Evidence::Known(*calibration_id),
            },
            Mutation::BindFloorEvidence {
                floor_id,
                reference,
            } => {
                if operation.schema_version() == OperationSchemaVersion::V1 {
                    return Err(MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "V1 floor-evidence binding lacks a non-reversible inverse",
                    });
                }
                ProjectCommand::BindFloorEvidence {
                    floor_id: *floor_id,
                    evidence: kyberia_domain::evidence::ArtifactReference {
                        sha256: reference.hash(),
                        media_type: reference.media_type().clone(),
                        byte_length: reference.byte_length(),
                    },
                }
            }
        },
        EffectValue::Calibration {
            map_id,
            calibration,
            ..
        } => ProjectCommand::ActivateCalibration {
            map_id: *map_id,
            calibration: calibration.clone(),
        },
    };
    project
        .apply_materialized(
            effect.operation_id,
            operation.logical_time().value(),
            command,
        )
        .map_err(MaterializationError::Domain)
}

#[allow(clippy::too_many_arguments)]
fn validate_operation_prior(
    operation: &Operation,
    baseline: &Project,
    causal: &CausalReplay,
    witness_ids: &BTreeSet<OperationId>,
    operations: &OperationSet,
    ordered: &[&Operation],
    work: &mut usize,
    copy_budget: &mut CopyBudget,
) -> Result<(), MaterializationError> {
    match operation.payload() {
        OperationPayload::Apply { mutation } => match operation.inverse() {
            InverseMetadata::Apply { mutation: prior } => {
                if matches!(mutation, Mutation::BindFloorEvidence { .. }) {
                    return Err(MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "V1 floor-evidence binding has no executable inverse",
                    });
                }
                reject_ambiguous_prior(
                    operation,
                    &causal.events,
                    prior.field_key(),
                    operations,
                    work,
                )?;
                validate_legacy_prior(operation.operation_id(), &causal.project, prior)
            }
            InverseMetadata::ApplyV2 { prior } => {
                reject_ambiguous_prior(
                    operation,
                    &causal.events,
                    prior.field_key(),
                    operations,
                    work,
                )?;
                validate_typed_prior(operation.operation_id(), &causal.project, prior)
            }
            InverseMetadata::NonReversible { reason } => {
                if operation.schema_version() != OperationSchemaVersion::V2
                    || !matches!(mutation, Mutation::BindFloorEvidence { .. })
                    || !matches!(
                        reason,
                        kyberia_operation_log::NonReversibleReason::FloorEvidenceBinding
                    )
                {
                    return Err(MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "invalid non-reversible operation",
                    });
                }
                Ok(())
            }
            InverseMetadata::Toggle { .. } => Err(MaterializationError::UnsupportedOperation {
                operation_id: operation.operation_id(),
                reason: "apply has toggle inverse metadata",
            }),
        },
        OperationPayload::Resolve {
            left,
            right,
            mutation,
        } => {
            let prior = match operation.inverse() {
                InverseMetadata::Apply { mutation } => mutation,
                _ => {
                    return Err(MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "V1 resolution has no mutation prior",
                    });
                }
            };
            validate_resolution_prior(
                operation,
                *left,
                *right,
                mutation.field_key(),
                witness_ids,
                Some(prior),
                None,
                operations,
                ordered,
                work,
                baseline,
                copy_budget,
            )
        }
        OperationPayload::ResolveV2 { left, right, value } => {
            let prior = match operation.inverse() {
                InverseMetadata::ApplyV2 { prior } => prior,
                _ => {
                    return Err(MaterializationError::UnsupportedOperation {
                        operation_id: operation.operation_id(),
                        reason: "V2 resolution has no typed prior",
                    });
                }
            };
            validate_resolution_prior(
                operation,
                *left,
                *right,
                value.field_key(),
                witness_ids,
                None,
                Some(prior),
                operations,
                ordered,
                work,
                baseline,
                copy_budget,
            )
        }
        OperationPayload::Undo { .. } | OperationPayload::Redo { .. } => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_resolution_prior(
    operation: &Operation,
    left: OperationReference,
    right: OperationReference,
    field: FieldKey,
    witness_ids: &BTreeSet<OperationId>,
    legacy_prior: Option<&Mutation>,
    typed_prior: Option<&InversePrior>,
    operations: &OperationSet,
    ordered: &[&Operation],
    work: &mut usize,
    baseline: &Project,
    copy_budget: &mut CopyBudget,
) -> Result<(), MaterializationError> {
    let common = common_causal_state(
        operations,
        ordered,
        witness_ids,
        left,
        right,
        operation.operation_id(),
        work,
        baseline,
        copy_budget,
    )?;
    reject_ambiguous_prior(operation, &common.events, field.clone(), operations, work)?;
    if let Some(prior) = legacy_prior {
        validate_legacy_prior(operation.operation_id(), &common.project, prior)?;
    }
    if let Some(prior) = typed_prior {
        validate_typed_prior(operation.operation_id(), &common.project, prior)?;
    }
    if field_value(&common.project, &field).is_none() {
        return Err(MaterializationError::CausalPriorMismatch {
            operation_id: operation.operation_id(),
            field,
        });
    }
    Ok(())
}

fn validate_legacy_prior(
    operation_id: OperationId,
    before: &Project,
    prior: &Mutation,
) -> Result<(), MaterializationError> {
    let matches = match prior {
        Mutation::SetProjectName { name } => {
            field_value(before, &FieldKey::ProjectName)
                == Some(CausalValue::ProjectName(name.clone()))
        }
        Mutation::SetSiteName { site_id, name } => {
            field_value(before, &FieldKey::SiteName(*site_id))
                == Some(CausalValue::SiteName(name.clone()))
        }
        Mutation::ActivateCalibration {
            map_id,
            calibration_id,
        } => {
            field_value(before, &FieldKey::MapCalibration(*map_id))
                == Some(CausalValue::Calibration(Evidence::Known(*calibration_id)))
        }
        Mutation::BindFloorEvidence { .. } => false,
    };
    if matches {
        Ok(())
    } else {
        Err(MaterializationError::CausalPriorMismatch {
            operation_id,
            field: prior.field_key(),
        })
    }
}

fn validate_typed_prior(
    operation_id: OperationId,
    before: &Project,
    prior: &InversePrior,
) -> Result<(), MaterializationError> {
    let matches = match prior {
        InversePrior::ProjectName { name } => {
            field_value(before, &FieldKey::ProjectName)
                == Some(CausalValue::ProjectName(name.clone()))
        }
        InversePrior::SiteName { site_id, name } => {
            field_value(before, &FieldKey::SiteName(*site_id))
                == Some(CausalValue::SiteName(name.clone()))
        }
        InversePrior::MapCalibration {
            map_id,
            calibration,
        } => {
            field_value(before, &FieldKey::MapCalibration(*map_id))
                == Some(CausalValue::Calibration(calibration.clone()))
        }
    };
    if matches {
        Ok(())
    } else {
        Err(MaterializationError::CausalPriorMismatch {
            operation_id,
            field: prior.field_key(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CausalValue {
    ProjectName(Text),
    SiteName(Text),
    Calibration(Evidence<CalibrationId>),
}

fn field_value(project: &Project, field: &FieldKey) -> Option<CausalValue> {
    match field {
        FieldKey::ProjectName => Some(CausalValue::ProjectName(project.name().clone())),
        FieldKey::SiteName(site_id) => project
            .site(*site_id)
            .map(|site| CausalValue::SiteName(site.name.clone())),
        FieldKey::MapCalibration(map_id) => project
            .map(*map_id)
            .and_then(|_| project.active_calibration(*map_id))
            .map(CausalValue::Calibration),
        FieldKey::FloorEvidence(_) => None,
    }
}

fn reject_cross_field_aggregate_conflicts(
    baseline: &Project,
    operations: &OperationSet,
    effects: &[AppliedEffect],
) -> Result<(), MaterializationError> {
    let mut locks: BTreeMap<FloorId, Vec<OperationId>> = BTreeMap::new();
    let mut calibrations: BTreeMap<FloorId, Vec<OperationId>> = BTreeMap::new();
    for effect in effects {
        if let Some(Mutation::BindFloorEvidence { floor_id, .. }) = effect.mutation() {
            locks
                .entry(*floor_id)
                .or_default()
                .push(effect.operation_id());
        }
        let map_id = match effect {
            AppliedEffect::Calibration { map_id, .. } => Some(*map_id),
            AppliedEffect::Mutation(applied) => match applied.mutation() {
                Mutation::ActivateCalibration { map_id, .. } => Some(*map_id),
                _ => None,
            },
        };
        if let Some(map_id) = map_id
            && let Some(map) = baseline.map(map_id)
        {
            calibrations
                .entry(map.data().floor_id)
                .or_default()
                .push(effect.operation_id());
        }
    }
    let mut checks = 0usize;
    for (floor_id, lock_ids) in locks {
        for evidence_operation in lock_ids {
            for calibration_operation in calibrations.get(&floor_id).into_iter().flatten() {
                if evidence_operation == *calibration_operation {
                    continue;
                }
                checks = checks
                    .checked_add(1)
                    .ok_or(MaterializationError::ResourceLimit(
                        "aggregate_conflict_work",
                    ))?;
                if checks > MAX_AGGREGATE_CONFLICT_CHECKS {
                    return Err(MaterializationError::ResourceLimit(
                        "aggregate_conflict_work",
                    ));
                }
                let evidence_ancestor = is_ancestor(
                    operations,
                    evidence_operation,
                    *calibration_operation,
                    &mut checks,
                )?;
                let calibration_ancestor = is_ancestor(
                    operations,
                    *calibration_operation,
                    evidence_operation,
                    &mut checks,
                )?;
                if !evidence_ancestor && !calibration_ancestor {
                    return Err(MaterializationError::AggregateConflict {
                        floor_id,
                        evidence_operation,
                        calibration_operation: *calibration_operation,
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_domain::{
        evidence::UnknownReason,
        identity::{ActorDeviceId, ActorId, ProjectId, SiteId},
        project::CommandRequest,
        spatial::{
            CalibrationControls, CoordinateFrame, FrameKind, ImageYAxis, PixelPoint, Point2,
            Point3, TwoPointCalibration,
        },
        units::{CoordinateMeters, Meters, Pixels, Radians},
    };
    use std::num::NonZeroU64;

    fn id(value: u8) -> OperationId {
        OperationId::from_bytes([value; 16]).unwrap()
    }

    fn actor(value: u8) -> ActorId {
        ActorId::from_bytes([value; 16]).unwrap()
    }

    fn device(value: u8) -> kyberia_operation_log::DeviceId {
        ActorDeviceId::from_bytes([value; 16]).unwrap()
    }

    fn project() -> Project {
        Project::new(
            ProjectId::from_bytes([1; 16]).unwrap(),
            Text::new("Home").unwrap(),
        )
    }

    fn with_site(project: Project) -> Project {
        let operation_id = id(200);
        project
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id,
                project_id: project.id(),
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(1).unwrap(),
                expected_revision: project.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::CreateSite(kyberia_domain::project::Site {
                    id: SiteId::from_bytes([2; 16]).unwrap(),
                    name: Text::new("Site").unwrap(),
                }),
            })
            .unwrap()
            .project
    }

    fn frame(value: u8, kind: FrameKind) -> CoordinateFrame {
        CoordinateFrame {
            id: kyberia_domain::identity::FrameId::from_bytes([value; 16]).unwrap(),
            name: Text::new("frame").unwrap(),
            kind,
        }
    }

    fn full_project() -> Project {
        let mut state = project();
        let commands = [
            ProjectCommand::CreateSite(kyberia_domain::project::Site {
                id: SiteId::from_bytes([1; 16]).unwrap(),
                name: Text::new("Site").unwrap(),
            }),
            ProjectCommand::CreateBuilding(
                kyberia_domain::project::Building::new(kyberia_domain::project::BuildingData {
                    id: kyberia_domain::identity::BuildingId::from_bytes([1; 16]).unwrap(),
                    site_id: SiteId::from_bytes([1; 16]).unwrap(),
                    name: Text::new("Building").unwrap(),
                    frame: frame(1, FrameKind::BuildingLocalMeters),
                })
                .unwrap(),
            ),
            ProjectCommand::CreateFloor(
                kyberia_domain::project::Floor::new(kyberia_domain::project::FloorData {
                    id: kyberia_domain::identity::FloorId::from_bytes([1; 16]).unwrap(),
                    building_id: kyberia_domain::identity::BuildingId::from_bytes([1; 16]).unwrap(),
                    name: Text::new("Ground").unwrap(),
                    frame: frame(2, FrameKind::FloorLocalMeters),
                    building_frame: kyberia_domain::identity::FrameId::from_bytes([1; 16]).unwrap(),
                    origin: Point3 {
                        x: CoordinateMeters::new(0.).unwrap(),
                        y: CoordinateMeters::new(0.).unwrap(),
                        z: CoordinateMeters::new(0.).unwrap(),
                    },
                    yaw: Radians::new(0.).unwrap(),
                    clear_height: Meters::new(2.5).unwrap(),
                })
                .unwrap(),
            ),
            ProjectCommand::ImportMap(
                kyberia_domain::project::MapAsset::new(kyberia_domain::project::MapAssetData {
                    id: MapAssetId::from_bytes([1; 16]).unwrap(),
                    floor_id: kyberia_domain::identity::FloorId::from_bytes([1; 16]).unwrap(),
                    name: Text::new("Map").unwrap(),
                    image_frame: frame(3, FrameKind::ImagePixels),
                    width: std::num::NonZeroU32::new(200).unwrap(),
                    height: std::num::NonZeroU32::new(200).unwrap(),
                    source: kyberia_domain::evidence::ArtifactReference {
                        sha256: kyberia_domain::identity::ContentHash::from_sha256([1; 32]),
                        media_type: Text::new("image/png").unwrap(),
                        byte_length: 42,
                    },
                    provenance: Text::new("fixture").unwrap(),
                })
                .unwrap(),
            ),
        ];
        for (index, command) in commands.into_iter().enumerate() {
            let operation_id = id((index + 1) as u8);
            state = state
                .execute(CommandRequest {
                    schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                    operation_id,
                    project_id: state.id(),
                    actor_id: actor(1),
                    device_id: device(1),
                    logical_time: NonZeroU64::new(state.logical_time() + 1).unwrap(),
                    expected_revision: state.revision(),
                    wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                    command,
                })
                .unwrap()
                .project;
        }
        state
    }

    fn calibration_for_source(source_frame: u8) -> TwoPointCalibration {
        TwoPointCalibration::new(CalibrationControls {
            source_frame: kyberia_domain::identity::FrameId::from_bytes([source_frame; 16])
                .unwrap(),
            target_frame: kyberia_domain::identity::FrameId::from_bytes([2; 16]).unwrap(),
            image_first: PixelPoint {
                x: Pixels::new(10.).unwrap(),
                y: Pixels::new(20.).unwrap(),
            },
            image_second: PixelPoint {
                x: Pixels::new(110.).unwrap(),
                y: Pixels::new(20.).unwrap(),
            },
            target_origin: Point2 {
                x: CoordinateMeters::new(2.).unwrap(),
                y: CoordinateMeters::new(3.).unwrap(),
            },
            known_distance: Meters::new(10.).unwrap(),
            target_direction: Radians::new(0.).unwrap(),
            image_y_axis: ImageYAxis::Down,
            distance_uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
            control_point_uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
        })
        .unwrap()
    }

    fn calibration() -> TwoPointCalibration {
        calibration_for_source(3)
    }

    fn two_map_project() -> Project {
        let baseline = full_project();
        let map = baseline
            .map(MapAssetId::from_bytes([1; 16]).unwrap())
            .unwrap()
            .data()
            .clone();
        let mut second_map = map;
        second_map.id = MapAssetId::from_bytes([2; 16]).unwrap();
        second_map.image_frame = frame(4, FrameKind::ImagePixels);
        second_map.source.sha256 = kyberia_domain::identity::ContentHash::from_sha256([2; 32]);
        baseline
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id: id(5),
                project_id: baseline.id(),
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(5).unwrap(),
                expected_revision: baseline.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::ImportMap(
                    kyberia_domain::project::MapAsset::new(second_map).unwrap(),
                ),
            })
            .unwrap()
            .project
    }

    fn baseline_with_two_calibrations() -> Project {
        let mut state = full_project();
        for (operation_id, calibration_id) in [(5_u8, [10_u8; 16]), (6_u8, [11_u8; 16])] {
            state = state
                .execute(CommandRequest {
                    schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                    operation_id: id(operation_id),
                    project_id: state.id(),
                    actor_id: actor(1),
                    device_id: device(1),
                    logical_time: NonZeroU64::new(state.logical_time() + 1).unwrap(),
                    expected_revision: state.revision(),
                    wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                    command: ProjectCommand::CalibrateMap(
                        kyberia_domain::project::MapCalibration {
                            id: CalibrationId::from_bytes(calibration_id).unwrap(),
                            map_id: MapAssetId::from_bytes([1; 16]).unwrap(),
                            transform: calibration(),
                            provenance: Text::new("fixture").unwrap(),
                            method_version: Text::new("two-point/v1").unwrap(),
                        },
                    ),
                })
                .unwrap()
                .project;
        }
        state
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id: id(7),
                project_id: state.id(),
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(state.logical_time() + 1).unwrap(),
                expected_revision: state.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::ActivateCalibration {
                    map_id: MapAssetId::from_bytes([1; 16]).unwrap(),
                    calibration: Evidence::Known(CalibrationId::from_bytes([10; 16]).unwrap()),
                },
            })
            .unwrap()
            .project
    }

    fn set_project_name(
        operation_id: OperationId,
        logical_time: u64,
        actor_id: u8,
        parents: Vec<OperationId>,
        name: &str,
        prior: &str,
    ) -> Operation {
        Operation::try_apply_v2(
            operation_id,
            project().id(),
            actor(actor_id),
            device(actor_id),
            kyberia_operation_log::LogicalTimestamp::new(logical_time).unwrap(),
            kyberia_operation_log::CausalDepth::new(if parents.is_empty() { 0 } else { 1 }),
            parents,
            Mutation::set_project_name(Text::new(name).unwrap()),
            InversePrior::ProjectName {
                name: Text::new(prior).unwrap(),
            },
        )
        .unwrap()
    }

    fn set_site_name(
        operation_id: OperationId,
        logical_time: u64,
        actor_id: u8,
        parents: Vec<OperationId>,
        site_id: SiteId,
        name: &str,
        prior: &str,
    ) -> Operation {
        Operation::try_apply_v2(
            operation_id,
            project().id(),
            actor(actor_id),
            device(actor_id),
            kyberia_operation_log::LogicalTimestamp::new(logical_time).unwrap(),
            kyberia_operation_log::CausalDepth::new(if parents.is_empty() { 0 } else { 1 }),
            parents,
            Mutation::set_site_name(site_id, Text::new(name).unwrap()),
            InversePrior::SiteName {
                site_id,
                name: Text::new(prior).unwrap(),
            },
        )
        .unwrap()
    }

    fn criss_cross_operations(prior: &str) -> Vec<Operation> {
        let baseline = with_site(project());
        let project_head = set_project_name(id(10), 2, 10, vec![], "Project", "Home");
        let site_id = SiteId::from_bytes([2; 16]).unwrap();
        let site_head = set_site_name(id(20), 2, 20, vec![], site_id, "Branch", "Site");
        let left = set_project_name(
            id(30),
            3,
            30,
            vec![project_head.operation_id(), site_head.operation_id()],
            "Left",
            "Project",
        );
        let right = set_project_name(
            id(40),
            3,
            40,
            vec![project_head.operation_id(), site_head.operation_id()],
            "Right",
            "Project",
        );
        let resolution = Operation::try_resolve_v2(
            id(50),
            baseline.id(),
            actor(50),
            device(50),
            kyberia_operation_log::LogicalTimestamp::new(4).unwrap(),
            kyberia_operation_log::CausalDepth::new(2),
            vec![left.operation_id(), right.operation_id()],
            OperationReference::from(&left),
            OperationReference::from(&right),
            Mutation::set_project_name(Text::new("Left").unwrap()),
            InversePrior::ProjectName {
                name: Text::new(prior).unwrap(),
            },
        )
        .unwrap();
        vec![project_head, site_head, left, right, resolution]
    }

    fn numbered_operation_id(number: usize) -> OperationId {
        let mut bytes = [0u8; 16];
        bytes[0] = 7;
        bytes[8..].copy_from_slice(&(number as u64).to_be_bytes());
        OperationId::from_bytes(bytes).unwrap()
    }

    fn numbered_site_id(number: usize) -> SiteId {
        let mut bytes = [0u8; 16];
        bytes[0] = 8;
        bytes[8..].copy_from_slice(&(number as u64).to_be_bytes());
        SiteId::from_bytes(bytes).unwrap()
    }

    fn baseline_with_sites(count: usize) -> Project {
        let mut baseline = project();
        for index in 0..count {
            let site_id = numbered_site_id(index);
            baseline = baseline
                .execute(CommandRequest {
                    schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                    operation_id: numbered_operation_id(index + 1),
                    project_id: baseline.id(),
                    actor_id: actor(1),
                    device_id: device(1),
                    logical_time: NonZeroU64::new(index as u64 + 1).unwrap(),
                    expected_revision: baseline.revision(),
                    wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                    command: ProjectCommand::CreateSite(kyberia_domain::project::Site {
                        id: site_id,
                        name: Text::new(format!("site-{index}")).unwrap(),
                    }),
                })
                .unwrap()
                .project;
        }
        baseline
    }

    fn chained_large_name_operations(
        project_id: kyberia_domain::identity::ProjectId,
        baseline_logical_time: u64,
        count: usize,
    ) -> Vec<Operation> {
        let mut operations = Vec::new();
        let mut parents = Vec::new();
        for index in 0..count {
            let current = format!("name-{index:04}-{}", "x".repeat(900));
            let operation_id = numbered_operation_id(index + 10_000);
            operations.push(
                Operation::try_apply_v2(
                    operation_id,
                    project_id,
                    actor(7),
                    device(7),
                    kyberia_operation_log::LogicalTimestamp::new(
                        baseline_logical_time + index as u64 + 1,
                    )
                    .unwrap(),
                    kyberia_operation_log::CausalDepth::new(index as u64),
                    parents,
                    Mutation::set_site_name(numbered_site_id(index), Text::new(current).unwrap()),
                    InversePrior::SiteName {
                        site_id: numbered_site_id(index),
                        name: Text::new(format!("site-{index}")).unwrap(),
                    },
                )
                .unwrap(),
            );
            parents = vec![operation_id];
        }
        operations
    }

    #[test]
    fn empty_set_is_identity_bound_without_mutating_baseline() {
        let project = Project::new(
            ProjectId::from_bytes([1; 16]).unwrap(),
            Text::new("baseline").unwrap(),
        );
        let set = OperationSet::empty(project.id());
        let result = materialize(&project, &set).unwrap();
        assert_eq!(result.project(), &project);
        assert_eq!(result.identity().operation_count(), 0);
    }

    #[test]
    fn equal_lamport_independent_operations_materialize_as_v2() {
        let baseline = with_site(project());
        let project_id = baseline.id();
        let first = Operation::try_apply_v2(
            id(10),
            project_id,
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::set_project_name(Text::new("Office").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Home").unwrap(),
            },
        )
        .unwrap();
        let second = Operation::try_apply_v2(
            id(20),
            project_id,
            actor(20),
            device(20),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::set_site_name(
                SiteId::from_bytes([2; 16]).unwrap(),
                Text::new("HQ").unwrap(),
            ),
            InversePrior::SiteName {
                site_id: SiteId::from_bytes([2; 16]).unwrap(),
                name: Text::new("Site").unwrap(),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([first, second]).unwrap();
        let materialized = materialize(&baseline, &set).unwrap();
        assert_eq!(
            materialized.project().schema_version(),
            ProjectSchemaVersion::V2
        );
        assert_eq!(materialized.project().name().as_str(), "Office");
        assert_eq!(
            materialized
                .project()
                .site(SiteId::from_bytes([2; 16]).unwrap())
                .unwrap()
                .name
                .as_str(),
            "HQ"
        );
        assert_eq!(materialized.project().revision(), baseline.revision() + 2);
        assert_eq!(materialized.project().logical_time(), 2);
        let bytes = serde_json::to_vec(materialized.project()).unwrap();
        let decoded: Project = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, *materialized.project());
    }

    #[test]
    fn project_and_site_renames_preserve_the_canonical_geometry() {
        let baseline = full_project();
        let project_id = baseline.id();
        let site_id = SiteId::from_bytes([1; 16]).unwrap();
        let floor_id = kyberia_domain::identity::FloorId::from_bytes([1; 16]).unwrap();
        let map_id = MapAssetId::from_bytes([1; 16]).unwrap();
        let project_name = Operation::try_apply_v2(
            id(10),
            project_id,
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(5).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::set_project_name(Text::new("Office").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Home").unwrap(),
            },
        )
        .unwrap();
        let site_name = Operation::try_apply_v2(
            id(20),
            project_id,
            actor(20),
            device(20),
            kyberia_operation_log::LogicalTimestamp::new(5).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::set_site_name(site_id, Text::new("HQ").unwrap()),
            InversePrior::SiteName {
                site_id,
                name: Text::new("Site").unwrap(),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([site_name, project_name]).unwrap();
        let result = materialize(&baseline, &set).unwrap();
        assert_eq!(result.project().name().as_str(), "Office");
        assert_eq!(result.project().site(site_id).unwrap().name.as_str(), "HQ");
        assert_eq!(result.project().floor(floor_id), baseline.floor(floor_id));
        assert_eq!(result.project().map(map_id), baseline.map(map_id));
    }

    #[test]
    fn forged_structural_prior_is_rejected_against_causal_baseline() {
        let baseline = project();
        let operation = set_project_name(id(10), 1, 10, vec![], "Office", "Forged");
        let set = OperationSet::from_operations([operation]).unwrap();
        let error = materialize(&baseline, &set).unwrap_err();
        assert!(matches!(
            error,
            MaterializationError::CausalPriorMismatch {
                field: FieldKey::ProjectName,
                ..
            }
        ));
    }

    #[test]
    fn resolved_independent_branches_use_common_ancestor_prior() {
        let baseline = project();
        let left = set_project_name(id(10), 1, 10, vec![], "Office", "Home");
        let right = set_project_name(id(20), 1, 20, vec![], "Lab", "Home");
        let resolve = Operation::try_resolve_v2(
            id(30),
            baseline.id(),
            actor(30),
            device(30),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(1),
            vec![id(10), id(20)],
            OperationReference::from(&left),
            OperationReference::from(&right),
            Mutation::set_project_name(Text::new("Office").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Home").unwrap(),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([right, resolve, left]).unwrap();
        let result = materialize(&baseline, &set).unwrap();
        assert_eq!(result.project().name().as_str(), "Office");
    }

    #[test]
    fn forged_prior_in_a_resolved_branch_is_rejected_before_resolution() {
        let baseline = project();
        let original_baseline = baseline.clone();
        let forged_left = set_project_name(id(10), 1, 10, vec![], "Office", "Forged");
        let valid_right = set_project_name(id(20), 1, 20, vec![], "Lab", "Home");
        let resolution = Operation::try_resolve_v2(
            id(30),
            baseline.id(),
            actor(30),
            device(30),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(1),
            vec![forged_left.operation_id(), valid_right.operation_id()],
            OperationReference::from(&forged_left),
            OperationReference::from(&valid_right),
            Mutation::set_project_name(Text::new("Lab").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Home").unwrap(),
            },
        )
        .unwrap();
        let operations = vec![forged_left, valid_right, resolution];
        let set = OperationSet::from_operations(operations.clone()).unwrap();
        assert!(set.replay_effects().is_ok());
        assert_eq!(
            materialize(&baseline, &set),
            Err(MaterializationError::CausalPriorMismatch {
                operation_id: id(10),
                field: FieldKey::ProjectName,
            })
        );
        assert_eq!(baseline, original_baseline);

        let mut reversed = operations;
        reversed.reverse();
        let reversed_set = OperationSet::from_operations(reversed).unwrap();
        assert_eq!(
            materialize(&baseline, &reversed_set),
            Err(MaterializationError::CausalPriorMismatch {
                operation_id: id(10),
                field: FieldKey::ProjectName,
            })
        );
        assert_eq!(baseline, original_baseline);
    }

    #[test]
    fn edit_after_resolution_uses_resolved_frontier_not_old_conflict_arms() {
        let baseline = project();
        let left = set_project_name(id(10), 1, 10, vec![], "Office", "Home");
        let right = set_project_name(id(20), 1, 20, vec![], "Lab", "Home");
        let resolve = Operation::try_resolve_v2(
            id(30),
            baseline.id(),
            actor(30),
            device(30),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(1),
            vec![id(10), id(20)],
            OperationReference::from(&left),
            OperationReference::from(&right),
            Mutation::set_project_name(Text::new("Office").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Home").unwrap(),
            },
        )
        .unwrap();
        let child = Operation::try_apply_v2(
            id(40),
            baseline.id(),
            actor(40),
            device(40),
            kyberia_operation_log::LogicalTimestamp::new(3).unwrap(),
            kyberia_operation_log::CausalDepth::new(2),
            vec![id(30)],
            Mutation::set_project_name(Text::new("HQ").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Office").unwrap(),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([child, right, resolve, left]).unwrap();
        let result = materialize(&baseline, &set).unwrap();
        assert_eq!(result.project().name().as_str(), "HQ");
    }

    #[test]
    fn baseline_identity_and_existing_operation_ids_are_admission_bound() {
        let baseline = with_site(project());
        let collision = Operation::try_apply_v2(
            id(200),
            baseline.id(),
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::set_project_name(Text::new("Office").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Home").unwrap(),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([collision]).unwrap();
        assert!(matches!(
            materialize(&baseline, &set),
            Err(MaterializationError::BaselineOperationCollision(_))
        ));

        let other_project = Project::new(
            ProjectId::from_bytes([9; 16]).unwrap(),
            Text::new("Other").unwrap(),
        );
        assert!(matches!(
            materialize(&other_project, &set),
            Err(MaterializationError::WrongProject)
        ));
    }

    #[test]
    fn root_operation_must_advance_the_baseline_logical_time() {
        let baseline = with_site(project());
        let operation = set_project_name(id(10), 1, 10, vec![], "Office", "Home");
        let set = OperationSet::from_operations([operation]).unwrap();
        assert!(matches!(
            materialize(&baseline, &set),
            Err(MaterializationError::BaselineLogicalTimeConflict { .. })
        ));
    }

    #[test]
    fn missing_site_is_rejected_before_canonical_project_output() {
        let baseline = project();
        let missing_site = SiteId::from_bytes([77; 16]).unwrap();
        let operation = Operation::try_apply_v2(
            id(10),
            baseline.id(),
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(1).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::set_site_name(missing_site, Text::new("nowhere").unwrap()),
            InversePrior::SiteName {
                site_id: missing_site,
                name: Text::new("prior").unwrap(),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([operation]).unwrap();
        assert_eq!(
            materialize(&baseline, &set),
            Err(MaterializationError::CausalPriorMismatch {
                operation_id: id(10),
                field: FieldKey::SiteName(missing_site),
            })
        );
    }

    #[test]
    fn known_typed_and_mutation_calibration_frontier_values_are_equivalent() {
        let baseline = baseline_with_two_calibrations();
        let project_id = baseline.id();
        let map_id = MapAssetId::from_bytes([1; 16]).unwrap();
        let known_calibration =
            |value: u8| Evidence::Known(CalibrationId::from_bytes([value; 16]).unwrap());
        let target = Operation::try_apply_v2(
            id(10),
            project_id,
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(8).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_id, CalibrationId::from_bytes([11; 16]).unwrap()),
            InversePrior::MapCalibration {
                map_id,
                calibration: known_calibration(10),
            },
        )
        .unwrap();
        let same_value = Operation::try_apply_v2(
            id(20),
            project_id,
            actor(20),
            device(20),
            kyberia_operation_log::LogicalTimestamp::new(8).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_id, CalibrationId::from_bytes([10; 16]).unwrap()),
            InversePrior::MapCalibration {
                map_id,
                calibration: known_calibration(10),
            },
        )
        .unwrap();
        let undo = Operation::try_undo_v2(
            id(30),
            project_id,
            actor(30),
            device(30),
            kyberia_operation_log::LogicalTimestamp::new(9).unwrap(),
            kyberia_operation_log::CausalDepth::new(1),
            vec![target.operation_id()],
            OperationReference::from(&target),
        )
        .unwrap();
        let joined = Operation::try_apply_v2(
            id(40),
            project_id,
            actor(40),
            device(40),
            kyberia_operation_log::LogicalTimestamp::new(10).unwrap(),
            kyberia_operation_log::CausalDepth::new(2),
            vec![same_value.operation_id(), undo.operation_id()],
            Mutation::activate_calibration(map_id, CalibrationId::from_bytes([10; 16]).unwrap()),
            InversePrior::MapCalibration {
                map_id,
                calibration: known_calibration(10),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([joined, undo, target, same_value]).unwrap();
        assert!(set.replay_effects().is_ok());
        let result = materialize(&baseline, &set).unwrap();
        assert_eq!(
            result.project().active_calibration(map_id),
            Some(known_calibration(10))
        );
    }

    #[test]
    fn cross_map_calibration_reference_is_rejected_by_materialized_domain() {
        let mut baseline = two_map_project();
        let map_one = MapAssetId::from_bytes([1; 16]).unwrap();
        let map_two = MapAssetId::from_bytes([2; 16]).unwrap();
        let calibration_id = CalibrationId::from_bytes([10; 16]).unwrap();
        baseline = baseline
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id: id(6),
                project_id: baseline.id(),
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(baseline.logical_time() + 1).unwrap(),
                expected_revision: baseline.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::CalibrateMap(kyberia_domain::project::MapCalibration {
                    id: calibration_id,
                    map_id: map_one,
                    transform: calibration(),
                    provenance: Text::new("fixture").unwrap(),
                    method_version: Text::new("two-point/v1").unwrap(),
                }),
            })
            .unwrap()
            .project;
        let operation = Operation::try_apply_v2(
            id(10),
            baseline.id(),
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(baseline.logical_time() + 1).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_two, calibration_id),
            InversePrior::MapCalibration {
                map_id: map_two,
                calibration: Evidence::Unknown(UnknownReason::NotMeasured),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([operation]).unwrap();
        assert_eq!(
            materialize(&baseline, &set),
            Err(MaterializationError::Domain(ProjectError::InvalidReference))
        );
    }

    #[test]
    fn incompatible_calibration_frame_is_rejected_before_materialized_baseline() {
        let baseline = full_project();
        let calibration = kyberia_domain::project::MapCalibration {
            id: CalibrationId::from_bytes([10; 16]).unwrap(),
            map_id: MapAssetId::from_bytes([1; 16]).unwrap(),
            transform: calibration_for_source(4),
            provenance: Text::new("fixture").unwrap(),
            method_version: Text::new("two-point/v1").unwrap(),
        };
        let error = baseline
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id: id(5),
                project_id: baseline.id(),
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(5).unwrap(),
                expected_revision: baseline.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::CalibrateMap(calibration),
            })
            .unwrap_err();
        assert_eq!(error, ProjectError::InvalidReference);
    }

    #[test]
    fn serialized_baseline_with_incompatible_calibration_frame_is_rejected() {
        fn replace_source_frame(value: &mut serde_json::Value) -> bool {
            match value {
                serde_json::Value::Object(object) => {
                    if let Some(source_frame) = object.get_mut("source_frame") {
                        *source_frame = serde_json::Value::String(String::from(
                            kyberia_domain::identity::FrameId::from_bytes([4; 16]).unwrap(),
                        ));
                        return true;
                    }
                    object.values_mut().any(replace_source_frame)
                }
                serde_json::Value::Array(values) => values.iter_mut().any(replace_source_frame),
                _ => false,
            }
        }

        // Project deserialization is the materializer's baseline trust
        // boundary. The operation log can only select a calibration ID; it
        // cannot repair a calibration whose source frame no longer matches
        // its map asset.
        let baseline = baseline_with_two_calibrations();
        let mut wire = serde_json::to_value(&baseline).unwrap();
        assert!(replace_source_frame(&mut wire));
        assert!(serde_json::from_value::<Project>(wire).is_err());
    }

    #[test]
    fn invalid_floor_evidence_entity_is_rejected_without_public_output() {
        let baseline = full_project();
        let missing_floor = FloorId::from_bytes([77; 16]).unwrap();
        let reference = kyberia_operation_log::ImmutableReference::new(
            kyberia_domain::identity::ContentHash::from_sha256([9; 32]),
            Text::new("application/octet-stream").unwrap(),
            10,
        )
        .unwrap();
        let operation = Operation::try_apply_v2_non_reversible(
            id(10),
            baseline.id(),
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(5).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::bind_floor_evidence(missing_floor, reference),
            kyberia_operation_log::NonReversibleReason::FloorEvidenceBinding,
        )
        .unwrap();
        let set = OperationSet::from_operations([operation]).unwrap();
        assert_eq!(
            materialize(&baseline, &set),
            Err(MaterializationError::Domain(ProjectError::MissingEntity))
        );
    }

    #[test]
    fn criss_cross_resolution_uses_all_common_heads_deterministically() {
        let baseline = with_site(project());
        let operations = criss_cross_operations("Project");
        let set = OperationSet::from_operations(operations.clone()).unwrap();
        let mut reversed = operations;
        reversed.reverse();
        let reversed_set = OperationSet::from_operations(reversed).unwrap();
        let first = materialize(&baseline, &set).unwrap();
        let second = materialize(&baseline, &reversed_set).unwrap();
        assert_eq!(first.project(), second.project());
        assert_eq!(first.identity(), second.identity());
        assert_eq!(first.project().name().as_str(), "Left");
        assert_eq!(
            first
                .project()
                .site(SiteId::from_bytes([2; 16]).unwrap())
                .unwrap()
                .name
                .as_str(),
            "Branch"
        );
    }

    #[test]
    fn criss_cross_resolution_rejects_a_forged_common_causal_prior() {
        let baseline = with_site(project());
        let set = OperationSet::from_operations(criss_cross_operations("Forged")).unwrap();
        assert_eq!(
            materialize(&baseline, &set),
            Err(MaterializationError::CausalPriorMismatch {
                operation_id: id(50),
                field: FieldKey::ProjectName,
            })
        );
    }

    #[test]
    fn redo_uses_its_own_operation_identity_and_preserves_final_state() {
        let baseline = project();
        let apply = set_project_name(id(10), 1, 10, vec![], "Office", "Home");
        let undo = Operation::try_undo_v2(
            id(20),
            baseline.id(),
            actor(20),
            device(20),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(1),
            vec![id(10)],
            OperationReference::from(&apply),
        )
        .unwrap();
        let redo = Operation::try_redo_v2(
            id(30),
            baseline.id(),
            actor(30),
            device(30),
            kyberia_operation_log::LogicalTimestamp::new(3).unwrap(),
            kyberia_operation_log::CausalDepth::new(2),
            vec![id(20)],
            OperationReference::from(&apply),
        )
        .unwrap();
        let set = OperationSet::from_operations([redo, apply, undo]).unwrap();
        let result = materialize(&baseline, &set).unwrap();
        assert_eq!(result.project().name().as_str(), "Office");
        assert_eq!(result.project().revision(), 3);
    }

    #[test]
    fn nested_toggle_chain_matches_typed_replay_without_duplicate_ids() {
        let baseline = project();
        let first = set_project_name(id(10), 1, 10, vec![], "Office", "Home");
        let second = Operation::try_apply_v2(
            id(20),
            baseline.id(),
            actor(20),
            device(20),
            kyberia_operation_log::LogicalTimestamp::new(2).unwrap(),
            kyberia_operation_log::CausalDepth::new(1),
            vec![id(10)],
            Mutation::set_project_name(Text::new("HQ").unwrap()),
            InversePrior::ProjectName {
                name: Text::new("Office").unwrap(),
            },
        )
        .unwrap();
        let undo = Operation::try_undo_v2(
            id(30),
            baseline.id(),
            actor(30),
            device(30),
            kyberia_operation_log::LogicalTimestamp::new(3).unwrap(),
            kyberia_operation_log::CausalDepth::new(2),
            vec![id(20)],
            OperationReference::from(&second),
        )
        .unwrap();
        let redo = Operation::try_redo_v2(
            id(40),
            baseline.id(),
            actor(40),
            device(40),
            kyberia_operation_log::LogicalTimestamp::new(4).unwrap(),
            kyberia_operation_log::CausalDepth::new(3),
            vec![id(30)],
            OperationReference::from(&second),
        )
        .unwrap();
        let set = OperationSet::from_operations([redo, undo, first, second]).unwrap();
        let result = materialize(&baseline, &set).unwrap();
        assert_eq!(result.project().name().as_str(), "HQ");
        assert_eq!(result.project().revision(), 4);
    }

    #[test]
    fn oversized_operation_set_is_rejected_before_replay() {
        fn numbered_id(number: usize) -> OperationId {
            let mut bytes = [0u8; 16];
            bytes[0] = 1;
            bytes[8..].copy_from_slice(&(number as u64).to_be_bytes());
            OperationId::from_bytes(bytes).unwrap()
        }
        let baseline = project();
        let operations = (1..=MAX_MATERIALIZATION_OPERATIONS + 1)
            .map(|number| {
                Operation::try_apply_v2(
                    numbered_id(number),
                    baseline.id(),
                    actor(1),
                    device(1),
                    kyberia_operation_log::LogicalTimestamp::new(1).unwrap(),
                    kyberia_operation_log::CausalDepth::new(0),
                    vec![],
                    Mutation::set_project_name(Text::new("same").unwrap()),
                    InversePrior::ProjectName {
                        name: Text::new("Home").unwrap(),
                    },
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let set = OperationSet::from_operations(operations).unwrap();
        assert!(matches!(
            materialize(&baseline, &set),
            Err(MaterializationError::ResourceLimit(
                "materialization_operations"
            ))
        ));
    }

    #[test]
    fn cumulative_causal_copy_budget_accepts_small_and_rejects_repeated_clones() {
        let small_baseline = baseline_with_sites(2);
        let small_operation = Operation::try_apply_v2(
            numbered_operation_id(1000),
            small_baseline.id(),
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(3).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::set_site_name(numbered_site_id(0), Text::new("small-update").unwrap()),
            InversePrior::SiteName {
                site_id: numbered_site_id(0),
                name: Text::new("site-0").unwrap(),
            },
        )
        .unwrap();
        let small = OperationSet::from_operations([small_operation]).unwrap();
        assert!(materialize(&small_baseline, &small).is_ok());

        let large_baseline = baseline_with_sites(100);
        let original_large_baseline = large_baseline.clone();
        let operations =
            chained_large_name_operations(large_baseline.id(), large_baseline.logical_time(), 100);
        let large = OperationSet::from_operations(operations.clone()).unwrap();
        let mut reversed_operations = operations;
        reversed_operations.reverse();
        let reversed_large = OperationSet::from_operations(reversed_operations).unwrap();
        let error = materialize(&large_baseline, &large).unwrap_err();
        let reversed_error = materialize(&large_baseline, &reversed_large).unwrap_err();
        assert!(matches!(
            error,
            MaterializationError::ResourceLimit("causal_copy_bytes")
        ));
        assert_eq!(error, reversed_error);
        assert_eq!(large_baseline, original_large_baseline);
    }

    #[test]
    fn legacy_project_bytes_remain_v1_and_materialized_v2_is_explicit() {
        let baseline = project();
        let bytes = serde_json::to_vec(&baseline).unwrap();
        assert!(
            String::from_utf8(bytes.clone())
                .unwrap()
                .contains("\"schema_version\":\"1\"")
        );
        let decoded: Project = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.schema_version(), ProjectSchemaVersion::V1);
    }

    #[test]
    fn v2_project_with_applied_operation_cannot_have_zero_logical_time() {
        let id_text = String::from(project().id());
        let operation_id = String::from(id(201));
        let wire = serde_json::json!({
            "schema_version": "2",
            "id": id_text,
            "name": "Home",
            "revision": 1,
            "logical_time": 0,
            "sites": {},
            "buildings": {},
            "floors": {},
            "maps": {},
            "calibrations": {},
            "active_calibrations": {},
            "bound_evidence": {},
            "applied_operations": { operation_id: 1 }
        });
        assert!(serde_json::from_value::<Project>(wire).is_err());
    }

    #[test]
    fn legacy_floor_evidence_binding_is_explicitly_unsupported() {
        let baseline = full_project();
        let floor_id = kyberia_domain::identity::FloorId::from_bytes([1; 16]).unwrap();
        let reference = kyberia_operation_log::ImmutableReference::new(
            kyberia_domain::identity::ContentHash::from_sha256([9; 32]),
            Text::new("application/json").unwrap(),
            10,
        )
        .unwrap();
        let mutation = Mutation::bind_floor_evidence(floor_id, reference);
        let operation = Operation::try_apply(
            id(10),
            baseline.id(),
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(5).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            mutation.clone(),
            mutation,
        )
        .unwrap();
        let set = OperationSet::from_operations([operation]).unwrap();
        assert!(matches!(
            materialize(&baseline, &set),
            Err(MaterializationError::UnsupportedOperation { .. })
        ));
    }

    #[test]
    fn unknown_calibration_undo_restores_explicit_unknown_state() {
        let baseline = full_project();
        let project_id = baseline.id();
        let map_id = MapAssetId::from_bytes([1; 16]).unwrap();
        let calibration_id = CalibrationId::from_bytes([1; 16]).unwrap();
        let calibrated = baseline
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id: id(5),
                project_id,
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(5).unwrap(),
                expected_revision: baseline.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::CalibrateMap(kyberia_domain::project::MapCalibration {
                    id: calibration_id,
                    map_id,
                    transform: calibration(),
                    provenance: Text::new("fixture").unwrap(),
                    method_version: Text::new("two-point/v1").unwrap(),
                }),
            })
            .unwrap()
            .project;
        let baseline = calibrated
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id: id(6),
                project_id,
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(6).unwrap(),
                expected_revision: calibrated.revision(),
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::ActivateCalibration {
                    map_id,
                    calibration: Evidence::Unknown(UnknownReason::NotMeasured),
                },
            })
            .unwrap()
            .project;
        let apply = Operation::try_apply_v2(
            id(10),
            project_id,
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(7).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_id, calibration_id),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Unknown(UnknownReason::NotMeasured),
            },
        )
        .unwrap();
        let undo = Operation::try_undo_v2(
            id(20),
            project_id,
            actor(20),
            device(20),
            kyberia_operation_log::LogicalTimestamp::new(8).unwrap(),
            kyberia_operation_log::CausalDepth::new(1),
            vec![id(10)],
            OperationReference::from(&apply),
        )
        .unwrap();
        let set = OperationSet::from_operations([undo, apply]).unwrap();
        let result = materialize(&baseline, &set).unwrap();
        assert_eq!(
            result.project().active_calibration(map_id),
            Some(Evidence::Unknown(UnknownReason::NotMeasured))
        );
        assert!(result.project().calibration(calibration_id).is_some());
    }

    #[test]
    fn concurrent_evidence_lock_and_calibration_are_explicit_aggregate_conflict() {
        let baseline = full_project();
        let project_id = baseline.id();
        let map_id = MapAssetId::from_bytes([1; 16]).unwrap();
        let calibration_id = CalibrationId::from_bytes([1; 16]).unwrap();
        let baseline = baseline
            .execute(CommandRequest {
                schema_version: kyberia_domain::evidence::SchemaVersion::V1,
                operation_id: id(5),
                project_id,
                actor_id: actor(1),
                device_id: device(1),
                logical_time: NonZeroU64::new(5).unwrap(),
                expected_revision: 4,
                wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
                command: ProjectCommand::CalibrateMap(kyberia_domain::project::MapCalibration {
                    id: calibration_id,
                    map_id,
                    transform: calibration(),
                    provenance: Text::new("fixture").unwrap(),
                    method_version: Text::new("two-point/v1").unwrap(),
                }),
            })
            .unwrap()
            .project;
        let bind = Operation::try_apply_v2_non_reversible(
            id(10),
            project_id,
            actor(10),
            device(10),
            kyberia_operation_log::LogicalTimestamp::new(6).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::bind_floor_evidence(
                kyberia_domain::identity::FloorId::from_bytes([1; 16]).unwrap(),
                kyberia_operation_log::ImmutableReference::new(
                    kyberia_domain::identity::ContentHash::from_sha256([9; 32]),
                    Text::new("application/json").unwrap(),
                    10,
                )
                .unwrap(),
            ),
            kyberia_operation_log::NonReversibleReason::FloorEvidenceBinding,
        )
        .unwrap();
        let activate = Operation::try_apply_v2(
            id(20),
            project_id,
            actor(20),
            device(20),
            kyberia_operation_log::LogicalTimestamp::new(6).unwrap(),
            kyberia_operation_log::CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_id, calibration_id),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Known(calibration_id),
            },
        )
        .unwrap();
        let set = OperationSet::from_operations([activate, bind]).unwrap();
        assert!(matches!(
            materialize(&baseline, &set),
            Err(MaterializationError::AggregateConflict { .. })
        ));
    }
}
