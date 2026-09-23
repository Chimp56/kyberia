mod common;

use common::*;
use kyberia_domain::{evidence::Evidence, units::Radians};
use kyberia_mobile_pose::{
    DriftAnchor, PoseLimits, PoseSample, PoseTimeline, PositionCovarianceScope, TrackingState,
    ValidatedAnchors, fuse_at,
};

fn two_point_track(limits: PoseLimits) -> (Vec<PoseSample>, Vec<DriftAnchor>) {
    let samples = vec![
        tracked_sample(10, 0, 0.0, 0.0),
        tracked_sample(11, 10, 10.0, 0.0),
    ];
    let anchors = vec![
        anchor(20, &samples[0], 100.0, 0.0, 0.0, covariance(0.0)),
        anchor(21, &samples[1], 112.0, 0.0, 0.0, covariance(0.0)),
    ];
    assert_eq!(limits.maximum_samples(), 128);
    (samples, anchors)
}

#[test]
fn applies_interpolated_drift_translation_and_keeps_evidence_links() {
    let limits = limits(11.0, 0.0);
    let (samples, anchors) = two_point_track(limits);
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();

    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("supported pose should fuse")
    };
    assert!((result.position.x.get() - 106.0).abs() < 1e-12);
    assert_eq!(result.position.y.get(), 0.0);
    assert_eq!(result.frame_id, target_frame());
    assert_eq!(result.session_id, session());
    assert_eq!(result.source_id, source());
    assert_eq!(result.source_frame_id, source_frame());
    assert_eq!(
        result.position_covariance_scope,
        PositionCovarianceScope::ConditionalOnInputOrientation
    );
    assert_eq!(result.support.source_pose_ids[0], samples[0].pose().pose_id);
    assert_eq!(result.support.source_pose_ids[1], samples[1].pose().pose_id);
    assert_eq!(
        result.support.anchor_ids,
        [anchors[0].id(), anchors[1].id()]
    );
    assert_eq!(result.algorithm_version, "anchor-bound-yaw-translation/v1");
}

#[test]
fn applies_yaw_rotation_before_translation() {
    let limits = limits(11.0, 0.0);
    let samples = vec![
        tracked_sample(12, 0, 0.0, 0.0),
        tracked_sample(13, 10, 10.0, 0.0),
    ];
    let anchors = vec![
        anchor(
            22,
            &samples[0],
            1.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            covariance(0.0),
        ),
        anchor(
            23,
            &samples[1],
            1.0,
            10.0,
            std::f64::consts::FRAC_PI_2,
            covariance(0.0),
        ),
    ];
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("supported pose should fuse")
    };
    assert!((result.position.x.get() - 1.0).abs() < 1e-12);
    assert!((result.position.y.get() - 5.0).abs() < 1e-12);
    assert!(
        (result.orientation.as_known().unwrap().yaw.get() - std::f64::consts::FRAC_PI_2).abs()
            < 1e-12
    );
}

