use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{ClockEpochId, FrameId, PoseId, SessionId, SourceId, Text},
    spatial::{Orientation, Point3, PoseReference, PositionCovariance},
    time::{CaptureTime, MonotonicTimestamp},
    units::{CoordinateMeters, Probability, Radians, Seconds},
};
use kyberia_mobile_pose::{
    AnchorId, AnchorKind, DriftAnchor, PoseLimits, PoseSample, TrackingState,
};

pub fn session() -> SessionId {
    SessionId::from_bytes([1; 16]).unwrap()
}

pub fn source() -> SourceId {
    SourceId::from_bytes([2; 16]).unwrap()
}

pub fn source_frame() -> FrameId {
    FrameId::from_bytes([3; 16]).unwrap()
}

pub fn target_frame() -> FrameId {
    FrameId::from_bytes([4; 16]).unwrap()
}

pub fn epoch() -> ClockEpochId {
    ClockEpochId::from_bytes([5; 16]).unwrap()
}

pub fn limits(
    maximum_gap_seconds: f64,
    position_process_variance_m2_per_second: f64,
) -> PoseLimits {
    PoseLimits::new(
        128,
        32,
        Seconds::new(maximum_gap_seconds).unwrap(),
        100.0,
        position_process_variance_m2_per_second,
    )
    .unwrap()
}

pub fn timestamp(nanoseconds: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds,
    }
}

pub fn position(x: f64, y: f64, z: f64) -> Point3 {
    Point3 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
        z: CoordinateMeters::new(z).unwrap(),
    }
}

pub fn covariance(variance: f64) -> Evidence<PositionCovariance> {
    Evidence::Known(PositionCovariance::new([variance, 0.0, 0.0, variance, 0.0, variance]).unwrap())
}

pub fn pose_sample(
    pose_number: u8,
    nanoseconds: u64,
    x: f64,
    yaw: Evidence<Radians>,
    covariance: Evidence<PositionCovariance>,
    tracking_state: TrackingState,
    tracking_quality: Evidence<Probability>,
) -> PoseSample {
    let pose = PoseReference {
        pose_id: PoseId::from_bytes([pose_number; 16]).unwrap(),
        frame_id: source_frame(),
        assignment_version: Text::new("source-assignment/v1").unwrap(),
        position: position(x, 0.0, 0.0),
        covariance,
        orientation: yaw.map(|yaw| Orientation {
            yaw,
            pitch: Radians::new(0.0).unwrap(),
            roll: Radians::new(0.0).unwrap(),
        }),
        method_version: Text::new("test-ar-provider/v1").unwrap(),
    };
    let capture_time = CaptureTime {
        wall: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        monotonic: Evidence::Known(timestamp(nanoseconds)),
        synchronization: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
    };
    PoseSample::new(
        session(),
        source(),
        pose,
        capture_time,
        tracking_state,
        tracking_quality,
    )
    .unwrap()
}

pub fn tracked_sample(pose_number: u8, seconds: u64, x: f64, yaw: f64) -> PoseSample {
    pose_sample(
        pose_number,
        seconds * 1_000_000_000,
        x,
        Evidence::Known(Radians::new(yaw).unwrap()),
        covariance(0.04),
        TrackingState::Tracking,
        Evidence::Known(Probability::new(0.9).unwrap()),
    )
}

pub fn anchor(
    number: u8,
    sample: &PoseSample,
    target_x: f64,
    target_y: f64,
    target_yaw: f64,
    alignment_covariance: Evidence<PositionCovariance>,
) -> DriftAnchor {
    DriftAnchor::new(
        AnchorId::from_bytes([number; 16]).unwrap(),
        sample.pose().pose_id,
        sample.monotonic_time(),
        target_frame(),
        position(target_x, target_y, 0.0),
        Radians::new(target_yaw).unwrap(),
        alignment_covariance,
        AnchorKind::Manual,
        Text::new("control-point/v1").unwrap(),
    )
}
