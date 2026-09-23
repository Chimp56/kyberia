mod common;

use common::*;
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{ClockEpochId, FrameId, SessionId, SourceId, Text},
    spatial::{Point3, PoseReference},
    time::{CaptureTime, ClockModel, MonotonicTimestamp, UtcTimestamp},
    units::{PartsPerMillion, Probability, Radians, Seconds, SignedSeconds},
};
use kyberia_mobile_pose::{
    AnchorId, AnchorKind, DriftAnchor, PoseError, PoseLimits, PoseSample, PoseTimeline,
    TrackingState, ValidatedAnchors,
};

#[test]
fn timeline_requires_one_source_session_frame_epoch_and_strict_time_order() {
    let limits = limits(5.0, 0.0);
    let first = tracked_sample(40, 0, 0.0, 0.0);
    let second = tracked_sample(41, 1, 1.0, 0.0);
    assert!(PoseTimeline::new(&[second.clone(), first.clone()], limits).is_err());
    assert!(PoseTimeline::new(&[first.clone(), first.clone()], limits).is_err());

    let foreign_source = PoseSample::new(
        session(),
        SourceId::from_bytes([42; 16]).unwrap(),
        second.pose().clone(),
        second.capture_time().clone(),
        second.tracking_state(),
        second.tracking_quality().clone(),
    )
    .unwrap();
    assert_eq!(
        PoseTimeline::new(&[first.clone(), foreign_source], limits).unwrap_err(),
        PoseError::MixedSourceAuthority
    );

    let foreign_session = PoseSample::new(
        SessionId::from_bytes([45; 16]).unwrap(),
        source(),
        second.pose().clone(),
        second.capture_time().clone(),
        second.tracking_state(),
        second.tracking_quality().clone(),
    )
    .unwrap();
    assert_eq!(
        PoseTimeline::new(&[first.clone(), foreign_session], limits).unwrap_err(),
        PoseError::MixedSourceAuthority
    );

    let foreign_frame = PoseReference {
        frame_id: FrameId::from_bytes([43; 16]).unwrap(),
        ..second.pose().clone()
    };
    let foreign_frame = PoseSample::new(
        session(),
        source(),
        foreign_frame,
        second.capture_time().clone(),
        second.tracking_state(),
        second.tracking_quality().clone(),
    )
    .unwrap();
    assert_eq!(
        PoseTimeline::new(&[first.clone(), foreign_frame], limits).unwrap_err(),
        PoseError::MixedSourceFrame
    );

    let other_epoch = ClockEpochId::from_bytes([44; 16]).unwrap();
    let epoch_sample = PoseSample::new(
        session(),
        source(),
        second.pose().clone(),
        CaptureTime {
            wall: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            monotonic: Evidence::Known(MonotonicTimestamp {
                epoch: other_epoch,
                nanoseconds: 1,
            }),
            synchronization: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        },
        second.tracking_state(),
        second.tracking_quality().clone(),
    )
    .unwrap();
    assert_eq!(
        PoseTimeline::new(&[first, epoch_sample], limits).unwrap_err(),
        PoseError::ClockEpochMismatch
    );
}

#[test]
fn known_clock_model_must_name_the_pose_clock_epoch() {
    let sample = tracked_sample(45, 0, 0.0, 0.0);
    let mismatched = ClockModel {
        epoch: ClockEpochId::from_bytes([46; 16]).unwrap(),
        reference_monotonic_nanoseconds: 0,
        reference_utc: UtcTimestamp(0),
        offset_to_reference: Evidence::Known(SignedSeconds::new(0.0).unwrap()),
        drift: Evidence::Known(PartsPerMillion::new(0.0).unwrap()),
        error: Evidence::Known(Seconds::new(0.0).unwrap()),
        method_version: Text::new("clock-test/v1").unwrap(),
    };
    let error = PoseSample::new(
        session(),
        source(),
        sample.pose().clone(),
        CaptureTime {
            wall: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            monotonic: Evidence::Known(timestamp(0)),
            synchronization: Evidence::Known(mismatched),
        },
        TrackingState::Tracking,
        Evidence::Known(Probability::new(0.9).unwrap()),
    )
    .unwrap_err();
    assert_eq!(error, PoseError::ClockEpochMismatch);
}