#[test]
fn interpolates_angles_across_wrap_without_taking_the_long_arc() {
    let limits = limits(11.0, 0.0);
    let left_yaw = 179_f64.to_radians();
    let right_yaw = -179_f64.to_radians();
    let samples = vec![
        tracked_sample(14, 0, 0.0, left_yaw),
        tracked_sample(15, 10, 10.0, right_yaw),
    ];
    let anchors = vec![
        anchor(24, &samples[0], 0.0, 0.0, left_yaw, covariance(0.0)),
        anchor(25, &samples[1], 10.0, 0.0, right_yaw, covariance(0.0)),
    ];
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("supported pose should fuse")
    };
    let fused_yaw = result.orientation.as_known().unwrap().yaw.get();
    assert!((fused_yaw.abs() - std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn uses_positive_pi_for_exact_antipodal_yaw() {
    let policy = limits(11.0, 0.0);
    let samples = vec![
        tracked_sample(16, 0, 0.0, 0.0),
        tracked_sample(17, 10, 10.0, std::f64::consts::PI),
    ];
    let anchors = vec![
        anchor(26, &samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(
            27,
            &samples[1],
            10.0,
            0.0,
            std::f64::consts::PI,
            covariance(0.0),
        ),
    ];
    let validated =
        ValidatedAnchors::new(PoseTimeline::new(&samples, policy).unwrap(), &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("supported pose should fuse")
    };
    assert!(
        (result.orientation.as_known().unwrap().yaw.get() - std::f64::consts::FRAC_PI_2).abs()
            < 1e-12
    );
}

#[test]
fn covariance_bound_does_not_claim_independent_endpoint_samples() {
    let limits = limits(11.0, 0.0);
    let (samples, anchors) = two_point_track(limits);
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("supported pose should fuse")
    };
    let covariance = result.position_covariance.as_known().unwrap().packed();
    // Each correction includes its source anchor-pose covariance under a
    // factor-of-two bound; fusion then applies the same bound between the
    // interpolated query pose and correction.
    assert!((covariance[0] - 0.24).abs() < 1e-12);
    assert!((covariance[3] - 0.24).abs() < 1e-12);
}

#[test]
fn process_variance_grows_uncertainty_between_samples_and_anchors() {
    let limits = limits(11.0, 0.1);
    let (samples, anchors) = two_point_track(limits);
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("supported pose should fuse")
    };
    let variance = result.position_covariance.as_known().unwrap().packed()[0];
    assert!((variance - 0.74).abs() < 1e-12);
}

#[test]
fn anchor_source_covariance_contributes_even_when_query_samples_are_exactly_known() {
    let policy = limits(16.0, 0.0);
    let samples = vec![
        pose_sample(
            60,
            0,
            0.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            covariance(1.0),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
        pose_sample(
            61,
            5_000_000_000,
            5.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            covariance(0.0),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
        pose_sample(
            62,
            10_000_000_000,
            10.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            covariance(0.0),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
        pose_sample(
            63,
            15_000_000_000,
            15.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            covariance(1.0),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
    ];
    let anchors = vec![
        anchor(64, &samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(65, &samples[3], 15.0, 0.0, 0.0, covariance(0.0)),
    ];
    let validated =
        ValidatedAnchors::new(PoseTimeline::new(&samples, policy).unwrap(), &anchors).unwrap();

    let Evidence::Known(result) = fuse_at(&validated, timestamp(7_500_000_000)).unwrap() else {
        panic!("supported pose should fuse")
    };
    let covariance = result.position_covariance.as_known().unwrap().packed();
    assert!((covariance[0] - 4.0).abs() < 1e-12);
    assert!((covariance[3] - 4.0).abs() < 1e-12);
}

#[test]
fn unknown_anchor_source_covariance_keeps_fused_covariance_unknown() {
    let policy = limits(16.0, 0.0);
    let samples = vec![
        pose_sample(
            66,
            0,
            0.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
        pose_sample(
            67,
            5_000_000_000,
            5.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            covariance(0.0),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
        pose_sample(
            68,
            10_000_000_000,
            10.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            covariance(0.0),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
        pose_sample(
            69,
            15_000_000_000,
            15.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.9).unwrap()),
        ),
    ];
    let anchors = vec![
        anchor(70, &samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(71, &samples[3], 15.0, 0.0, 0.0, covariance(0.0)),
    ];
    let validated =
        ValidatedAnchors::new(PoseTimeline::new(&samples, policy).unwrap(), &anchors).unwrap();

    let Evidence::Known(result) = fuse_at(&validated, timestamp(7_500_000_000)).unwrap() else {
        panic!("supported position should remain available")
    };
    assert!((result.position.x.get() - 7.5).abs() < 1e-12);
    assert!(matches!(
        result.position_covariance,
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured)
    ));
}

#[test]
fn missing_uncertainty_remains_unknown_while_position_is_still_available() {
    let limits = limits(11.0, 0.0);
    let samples = vec![
        pose_sample(
            16,
            0,
            0.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured),
            TrackingState::Tracking,
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::SourceDidNotProvide),
        ),
        pose_sample(
            17,
            10_000_000_000,
            10.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured),
            TrackingState::Tracking,
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::SourceDidNotProvide),
        ),
    ];
    let anchors = vec![
        anchor(26, &samples[0], 100.0, 0.0, 0.0, covariance(0.0)),
        anchor(27, &samples[1], 112.0, 0.0, 0.0, covariance(0.0)),
    ];
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("known pose can retain unknown uncertainty")
    };
    assert!((result.position.x.get() - 106.0).abs() < 1e-12);
    assert!(matches!(
        result.position_covariance,
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured)
    ));
}

#[test]
fn missing_orientation_is_not_fabricated_from_neighboring_anchor_corrections() {
    let policy = limits(11.0, 0.0);
    let samples = vec![
        tracked_sample(37, 0, 0.0, 0.0),
        pose_sample(
            38,
            5_000_000_000,
            5.0,
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured),
            covariance(0.04),
            TrackingState::Tracking,
            Evidence::Known(kyberia_domain::units::Probability::new(0.8).unwrap()),
        ),
        tracked_sample(39, 10, 10.0, 0.0),
    ];
    let anchors = vec![
        anchor(37, &samples[0], 100.0, 0.0, 0.0, covariance(0.0)),
        anchor(39, &samples[2], 110.0, 0.0, 0.0, covariance(0.0)),
    ];
    let timeline = PoseTimeline::new(&samples, policy).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("position remains available from known evidence")
    };
    assert!((result.position.x.get() - 105.0).abs() < 1e-12);
    assert!(matches!(
        result.orientation,
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotMeasured)
    ));
}

