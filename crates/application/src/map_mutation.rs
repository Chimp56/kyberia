use kyberia_domain::{
    evidence::Evidence,
    identity::{
        ActorDeviceId, ActorId, CalibrationId, ContentHash, FloorId, MapAssetId, OperationId, Text,
    },
    project::MapCalibration,
    spatial::CoordinateFrame,
};
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
