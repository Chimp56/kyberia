//! Validated, borrowed mobile-pose and anchor evidence.
use kyberia_domain::{
    ValidationError,
    evidence::{Evidence, UnknownReason},
    identity::{ClockEpochId, FrameId, PoseId, SessionId, SourceId, Text},
    spatial::{PoseReference, PositionCovariance},
    time::{CaptureTime, MonotonicTimestamp},
    units::{Probability, Radians, Seconds},
};

const HARD_MAX_SAMPLES: usize = 100_000;
const HARD_MAX_ANCHORS: usize = 512;

/// Opaque identity for a surveyed control point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnchorId([u8; 16]);

impl AnchorId {
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, PoseError> {
        if bytes == [0; 16] {
            return Err(PoseError::InvalidIdentity);
        }
        Ok(Self(bytes))
    }

    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }
}

/// What supplied a map-alignment control point. It is provenance, not a
/// numerical accuracy grade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    Manual,
    QrMarker,
    AprilTag,
}

/// Tracking lifecycle reported by the platform provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingState {
    Tracking,
    Limited,
    NotTracking,
}

/// Limits work on untrusted in-memory batches. The timeline and anchor set
/// borrow caller storage, so these limits bound validation/fusion work without
/// claiming to bound memory already owned by the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseLimits {
    maximum_samples: usize,
    maximum_anchors: usize,
    maximum_pose_gap: Seconds,
    maximum_speed_mps: f64,
    position_process_variance_m2_per_second: f64,
}

impl PoseLimits {
    pub fn new(
        maximum_samples: usize,
        maximum_anchors: usize,
        maximum_pose_gap: Seconds,
        maximum_speed_mps: f64,
        position_process_variance_m2_per_second: f64,
    ) -> Result<Self, PoseError> {
        if !(2..=HARD_MAX_SAMPLES).contains(&maximum_samples)
            || !(2..=HARD_MAX_ANCHORS).contains(&maximum_anchors)
            || maximum_pose_gap.get() <= 0.0
            || !maximum_speed_mps.is_finite()
            || maximum_speed_mps <= 0.0
            || !position_process_variance_m2_per_second.is_finite()
            || position_process_variance_m2_per_second < 0.0
        {
            return Err(PoseError::InvalidPolicy);
        }
        Ok(Self {
            maximum_samples,
            maximum_anchors,
            maximum_pose_gap,
            maximum_speed_mps,
            position_process_variance_m2_per_second,
        })
    }

    pub const fn maximum_samples(self) -> usize {
        self.maximum_samples
    }

    pub const fn maximum_anchors(self) -> usize {
        self.maximum_anchors
    }

    pub const fn maximum_pose_gap(self) -> Seconds {
        self.maximum_pose_gap
    }

    pub const fn maximum_speed_mps(self) -> f64 {
        self.maximum_speed_mps
    }

    pub const fn position_process_variance_m2_per_second(self) -> f64 {
        self.position_process_variance_m2_per_second
    }
}

/// One mobile pose sample. Time is retained in both wall/monotonic form when
/// supplied by the source; monotonic time is mandatory for deterministic local
/// interpolation. A known clock model must belong to the same source epoch.
#[derive(Clone, Debug, PartialEq)]
pub struct PoseSample {
    session_id: SessionId,
    source_id: SourceId,
    pose: PoseReference,
    capture_time: CaptureTime,
    monotonic_time: MonotonicTimestamp,
    tracking_state: TrackingState,
    tracking_quality: Evidence<Probability>,
}

impl PoseSample {
    pub fn new(
        session_id: SessionId,
        source_id: SourceId,
        pose: PoseReference,
        capture_time: CaptureTime,
        tracking_state: TrackingState,
        tracking_quality: Evidence<Probability>,
    ) -> Result<Self, PoseError> {
        let monotonic = *capture_time
            .monotonic
            .as_known()
            .ok_or(PoseError::MonotonicTimeUnavailable)?;
        if let Evidence::Known(clock) = &capture_time.synchronization
            && clock.epoch != monotonic.epoch
        {
            return Err(PoseError::ClockEpochMismatch);
        }
        Ok(Self {
            session_id,
            source_id,
            pose,
            capture_time,
            monotonic_time: monotonic,
            tracking_state,
            tracking_quality,
        })
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }

    pub const fn pose(&self) -> &PoseReference {
        &self.pose
    }

    pub const fn capture_time(&self) -> &CaptureTime {
        &self.capture_time
    }

    pub fn monotonic_time(&self) -> MonotonicTimestamp {
        self.monotonic_time
    }

    pub const fn tracking_state(&self) -> TrackingState {
        self.tracking_state
    }

    pub const fn tracking_quality(&self) -> &Evidence<Probability> {
        &self.tracking_quality
    }
}

/// A validated view of one source/session/frame/clock-epoch pose stream.
/// Construction is O(n), performs no allocation, and rejects mixed authority
/// or unordered times. The slice remains borrowed and immutable for its life.
#[derive(Clone, Copy, Debug)]
pub struct PoseTimeline<'a> {
    samples: &'a [PoseSample],
    session_id: SessionId,
    source_id: SourceId,
    source_frame_id: FrameId,
    clock_epoch: ClockEpochId,
    limits: PoseLimits,
}