#[test]
fn timeline_rejects_reused_pose_identity_at_a_distinct_time() {
    let limits = limits(5.0, 0.0);
    let first = tracked_sample(77, 0, 0.0, 0.0);
    let second = tracked_sample(78, 1, 1.0, 0.0);
    let duplicate_identity = PoseReference {
        pose_id: first.pose().pose_id,
        ..second.pose().clone()
    };
    let second = PoseSample::new(
        session(),
        source(),
        duplicate_identity,
        second.capture_time().clone(),
        second.tracking_state(),
        second.tracking_quality().clone(),
    )
    .unwrap();

    assert_eq!(
        PoseTimeline::new(&[first, second], limits).unwrap_err(),
        PoseError::DuplicatePoseId
    );
}

#[test]
fn caller_limits_have_hard_caps_and_apply_before_timeline_work() {
    assert!(PoseLimits::new(100_001, 2, Seconds::new(1.0).unwrap(), 10.0, 0.0,).is_err());
    assert!(PoseLimits::new(2, 513, Seconds::new(1.0).unwrap(), 10.0, 0.0,).is_err());

    let limits = PoseLimits::new(2, 2, Seconds::new(5.0).unwrap(), 10.0, 0.0).unwrap();
    let samples = vec![
        tracked_sample(47, 0, 0.0, 0.0),
        tracked_sample(48, 1, 1.0, 0.0),
        tracked_sample(49, 2, 2.0, 0.0),
    ];
    assert_eq!(
        PoseTimeline::new(&samples, limits).unwrap_err(),
        PoseError::SampleLimitExceeded
    );
}

#[test]
fn anchors_must_bind_to_exact_sample_identity_time_and_one_target_frame() {
    let limits = limits(5.0, 0.0);
    let samples = vec![
        tracked_sample(50, 0, 0.0, 0.0),
        tracked_sample(51, 2, 2.0, 0.0),
    ];
    let timeline = PoseTimeline::new(&samples, limits).unwrap();

    let wrong_pose = DriftAnchor::new(
        AnchorId::from_bytes([52; 16]).unwrap(),
        samples[0].pose().pose_id,
        samples[1].monotonic_time(),
        target_frame(),
        position(0.0, 0.0, 0.0),
        Radians::new(0.0).unwrap(),
        covariance(0.0),
        AnchorKind::Manual,
        Text::new("anchor-test/v1").unwrap(),
    );
    let anchors = vec![
        anchor(53, &samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        wrong_pose,
    ];
    assert_eq!(
        ValidatedAnchors::new(timeline, &anchors).unwrap_err(),
        PoseError::AnchorPoseMismatch
    );

    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let mut anchors = vec![
        anchor(54, &samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(55, &samples[1], 2.0, 0.0, 0.0, covariance(0.0)),
    ];
    anchors[1] = DriftAnchor::new(
        anchors[1].id(),
        anchors[1].pose_id(),
        anchors[1].at(),
        FrameId::from_bytes([56; 16]).unwrap(),
        Point3 {
            x: anchors[1].target_position().x,
            y: anchors[1].target_position().y,
            z: anchors[1].target_position().z,
        },
        anchors[1].target_yaw(),
        anchors[1].alignment_covariance().clone(),
        anchors[1].kind(),
        anchors[1].method_version().clone(),
    );
    assert_eq!(
        ValidatedAnchors::new(timeline, &anchors).unwrap_err(),
        PoseError::MixedTargetFrame
    );
}

#[test]
fn duplicate_anchor_ids_are_rejected_without_building_a_correction_path() {
    let limits = limits(5.0, 0.0);
    let samples = vec![
        tracked_sample(57, 0, 0.0, 0.0),
        tracked_sample(58, 2, 2.0, 0.0),
    ];
    let anchors = vec![
        anchor(59, &samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        DriftAnchor::new(
            AnchorId::from_bytes([59; 16]).unwrap(),
            samples[1].pose().pose_id,
            samples[1].monotonic_time(),
            target_frame(),
            position(2.0, 0.0, 0.0),
            Radians::new(0.0).unwrap(),
            covariance(0.0),
            AnchorKind::Manual,
            Text::new("anchor-test/v1").unwrap(),
        ),
    ];
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    assert_eq!(
        ValidatedAnchors::new(timeline, &anchors).unwrap_err(),
        PoseError::DuplicateAnchor
    );
}
