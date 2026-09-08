//! Pure numerical RSSI tiles with explicit geometric support policy.
//! See docs/architecture/spatial-analysis.md for assumptions and limitations.
mod model;
mod tile;
pub use model::*;
pub use tile::*;

use kyberia_domain::{
    evidence::Evidence,
    identity::{FloorId, FrameId, ObservationId},
    spatial::PositionCovariance,
    units::{CoordinateMeters, Dbm},
};
use serde::{Deserialize, Serialize};

pub use kyberia_wifi_semantics::{AggregateMethod, SignalAggregate, SignalAlgorithmVersion};

pub const ALGORITHM_VERSION: &str = "kyberia-spatial/2";
pub const MAX_SAMPLES: usize = 100_000;
pub const MAX_CELLS: usize = 100_000;
pub const MAX_NEIGHBORS: usize = 64;
pub const MAX_DISTANCE_EVALUATIONS: usize = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point2 {
    pub x: CoordinateMeters,
    pub y: CoordinateMeters,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub observation_id: ObservationId,
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub position: Point2,
    pub value: Evidence<Dbm>,
    pub position_covariance: Evidence<PositionCovariance>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfiguration(&'static str),
    ResourceLimit(&'static str),
    DuplicateObservation(ObservationId),
    FrameMismatch,
    FloorMismatch,
    NumericalFailure(&'static str),
    AggregationVersionMismatch,
    TemporalAggregationRequiresMonotonicEvidence,
    InvalidAggregationConfiguration(&'static str),
    Cancelled,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
