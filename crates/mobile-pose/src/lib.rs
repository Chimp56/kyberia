//! Platform-neutral contracts for using mobile tracking as spatial evidence.
//!
//! This crate consumes caller-owned, immutable slices and produces a corrected
//! pose at one monotonic instant. It does not acquire camera/IMU data, persist
//! an archive, or claim platform or field accuracy.
mod fusion;
mod model;

pub use fusion::{FusedPose, PoseSupport, PositionCovarianceScope, ValidatedAnchors, fuse_at};
pub use model::{
    AnchorId, AnchorKind, DriftAnchor, PoseError, PoseLimits, PoseSample, PoseTimeline,
    TrackingState,
};
