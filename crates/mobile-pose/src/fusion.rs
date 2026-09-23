//! Deterministic interpolation and anchor-bound drift correction.
use crate::model::{
    AnchorId, DriftAnchor, PoseError, PoseLimits, PoseSample, PoseTimeline, TrackingState,
    shortest_angle_delta, unknown_from, wrap_angle,
};
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{FrameId, PoseId, SessionId, SourceId},
    spatial::{Orientation, Point3, PoseReference, PositionCovariance},
    time::MonotonicTimestamp,
    units::{CoordinateMeters, Probability, Radians},
};

/// A validated anchor view tied to the exact immutable timeline it validated.
/// Its private fields prevent callers from constructing a correction segment
/// and passing it directly to fusion.
#[derive(Clone, Copy, Debug)]
pub struct ValidatedAnchors<'a> {
    timeline: PoseTimeline<'a>,
    anchors: &'a [DriftAnchor],
    target_frame_id: FrameId,
}

impl<'a> ValidatedAnchors<'a> {
    pub fn new(timeline: PoseTimeline<'a>, anchors: &'a [DriftAnchor]) -> Result<Self, PoseError> {
        if anchors.len() < 2 {
            return Err(PoseError::InsufficientAnchors);
        }
        if anchors.len() > timeline.limits().maximum_anchors() {
            return Err(PoseError::AnchorLimitExceeded);
        }
        let target_frame_id = anchors[0].target_frame_id();
        let mut previous_at = None;
        for (index, anchor) in anchors.iter().enumerate() {
            if anchor.at().epoch != timeline.clock_epoch() {
                return Err(PoseError::ClockEpochMismatch);
            }
            if anchor.target_frame_id() != target_frame_id {
                return Err(PoseError::MixedTargetFrame);
            }
            if previous_at.is_some_and(|time| anchor.at().nanoseconds <= time) {
                return Err(PoseError::UnorderedTime);
            }
            previous_at = Some(anchor.at().nanoseconds);

            // At most 512 entries are allowed, so this allocation-free check
            // has a fixed upper bound and catches ambiguous anchor identities.
            if anchors[..index]
                .iter()
                .any(|prior| prior.id() == anchor.id())
            {
                return Err(PoseError::DuplicateAnchor);
            }
            let source_pose = pose_at_exact_time(timeline.samples(), anchor.at())
                .ok_or(PoseError::AnchorPoseMismatch)?;
            if source_pose.pose().pose_id != anchor.pose_id() {
                return Err(PoseError::AnchorPoseMismatch);
            }
            if source_pose.tracking_state() == TrackingState::NotTracking {
                return Err(PoseError::AnchorPoseNotTracking);
            }
            // A translation/yaw correction is underdetermined without both the
            // source orientation at the anchor and the target map orientation.
            source_yaw(source_pose.pose())?;
        }
        Ok(Self {
            timeline,
            anchors,
            target_frame_id,
        })
    }

    pub const fn timeline(&self) -> PoseTimeline<'a> {
        self.timeline
    }

    pub const fn anchors(&self) -> &'a [DriftAnchor] {
        self.anchors
    }

    pub const fn target_frame_id(&self) -> FrameId {
        self.target_frame_id
    }
}

/// Evidence links returned by one interpolation and its two map controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseSupport {
    pub source_pose_ids: [PoseId; 2],
    pub anchor_ids: [AnchorId; 2],
    pub pose_fraction: f64,
    pub anchor_fraction: f64,
}

/// Scope of the numeric position covariance emitted by this 2.5D fusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionCovarianceScope {
    /// Includes translational pose covariance, anchor alignment covariance,
    /// and configured process variance, conditional on supplied orientation
    /// estimates. Orientation uncertainty and frame-calibration uncertainty
    /// are not modeled by this contract.
    ConditionalOnInputOrientation,
}

/// A pose expressed in the anchor target frame. Orientation correction is yaw
/// only; pitch and roll remain sourced from the mobile tracker.
#[derive(Clone, Debug, PartialEq)]
pub struct FusedPose {
    pub at: MonotonicTimestamp,
    pub session_id: SessionId,
    pub source_id: SourceId,
    pub source_frame_id: FrameId,
    pub frame_id: FrameId,
    pub position: Point3,
    pub orientation: Evidence<Orientation>,
    pub position_covariance: Evidence<PositionCovariance>,
    pub position_covariance_scope: PositionCovarianceScope,
    pub tracking_state: TrackingState,
    pub tracking_quality: Evidence<Probability>,
    pub support: PoseSupport,
    pub algorithm_version: &'static str,
}