#[test]
fn limited_tracking_and_unknown_quality_remain_visible_in_fused_evidence() {
    let policy = limits(11.0, 0.0);
    let samples = vec![
        tracked_sample(46, 0, 0.0, 0.0),
        pose_sample(
            47,
            10_000_000_000,
            10.0,
            Evidence::Known(Radians::new(0.0).unwrap()),
            covariance(0.04),
            TrackingState::Limited,
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotObservable),
        ),
    ];
    let anchors = vec![
        anchor(48, &samples[0], 100.0, 0.0, 0.0, covariance(0.0)),
        anchor(49, &samples[1], 110.0, 0.0, 0.0, covariance(0.0)),
    ];
    let validated =
        ValidatedAnchors::new(PoseTimeline::new(&samples, policy).unwrap(), &anchors).unwrap();
    let Evidence::Known(result) = fuse_at(&validated, timestamp(5_000_000_000)).unwrap() else {
        panic!("limited tracking retains explicit pose evidence")
    };
    assert_eq!(result.tracking_state, TrackingState::Limited);
    assert!(matches!(
        result.tracking_quality,
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotObservable)
    ));
}

#[test]
fn refuses_extrapolation_beyond_the_pose_or_anchor_support() {
    let limits = limits(11.0, 0.0);
    let samples = vec![
        tracked_sample(40, 0, 0.0, 0.0),
        tracked_sample(41, 5, 5.0, 0.0),
        tracked_sample(42, 10, 10.0, 0.0),
    ];
    let anchors = vec![
        anchor(43, &samples[1], 105.0, 0.0, 0.0, covariance(0.0)),
        anchor(44, &samples[2], 110.0, 0.0, 0.0, covariance(0.0)),
    ];
    let timeline = PoseTimeline::new(&samples, limits).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    assert!(matches!(
        // The query is on the pose path but outside map-anchor support.
        fuse_at(&validated, timestamp(0)).unwrap(),
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::OutsideEvidenceSupport)
    ));
    assert!(matches!(
        // This query is beyond the pose and anchor supports.
        fuse_at(&validated, timestamp(11_000_000_000)).unwrap(),
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::OutsideEvidenceSupport)
    ));
}

#[test]
fn refuses_anchor_gaps_larger_than_policy_even_with_dense_pose_samples() {
    let policy = limits(5.0, 0.0);
    let samples = vec![
        tracked_sample(32, 0, 0.0, 0.0),
        tracked_sample(33, 5, 5.0, 0.0),
        tracked_sample(34, 10, 10.0, 0.0),
    ];
    let anchors = vec![
        anchor(35, &samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(36, &samples[2], 10.0, 0.0, 0.0, covariance(0.0)),
    ];
    let timeline = PoseTimeline::new(&samples, policy).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    assert!(matches!(
        fuse_at(&validated, timestamp(5_000_000_000)).unwrap(),
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::OutsideEvidenceSupport)
    ));
}