impl<'a> PoseTimeline<'a> {
    pub fn new(samples: &'a [PoseSample], limits: PoseLimits) -> Result<Self, PoseError> {
        if samples.len() < 2 {
            return Err(PoseError::InsufficientSamples);
        }
        if samples.len() > limits.maximum_samples {
            return Err(PoseError::SampleLimitExceeded);
        }
        let first = &samples[0];
        let first_time = first.monotonic_time();
        let session_id = first.session_id;
        let source_id = first.source_id;
        let source_frame_id = first.pose.frame_id;
        let clock_epoch = first_time.epoch;
        let mut previous = first_time;
        for sample in &samples[1..] {
            let current = sample.monotonic_time();
            if sample.session_id != session_id || sample.source_id != source_id {
                return Err(PoseError::MixedSourceAuthority);
            }
            if sample.pose.frame_id != source_frame_id {
                return Err(PoseError::MixedSourceFrame);
            }
            if current.epoch != clock_epoch {
                return Err(PoseError::ClockEpochMismatch);
            }
            if current.nanoseconds <= previous.nanoseconds {
                return Err(PoseError::UnorderedTime);
            }
            previous = current;
        }
        Ok(Self {
            samples,
            session_id,
            source_id,
            source_frame_id,
            clock_epoch,
            limits,
        })
    }

    pub const fn samples(&self) -> &'a [PoseSample] {
        self.samples
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }

    pub const fn source_frame_id(&self) -> FrameId {
        self.source_frame_id
    }

    pub const fn clock_epoch(&self) -> ClockEpochId {
        self.clock_epoch
    }

    pub const fn limits(&self) -> PoseLimits {
        self.limits
    }
}

/// A manual or visual landmark alignment bound to one exact pose sample.
/// `target_yaw` gives the source-to-map rotation around +z; pitch and roll are
/// retained from tracking and are not corrected by this bounded 2.5D contract.
#[derive(Clone, Debug, PartialEq)]
pub struct DriftAnchor {
    anchor_id: AnchorId,
    pose_id: PoseId,
    at: MonotonicTimestamp,
    target_frame_id: FrameId,
    target_position: kyberia_domain::spatial::Point3,
    target_yaw: Radians,
    alignment_covariance: Evidence<PositionCovariance>,
    kind: AnchorKind,
    method_version: Text,
}

impl DriftAnchor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        anchor_id: AnchorId,
        pose_id: PoseId,
        at: MonotonicTimestamp,
        target_frame_id: FrameId,
        target_position: kyberia_domain::spatial::Point3,
        target_yaw: Radians,
        alignment_covariance: Evidence<PositionCovariance>,
        kind: AnchorKind,
        method_version: Text,
    ) -> Self {
        Self {
            anchor_id,
            pose_id,
            at,
            target_frame_id,
            target_position,
            target_yaw,
            alignment_covariance,
            kind,
            method_version,
        }
    }

    pub const fn id(&self) -> AnchorId {
        self.anchor_id
    }

    pub const fn pose_id(&self) -> PoseId {
        self.pose_id
    }

    pub const fn at(&self) -> MonotonicTimestamp {
        self.at
    }

    pub const fn target_frame_id(&self) -> FrameId {
        self.target_frame_id
    }

    pub const fn target_position(&self) -> kyberia_domain::spatial::Point3 {
        self.target_position
    }

    pub const fn target_yaw(&self) -> Radians {
        self.target_yaw
    }

    pub const fn alignment_covariance(&self) -> &Evidence<PositionCovariance> {
        &self.alignment_covariance
    }

    pub const fn kind(&self) -> AnchorKind {
        self.kind
    }

    pub const fn method_version(&self) -> &Text {
        &self.method_version
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoseError {
    InvalidIdentity,
    InvalidPolicy,
    InsufficientSamples,
    SampleLimitExceeded,
    AnchorLimitExceeded,
    InsufficientAnchors,
    MonotonicTimeUnavailable,
    ClockEpochMismatch,
    MixedSourceAuthority,
    MixedSourceFrame,
    MixedTargetFrame,
    UnorderedTime,
    DuplicateAnchor,
    AnchorPoseMismatch,
    AnchorPoseNotTracking,
    AnchorOrientationUnavailable,
    InvalidArithmetic,
}

impl From<ValidationError> for PoseError {
    fn from(_: ValidationError) -> Self {
        Self::InvalidArithmetic
    }
}

pub(crate) fn unknown_from<T>(evidence: &Evidence<T>) -> UnknownReason {
    match evidence {
        Evidence::Known(_) => UnknownReason::SourceDidNotProvide,
        Evidence::Unknown(reason) => reason.clone(),
    }
}

/// Normalize to [-pi, pi]; exact antipodal deltas use +pi in fusion.
pub(crate) fn wrap_angle(angle: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let wrapped = angle.rem_euclid(tau);
    if wrapped > std::f64::consts::PI {
        wrapped - tau
    } else {
        wrapped
    }
}

pub(crate) fn shortest_angle_delta(start: f64, end: f64) -> f64 {
    let delta = (wrap_angle(end) - wrap_angle(start)).rem_euclid(std::f64::consts::TAU);
    if delta > std::f64::consts::PI {
        delta - std::f64::consts::TAU
    } else {
        delta
    }
}