/// Interpolate a mobile pose and apply only drift corrections bracketed by
/// validated, exact-pose anchors. Queries outside either evidence interval
/// return an explicit unknown rather than extrapolated coordinates.
pub fn fuse_at(
    anchors: &ValidatedAnchors<'_>,
    at: MonotonicTimestamp,
) -> Result<Evidence<FusedPose>, PoseError> {
    let timeline = anchors.timeline;
    if at.epoch != timeline.clock_epoch() {
        return Err(PoseError::ClockEpochMismatch);
    }
    let Some(pose_bracket) = bracket_samples(timeline.samples(), at) else {
        return Ok(Evidence::Unknown(UnknownReason::OutsideEvidenceSupport));
    };
    let Some(anchor_bracket) = bracket_anchors(anchors.anchors, at) else {
        return Ok(Evidence::Unknown(UnknownReason::OutsideEvidenceSupport));
    };

    let pose_gap = bracket_gap_seconds(
        timeline.samples()[pose_bracket.left].monotonic_time(),
        timeline.samples()[pose_bracket.right].monotonic_time(),
    );
    if pose_gap > timeline.limits().maximum_pose_gap().get() {
        return Ok(Evidence::Unknown(UnknownReason::OutsideEvidenceSupport));
    }

    let left_sample = &timeline.samples()[pose_bracket.left];
    let right_sample = &timeline.samples()[pose_bracket.right];
    if left_sample.tracking_state() == TrackingState::NotTracking
        || right_sample.tracking_state() == TrackingState::NotTracking
    {
        return Ok(Evidence::Unknown(UnknownReason::NotObservable));
    }
    if pose_gap > 0.0 {
        let implied_speed =
            distance(left_sample.pose().position, right_sample.pose().position)? / pose_gap;
        if implied_speed > timeline.limits().maximum_speed_mps() {
            return Ok(Evidence::Unknown(UnknownReason::OutsideEvidenceSupport));
        }
    }

    let anchor_left = &anchors.anchors[anchor_bracket.left];
    let anchor_right = &anchors.anchors[anchor_bracket.right];
    let anchor_gap = bracket_gap_seconds(anchor_left.at(), anchor_right.at());
    if anchor_gap > timeline.limits().maximum_pose_gap().get() {
        return Ok(Evidence::Unknown(UnknownReason::OutsideEvidenceSupport));
    }
    let left_correction = anchor_correction(timeline, anchor_left)?;
    let right_correction = anchor_correction(timeline, anchor_right)?;

    let source_position = interpolate_point(
        left_sample.pose().position,
        right_sample.pose().position,
        pose_bracket.fraction,
    )?;
    let correction_yaw = interpolate_angle(
        left_correction.yaw,
        right_correction.yaw,
        anchor_bracket.fraction,
    );
    let correction_translation = interpolate_vector(
        left_correction.translation,
        right_correction.translation,
        anchor_bracket.fraction,
    )?;
    let corrected_position =
        transform_position(source_position, correction_yaw, correction_translation)?;

    let orientation = interpolate_orientation(
        left_sample.pose(),
        right_sample.pose(),
        pose_bracket.fraction,
        correction_yaw,
    )?;
    let position_covariance = fused_covariance(CovarianceSupport {
        samples: [left_sample, right_sample],
        alignments: [left_correction.covariance, right_correction.covariance],
        pose_bracket,
        anchor_bracket,
        anchor_gap,
        correction_yaw,
        limits: timeline.limits(),
    })?;
    let tracking_state = if left_sample.tracking_state() == TrackingState::Limited
        || right_sample.tracking_state() == TrackingState::Limited
    {
        TrackingState::Limited
    } else {
        TrackingState::Tracking
    };
    let tracking_quality = interpolate_quality(
        left_sample.tracking_quality(),
        right_sample.tracking_quality(),
        pose_bracket.fraction,
    )?;

    Ok(Evidence::Known(FusedPose {
        at,
        session_id: timeline.session_id(),
        source_id: timeline.source_id(),
        source_frame_id: timeline.source_frame_id(),
        frame_id: anchors.target_frame_id,
        position: corrected_position,
        orientation,
        position_covariance,
        position_covariance_scope: PositionCovarianceScope::ConditionalOnInputOrientation,
        tracking_state,
        tracking_quality,
        support: PoseSupport {
            source_pose_ids: [left_sample.pose().pose_id, right_sample.pose().pose_id],
            anchor_ids: [anchor_left.id(), anchor_right.id()],
            pose_fraction: pose_bracket.fraction,
            anchor_fraction: anchor_bracket.fraction,
        },
        algorithm_version: "anchor-bound-yaw-translation/v1",
    }))
}