#[test]
fn refuses_large_gaps_implausible_speed_and_not_tracking_samples() {
    let policy = limits(2.0, 0.0);
    let (samples, anchors) = two_point_track(policy);
    let timeline = PoseTimeline::new(&samples, policy).unwrap();
    let validated = ValidatedAnchors::new(timeline, &anchors).unwrap();
    assert!(matches!(
        fuse_at(&validated, timestamp(5_000_000_000)).unwrap(),
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::OutsideEvidenceSupport)
    ));

    let policy = limits(11.0, 0.0);
    let fast_samples = vec![
        tracked_sample(18, 0, 0.0, 0.0),
        tracked_sample(19, 10, 1_500.0, 0.0),
    ];
    let fast_anchors = vec![
        anchor(28, &fast_samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(29, &fast_samples[1], 1_500.0, 0.0, 0.0, covariance(0.0)),
    ];
    let timeline = PoseTimeline::new(&fast_samples, policy).unwrap();
    let validated = ValidatedAnchors::new(timeline, &fast_anchors).unwrap();
    assert!(matches!(
        fuse_at(&validated, timestamp(5_000_000_000)).unwrap(),
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::OutsideEvidenceSupport)
    ));

    let not_tracking = pose_sample(
        30,
        0,
        0.0,
        Evidence::Known(Radians::new(0.0).unwrap()),
        covariance(0.04),
        TrackingState::NotTracking,
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotObservable),
    );
    let not_tracking_samples = vec![not_tracking, tracked_sample(31, 10, 10.0, 0.0)];
    let not_tracking_anchors = vec![
        anchor(30, &not_tracking_samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(
            31,
            &not_tracking_samples[1],
            10.0,
            0.0,
            0.0,
            covariance(0.0),
        ),
    ];
    assert!(PoseTimeline::new(&not_tracking_samples, policy).is_ok());
    // An invalid tracking interval cannot serve as a map-control pair.
    assert!(
        ValidatedAnchors::new(
            PoseTimeline::new(&not_tracking_samples, policy).unwrap(),
            &not_tracking_anchors
        )
        .is_err()
    );

    let middle_not_tracking = pose_sample(
        33,
        5_000_000_000,
        5.0,
        Evidence::Known(Radians::new(0.0).unwrap()),
        covariance(0.04),
        TrackingState::NotTracking,
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotObservable),
    );
    let interrupted_samples = vec![
        tracked_sample(34, 0, 0.0, 0.0),
        middle_not_tracking,
        tracked_sample(35, 10, 10.0, 0.0),
    ];
    let interrupted_anchors = vec![
        anchor(36, &interrupted_samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(37, &interrupted_samples[2], 10.0, 0.0, 0.0, covariance(0.0)),
    ];
    let validated = ValidatedAnchors::new(
        PoseTimeline::new(&interrupted_samples, policy).unwrap(),
        &interrupted_anchors,
    )
    .unwrap();
    assert!(matches!(
        fuse_at(&validated, timestamp(5_000_000_000)).unwrap(),
        Evidence::Unknown(kyberia_domain::evidence::UnknownReason::NotObservable)
    ));
}

#[test]
fn exact_pose_samples_still_enforce_speed_limit_on_adjacent_segments() {
    let policy = limits(11.0, 0.0);
    let (samples, anchors) = two_point_track(policy);
    let valid =
        ValidatedAnchors::new(PoseTimeline::new(&samples, policy).unwrap(), &anchors).unwrap();
    assert!(matches!(
        fuse_at(&valid, timestamp(0)).unwrap(),
        Evidence::Known(_)
    ));

    let fast_samples = vec![
        tracked_sample(72, 0, 0.0, 0.0),
        tracked_sample(73, 5, 1_500.0, 0.0),
        tracked_sample(74, 10, 10.0, 0.0),
    ];
    let fast_anchors = vec![
        anchor(75, &fast_samples[0], 0.0, 0.0, 0.0, covariance(0.0)),
        anchor(76, &fast_samples[2], 10.0, 0.0, 0.0, covariance(0.0)),
    ];
    let fast = ValidatedAnchors::new(
        PoseTimeline::new(&fast_samples, policy).unwrap(),
        &fast_anchors,
    )
    .unwrap();

    for at in [0, 5_000_000_000, 10_000_000_000] {
        assert!(matches!(
            fuse_at(&fast, timestamp(at)).unwrap(),
            Evidence::Unknown(kyberia_domain::evidence::UnknownReason::OutsideEvidenceSupport)
        ));
    }
}
