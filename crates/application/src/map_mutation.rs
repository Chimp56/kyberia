use crate::ApplicationError;
use kyberia_domain::{
    evidence::Evidence,
    identity::{
        ActorDeviceId, ActorId, CalibrationId, ContentHash, FloorId, MapAssetId, OperationId, Text,
    },
    project::MapCalibration,
    spatial::CoordinateFrame,
};

/// Stable identity and authorship supplied by an interactive caller. Causal
/// parents, Lamport time, depth, and store revision are deliberately absent;
/// the application derives them from the validated canonical operation set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapIntentAuthority {
    pub operation_id: OperationId,
    pub actor_id: ActorId,
    pub device_id: ActorDeviceId,
    pub committed_utc_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportMapIntent {
    pub authority: MapIntentAuthority,
    pub map_id: MapAssetId,
    pub floor_id: FloorId,
    pub name: Text,
    pub image_frame: CoordinateFrame,
    pub provenance: Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalibrateMapIntent {
    pub authority: MapIntentAuthority,
    pub calibration: MapCalibration,
}
use kyberia_operation_log::{CausalDepth, LogicalTimestamp, ProjectVersion};

/// Immutable operation identity and optimistic local revision supplied by the
/// caller. Parents are part of the canonical operation and are never inferred
/// from wall-clock order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapOperationContext {
    pub operation_id: OperationId,
    pub actor_id: ActorId,
    pub device_id: ActorDeviceId,
    pub logical_time: LogicalTimestamp,
    pub causal_depth: CausalDepth,
    pub parents: Vec<OperationId>,
    pub expected_project_revision: ProjectVersion,
    pub committed_utc_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportMapRequest {
    pub context: MapOperationContext,
    pub map_id: MapAssetId,
    pub floor_id: FloorId,
    pub name: Text,
    pub image_frame: CoordinateFrame,
    pub provenance: Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalibrateMapRequest {
    pub context: MapOperationContext,
    pub calibration: MapCalibration,
    /// Exact active value before this operation. Retaining it in the request
    /// makes retries reproduce identical canonical operation bytes.
    pub prior_active: Evidence<CalibrationId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapMutationReceipt {
    pub operation_id: OperationId,
    pub project_revision: ProjectVersion,
    pub content_hash: ContentHash,
}

/// A durable receipt is authoritative even if the separate current-view
/// readback fails after commit. Callers must surface that state as committed
/// and reconcile by querying, never report it as an uncommitted mutation.
#[derive(Clone, Debug, PartialEq)]
pub struct MapMutationOutcome {
    pub receipt: MapMutationReceipt,
    pub current: Result<crate::CurrentProjectView, ApplicationError>,
}