#[derive(Clone, Copy)]
struct Bracket {
    left: usize,
    right: usize,
    fraction: f64,
}

#[derive(Clone)]
struct Correction {
    yaw: f64,
    translation: [f64; 3],
    covariance: Evidence<PositionCovariance>,
}

struct CovarianceSupport<'a> {
    samples: [&'a PoseSample; 2],
    alignments: [Evidence<PositionCovariance>; 2],
    pose_bracket: Bracket,
    anchor_bracket: Bracket,
    anchor_gap: f64,
    correction_yaw: f64,
    limits: PoseLimits,
}

fn bracket_samples(samples: &[PoseSample], at: MonotonicTimestamp) -> Option<Bracket> {
    let first = samples.first()?.monotonic_time().nanoseconds;
    let last = samples.last()?.monotonic_time().nanoseconds;
    if at.nanoseconds < first || at.nanoseconds > last {
        return None;
    }
    match samples.binary_search_by_key(&at.nanoseconds, |sample| {
        sample.monotonic_time().nanoseconds
    }) {
        Ok(index) => Some(Bracket {
            left: index,
            right: index,
            fraction: 0.0,
        }),
        Err(right) => {
            let left = right - 1;
            Some(Bracket {
                left,
                right,
                fraction: fraction(
                    samples[left].monotonic_time().nanoseconds,
                    samples[right].monotonic_time().nanoseconds,
                    at.nanoseconds,
                ),
            })
        }
    }
}

fn bracket_anchors(anchors: &[DriftAnchor], at: MonotonicTimestamp) -> Option<Bracket> {
    let first = anchors.first()?.at().nanoseconds;
    let last = anchors.last()?.at().nanoseconds;
    if at.nanoseconds < first || at.nanoseconds > last {
        return None;
    }
    match anchors.binary_search_by_key(&at.nanoseconds, |anchor| anchor.at().nanoseconds) {
        Ok(index) => Some(Bracket {
            left: index,
            right: index,
            fraction: 0.0,
        }),
        Err(right) => {
            let left = right - 1;
            Some(Bracket {
                left,
                right,
                fraction: fraction(
                    anchors[left].at().nanoseconds,
                    anchors[right].at().nanoseconds,
                    at.nanoseconds,
                ),
            })
        }
    }
}

fn pose_at_exact_time(samples: &[PoseSample], at: MonotonicTimestamp) -> Option<&PoseSample> {
    if samples.first()?.monotonic_time().epoch != at.epoch {
        return None;
    }
    samples
        .binary_search_by_key(&at.nanoseconds, |sample| {
            sample.monotonic_time().nanoseconds
        })
        .ok()
        .map(|index| &samples[index])
}

fn fraction(start: u64, end: u64, at: u64) -> f64 {
    (at - start) as f64 / (end - start) as f64
}

fn bracket_gap_seconds(start: MonotonicTimestamp, end: MonotonicTimestamp) -> f64 {
    (end.nanoseconds - start.nanoseconds) as f64 / 1_000_000_000.0
}

fn anchor_correction(
    timeline: PoseTimeline<'_>,
    anchor: &DriftAnchor,
) -> Result<Correction, PoseError> {
    let sample =
        pose_at_exact_time(timeline.samples(), anchor.at()).ok_or(PoseError::AnchorPoseMismatch)?;
    let source_yaw = source_yaw(sample.pose())?;
    let yaw = shortest_angle_delta(source_yaw, anchor.target_yaw().get());
    let source = sample.pose().position;
    let c = yaw.cos();
    let s = yaw.sin();
    let rotated = [
        c * source.x.get() - s * source.y.get(),
        s * source.x.get() + c * source.y.get(),
        source.z.get(),
    ];
    Ok(Correction {
        yaw,
        translation: [
            anchor.target_position().x.get() - rotated[0],
            anchor.target_position().y.get() - rotated[1],
            anchor.target_position().z.get() - rotated[2],
        ],
        covariance: anchor.alignment_covariance().clone(),
    })
}

fn source_yaw(pose: &PoseReference) -> Result<f64, PoseError> {
    pose.orientation
        .as_known()
        .map(|orientation| wrap_angle(orientation.yaw.get()))
        .ok_or(PoseError::AnchorOrientationUnavailable)
}

fn interpolate_point(a: Point3, b: Point3, u: f64) -> Result<Point3, PoseError> {
    Ok(Point3 {
        x: CoordinateMeters::new(mix(a.x.get(), b.x.get(), u))?,
        y: CoordinateMeters::new(mix(a.y.get(), b.y.get(), u))?,
        z: CoordinateMeters::new(mix(a.z.get(), b.z.get(), u))?,
    })
}

fn interpolate_vector(a: [f64; 3], b: [f64; 3], u: f64) -> Result<[f64; 3], PoseError> {
    let result = [mix(a[0], b[0], u), mix(a[1], b[1], u), mix(a[2], b[2], u)];
    if result.iter().all(|value| value.is_finite()) {
        Ok(result)
    } else {
        Err(PoseError::InvalidArithmetic)
    }
}

fn transform_position(point: Point3, yaw: f64, translation: [f64; 3]) -> Result<Point3, PoseError> {
    let c = yaw.cos();
    let s = yaw.sin();
    Ok(Point3 {
        x: CoordinateMeters::new(c * point.x.get() - s * point.y.get() + translation[0])?,
        y: CoordinateMeters::new(s * point.x.get() + c * point.y.get() + translation[1])?,
        z: CoordinateMeters::new(point.z.get() + translation[2])?,
    })
}

fn interpolate_orientation(
    left: &PoseReference,
    right: &PoseReference,
    u: f64,
    correction_yaw: f64,
) -> Result<Evidence<Orientation>, PoseError> {
    let (Some(left_orientation), Some(right_orientation)) =
        (left.orientation.as_known(), right.orientation.as_known())
    else {
        return Ok(Evidence::Unknown(
            if left.orientation.as_known().is_none() {
                unknown_from(&left.orientation)
            } else {
                unknown_from(&right.orientation)
            },
        ));
    };
    let orientation = Orientation {
        yaw: Radians::new(wrap_angle(
            interpolate_angle(left_orientation.yaw.get(), right_orientation.yaw.get(), u)
                + correction_yaw,
        ))?,
        pitch: Radians::new(interpolate_angle(
            left_orientation.pitch.get(),
            right_orientation.pitch.get(),
            u,
        ))?,
        roll: Radians::new(interpolate_angle(
            left_orientation.roll.get(),
            right_orientation.roll.get(),
            u,
        ))?,
    };
    Ok(Evidence::Known(orientation))
}

fn interpolate_angle(start: f64, end: f64, u: f64) -> f64 {
    wrap_angle(start + shortest_angle_delta(start, end) * u)
}

fn interpolate_quality(
    left: &Evidence<Probability>,
    right: &Evidence<Probability>,
    u: f64,
) -> Result<Evidence<Probability>, PoseError> {
    match (left, right) {
        (Evidence::Known(a), Evidence::Known(b)) => {
            Ok(Evidence::Known(Probability::new(mix(a.get(), b.get(), u))?))
        }
        (Evidence::Unknown(reason), _) | (_, Evidence::Unknown(reason)) => {
            Ok(Evidence::Unknown(reason.clone()))
        }
    }
}

fn fused_covariance(
    support: CovarianceSupport<'_>,
) -> Result<Evidence<PositionCovariance>, PoseError> {
    let [left_sample, right_sample] = support.samples;
    let [left_alignment, right_alignment] = support.alignments;
    let CovarianceSupport {
        pose_bracket,
        anchor_bracket,
        anchor_gap,
        correction_yaw,
        limits,
        ..
    } = support;
    let (Evidence::Known(left_pose), Evidence::Known(right_pose)) = (
        left_sample.pose().covariance.clone(),
        right_sample.pose().covariance.clone(),
    ) else {
        let covariance = if left_sample.pose().covariance.as_known().is_none() {
            &left_sample.pose().covariance
        } else {
            &right_sample.pose().covariance
        };
        return Ok(Evidence::Unknown(unknown_from(covariance)));
    };
    let (Evidence::Known(left_anchor), Evidence::Known(right_anchor)) =
        (left_alignment.clone(), right_alignment.clone())
    else {
        let covariance = if left_alignment.as_known().is_none() {
            &left_alignment
        } else {
            &right_alignment
        };
        return Ok(Evidence::Unknown(unknown_from(covariance)));
    };

    let pose_gap = bracket_gap_seconds(left_sample.monotonic_time(), right_sample.monotonic_time());
    let process_variance = limits.position_process_variance_m2_per_second()
        * (pose_gap * pose_bracket.fraction * (1.0 - pose_bracket.fraction)
            + anchor_gap * anchor_bracket.fraction * (1.0 - anchor_bracket.fraction));
    if !process_variance.is_finite() {
        return Err(PoseError::InvalidArithmetic);
    }

    let pose_covariance =
        interpolate_covariance(left_pose, right_pose, pose_bracket.fraction, 0.0)?;
    let anchor_covariance =
        interpolate_covariance(left_anchor, right_anchor, anchor_bracket.fraction, 0.0)?;
    let rotated_pose = rotate_covariance(pose_covariance, correction_yaw)?;
    // Covariance between pose and its alignment residual is unknown. The
    // factor of two is a Loewner upper bound for arbitrary cross-correlation;
    // it deliberately avoids an unproven independence assumption.
    let total = add_covariances(rotated_pose, anchor_covariance, 2.0)?;
    let with_process = add_isotropic(total, process_variance)?;
    Ok(Evidence::Known(PositionCovariance::new(with_process)?))
}

fn interpolate_covariance(
    left: PositionCovariance,
    right: PositionCovariance,
    u: f64,
    diagonal_variance: f64,
) -> Result<[f64; 6], PoseError> {
    let a = left.packed();
    let b = right.packed();
    let mut out = std::array::from_fn(|index| mix(a[index], b[index], u));
    out[0] += diagonal_variance;
    out[3] += diagonal_variance;
    out[5] += diagonal_variance;
    if out.iter().all(|value| value.is_finite()) {
        Ok(out)
    } else {
        Err(PoseError::InvalidArithmetic)
    }
}

fn rotate_covariance(covariance: [f64; 6], yaw: f64) -> Result<[f64; 6], PoseError> {
    let [xx, xy, xz, yy, yz, zz] = covariance;
    let c = yaw.cos();
    let s = yaw.sin();
    let cc = c * c;
    let ss = s * s;
    let cs = c * s;
    let out = [
        cc * xx - 2.0 * cs * xy + ss * yy,
        cs * xx + (cc - ss) * xy - cs * yy,
        c * xz - s * yz,
        ss * xx + 2.0 * cs * xy + cc * yy,
        s * xz + c * yz,
        zz,
    ];
    if out.iter().all(|value| value.is_finite()) {
        Ok(out)
    } else {
        Err(PoseError::InvalidArithmetic)
    }
}

fn add_covariances(left: [f64; 6], right: [f64; 6], factor: f64) -> Result<[f64; 6], PoseError> {
    let out = std::array::from_fn(|index| factor * (left[index] + right[index]));
    if out.iter().all(|value| value.is_finite()) {
        Ok(out)
    } else {
        Err(PoseError::InvalidArithmetic)
    }
}

fn add_isotropic(mut covariance: [f64; 6], variance: f64) -> Result<[f64; 6], PoseError> {
    covariance[0] += variance;
    covariance[3] += variance;
    covariance[5] += variance;
    if covariance.iter().all(|value| value.is_finite()) {
        Ok(covariance)
    } else {
        Err(PoseError::InvalidArithmetic)
    }
}

fn distance(left: Point3, right: Point3) -> Result<f64, PoseError> {
    let distance = (right.x.get() - left.x.get())
        .hypot(right.y.get() - left.y.get())
        .hypot(right.z.get() - left.z.get());
    if distance.is_finite() {
        Ok(distance)
    } else {
        Err(PoseError::InvalidArithmetic)
    }
}

fn mix(left: f64, right: f64, fraction: f64) -> f64 {
    left.mul_add(1.0 - fraction, right * fraction)
}
