use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{ClockEpochId, FrameId, ObservationId, PoseId, Text},
    spatial::{Point3, PoseReference, PositionCovariance},
    time::MonotonicTimestamp,
    units::{CoordinateMeters, Hertz, Meters, Radians},
};
use kyberia_survey::{
    ChannelCoverageCompleteness, ChannelCoverageInterval, ManualPathAnchorKind, ManualPathConfig,
    ManualPathError, ManualPathPhase, ManualPathSurvey, ManualTimeInterval, MetersPerSecond,
    PathDiagnostic, PoseDecision, PositionAssignment, PositionUnavailableReason,
    ScheduledChannelInterval,
};

fn epoch() -> ClockEpochId {
    ClockEpochId::from_bytes([1; 16]).unwrap()
}

fn frame() -> FrameId {
    FrameId::from_bytes([2; 16]).unwrap()
}

fn at(seconds: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds: seconds * 1_000_000_000,
    }
}

fn p(x: f64, y: f64) -> Point3 {
    Point3 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
        z: CoordinateMeters::new(0.0).unwrap(),
    }
}

fn config(maximum_speed: f64, sharp_turn: f64, maximum_pose_stddev: f64) -> ManualPathConfig {
    ManualPathConfig::new(
        frame(),
        epoch(),
        MetersPerSecond::new(maximum_speed).unwrap(),
        Radians::new(sharp_turn).unwrap(),
        Meters::new(maximum_pose_stddev).unwrap(),
    )
    .unwrap()
}

fn pose(x: f64, variance: Option<f64>) -> PoseReference {
    PoseReference {
        pose_id: PoseId::from_bytes([9; 16]).unwrap(),
        frame_id: frame(),
        assignment_version: Text::new("pose-v1").unwrap(),
        position: p(x, 0.0),
        covariance: match variance {
            Some(value) => Evidence::Known(
                PositionCovariance::new([value, 0.0, 0.0, value, 0.0, value]).unwrap(),
            ),
            None => Evidence::Unknown(UnknownReason::NotMeasured),
        },
        orientation: Evidence::Unknown(UnknownReason::NotMeasured),
        method_version: Text::new("test-pose-v1").unwrap(),
    }
}

fn id(value: u8) -> ObservationId {
    ObservationId::from_bytes([value; 16]).unwrap()
}

fn path() -> ManualPathSurvey {
    ManualPathSurvey::start(config(2.0, 1.0, 0.25), at(0), p(0.0, 0.0)).unwrap()
}

#[test]
fn start_turn_pause_resume_stop_and_exact_timestamp_interpolation() {
    let path = path()
        .record_observation(
            id(1),
            at(5),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        )
        .unwrap();
    assert!(matches!(
        path.positioned_observations().unwrap()[0].assignment,
        PositionAssignment::Unavailable {
            reason: PositionUnavailableReason::AwaitingNextAnchor
        }
    ));

    let path = path
        .turn(at(10), p(10.0, 0.0))
        .unwrap()
        .pause(at(20), p(20.0, 0.0))
        .unwrap()
        .record_observation(
            id(2),
            at(15),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        )
        .unwrap()
        .record_observation(
            id(3),
            at(25),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        )
        .unwrap()
        .resume(at(40), p(30.0, 0.0))
        .unwrap()
        .turn(at(50), p(40.0, 0.0))
        .unwrap()
        .stop(at(60), p(50.0, 0.0))
        .unwrap()
        .record_observation(
            id(4),
            at(45),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        )
        .unwrap();

    assert_eq!(path.phase(), ManualPathPhase::Stopped);
    assert_eq!(
        path.anchors()
            .iter()
            .map(|anchor| anchor.kind())
            .collect::<Vec<_>>(),
        vec![
            ManualPathAnchorKind::Start,
            ManualPathAnchorKind::Turn,
            ManualPathAnchorKind::Pause,
            ManualPathAnchorKind::Resume,
            ManualPathAnchorKind::Turn,
            ManualPathAnchorKind::Stop,
        ]
    );
    assert_eq!(
        path.anchors().iter().map(|a| a.leg()).collect::<Vec<_>>(),
        [0, 0, 0, 1, 1, 1]
    );
    let positions = path.positioned_observations().unwrap();
    assert!(matches!(
        &positions[0].assignment,
        PositionAssignment::ManualUniformMotion { .. }
    ));
    match &positions[0].assignment {
        PositionAssignment::ManualUniformMotion {
            position,
            interpolation_fraction,
            ..
        } => {
            assert_eq!(*interpolation_fraction, 0.5);
            assert_eq!(position.x.get(), 5.0);
        }
        _ => unreachable!(),
    }
    match &positions[1].assignment {
        PositionAssignment::ManualUniformMotion { position, .. } => {
            assert_eq!(position.x.get(), 15.0);
        }
        _ => unreachable!(),
    }
    assert!(matches!(
        &positions[2].assignment,
        PositionAssignment::Unavailable {
            reason: PositionUnavailableReason::PauseGap
        }
    ));
    match &positions[3].assignment {
        PositionAssignment::ManualUniformMotion { position, .. } => {
            assert_eq!(position.x.get(), 35.0);
        }
        _ => unreachable!(),
    }
    assert_eq!(
        path.record_observation(
            id(5),
            at(61),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide)
        ),
        Err(ManualPathError::OutsideSurveyTime)
    );
}

#[test]
fn duplicate_reversed_epochs_and_frames_fail_closed() {
    assert_eq!(
        ManualPathSurvey::start(
            ManualPathConfig::new(
                frame(),
                ClockEpochId::from_bytes([3; 16]).unwrap(),
                MetersPerSecond::new(1.0).unwrap(),
                Radians::new(1.0).unwrap(),
                Meters::new(0.1).unwrap(),
            )
            .unwrap(),
            at(0),
            p(0.0, 0.0)
        ),
        Err(ManualPathError::WrongClock)
    );
    assert_eq!(
        path().turn(at(0), p(1.0, 0.0)),
        Err(ManualPathError::DuplicateAnchorTimestamp)
    );
    assert_eq!(
        path()
            .turn(at(1), p(1.0, 0.0))
            .unwrap()
            .turn(at(0), p(2.0, 0.0)),
        Err(ManualPathError::ReversedTime)
    );

    let wrong_epoch = MonotonicTimestamp {
        epoch: ClockEpochId::from_bytes([8; 16]).unwrap(),
        nanoseconds: 1,
    };
    assert_eq!(
        path().record_observation(
            id(1),
            wrong_epoch,
            Evidence::Unknown(UnknownReason::NotMeasured)
        ),
        Err(ManualPathError::WrongClock)
    );
    let wrong_frame = PoseReference {
        frame_id: FrameId::from_bytes([7; 16]).unwrap(),
        ..pose(2.0, Some(0.01))
    };
    assert_eq!(
        path().record_observation(id(1), at(1), Evidence::Known(wrong_frame)),
        Err(ManualPathError::WrongFrame)
    );
}

#[test]
fn interior_timestamp_at_u64_scale_does_not_collapse_to_endpoint() {
    let end = MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds: u64::MAX,
    };
    let interior = MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds: u64::MAX - 1,
    };
    let path = path()
        .stop(end, p(1.0, 0.0))
        .unwrap()
        .record_observation(
            id(1),
            interior,
            Evidence::Unknown(UnknownReason::NotMeasured),
        )
        .unwrap();
    match &path.positioned_observations().unwrap()[0].assignment {
        PositionAssignment::ManualUniformMotion {
            position,
            interpolation_fraction,
            ..
        } => {
            assert!(*interpolation_fraction < 1.0);
            assert!(position.x.get() < 1.0);
        }
        _ => panic!("interior monotonic timestamp must remain inside its segment"),
    }
}

#[test]
fn supplied_trustworthy_pose_takes_precedence_and_uncertain_pose_falls_back() {
    let path = path()
        .turn(at(10), p(10.0, 0.0))
        .unwrap()
        .stop(at(20), p(20.0, 0.0))
        .unwrap()
        .record_observation(id(1), at(5), Evidence::Known(pose(99.0, Some(0.01))))
        .unwrap()
        .record_observation(id(2), at(5), Evidence::Known(pose(99.0, Some(0.25))))
        .unwrap()
        .record_observation(id(3), at(5), Evidence::Known(pose(99.0, None)))
        .unwrap();
    let samples = path.positioned_observations().unwrap();
    assert_eq!(samples[0].pose_decision, PoseDecision::UsedReportedPose);
    match &samples[0].assignment {
        PositionAssignment::ReportedPose { position, .. } => assert_eq!(position.x.get(), 99.0),
        _ => panic!("trustworthy source pose must win"),
    }
    assert_eq!(
        samples[1].pose_decision,
        PoseDecision::UncertaintyAboveThreshold
    );
    assert_eq!(samples[2].pose_decision, PoseDecision::CovarianceUnknown);
    for sample in &samples[1..] {
        match &sample.assignment {
            PositionAssignment::ManualUniformMotion { position, .. } => {
                assert_eq!(position.x.get(), 5.0);
            }
            _ => panic!("untrusted source pose should use supported manual projection"),
        }
    }
}

#[test]
fn implausible_speed_and_sharp_turns_are_typed_diagnostics_not_rejections() {
    let path = ManualPathSurvey::start(
        config(1.0, std::f64::consts::FRAC_PI_4, 0.25),
        at(0),
        p(0.0, 0.0),
    )
    .unwrap()
    .turn(at(1), p(1.0, 0.0))
    .unwrap();
    assert!(path.diagnostics().unwrap().is_empty());

    let path = path.turn(at(2), p(1.0, 1.0)).unwrap();
    let diagnostics = path.diagnostics().unwrap();
    assert!(diagnostics.iter().any(|item| matches!(
        item,
        PathDiagnostic::SharpTurn { heading_change, .. }
            if (heading_change.get() - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    )));
    assert!(
        !diagnostics
            .iter()
            .any(|item| matches!(item, PathDiagnostic::ImplausibleSpeed { .. }))
    );
    let path = path.turn(at(2 + 1), p(1.0, 4.0)).unwrap();
    assert!(
        path.diagnostics()
            .unwrap()
            .iter()
            .any(|item| matches!(item, PathDiagnostic::ImplausibleSpeed { .. }))
    );

    let exact_turn_limit = ManualPathSurvey::start(
        config(1.0, std::f64::consts::FRAC_PI_2, 0.25),
        at(0),
        p(0.0, 0.0),
    )
    .unwrap()
    .turn(at(1), p(1.0, 0.0))
    .unwrap()
    .turn(at(2), p(1.0, 1.0))
    .unwrap();
    assert!(exact_turn_limit.diagnostics().unwrap().is_empty());
}

#[test]
fn anchor_edits_reproject_without_changing_original_anchors_or_raw_times() {
    let capturing = path().turn(at(10), p(10.0, 0.0)).unwrap();
    assert_eq!(
        capturing.edit_anchor(capturing.anchors()[0].id(), p(-1.0, 0.0)),
        Err(ManualPathError::NotStopped)
    );
    let path = capturing
        .stop(at(20), p(20.0, 0.0))
        .unwrap()
        .record_observation(id(4), at(5), Evidence::Unknown(UnknownReason::NotMeasured))
        .unwrap();
    let before = path.positioned_observations().unwrap()[0].clone();
    let edited_anchor = path.anchors()[1].id();
    let edited = path.edit_anchor(edited_anchor, p(20.0, 0.0)).unwrap();
    let after = edited.positioned_observations().unwrap()[0].clone();
    assert_eq!(before.observation_id, after.observation_id);
    assert_eq!(before.captured_at, after.captured_at);
    match (before.assignment, after.assignment) {
        (
            PositionAssignment::ManualUniformMotion { position: old, .. },
            PositionAssignment::ManualUniformMotion { position: new, .. },
        ) => {
            assert_eq!(old.x.get(), 5.0);
            assert_eq!(new.x.get(), 10.0);
        }
        _ => panic!("both projections should remain on the same supported route"),
    }
    assert_eq!(edited.anchors()[1].original_position().x.get(), 10.0);
    assert_eq!(edited.anchors()[1].position().x.get(), 20.0);
    assert_eq!(path.anchors()[1].position().x.get(), 10.0);
    assert_eq!(edited.anchors().last().unwrap().position().x.get(), 20.0);
}

#[test]
fn channel_gaps_are_limited_to_scheduled_path_time_and_never_mean_ap_absence() {
    let path = path()
        .turn(at(10), p(10.0, 0.0))
        .unwrap()
        .stop(at(20), p(20.0, 0.0))
        .unwrap();
    let schedule = [ScheduledChannelInterval {
        evidence_ref: Text::new("schedule-a").unwrap(),
        frequency: Hertz::new(2_412_000_000.0).unwrap(),
        interval: ManualTimeInterval::new(at(0), at(10)).unwrap(),
    }];
    let coverage = [ChannelCoverageInterval {
        evidence_ref: Text::new("dwell-a").unwrap(),
        frequency: Hertz::new(2_412_000_000.0).unwrap(),
        interval: ManualTimeInterval::new(at(4), at(6)).unwrap(),
        completeness: ChannelCoverageCompleteness::Complete,
    }];
    let gaps = path.channel_gaps(&schedule, &coverage).unwrap();
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0].interval.start(), at(0));
    assert_eq!(gaps[0].interval.end_exclusive(), at(4));
    assert_eq!(gaps[0].start_position.x.get(), 0.0);
    assert_eq!(gaps[0].end_position.x.get(), 4.0);
    assert_eq!(gaps[1].interval.start(), at(6));
    assert_eq!(gaps[1].interval.end_exclusive(), at(10));
    assert_eq!(
        gaps[0].interpretation,
        kyberia_survey::ChannelGapInterpretation::ScheduledIntervalWithoutCompleteCoverage
    );
    assert!(path.channel_gaps(&[], &[]).unwrap().is_empty());

    let partial = [ChannelCoverageInterval {
        completeness: ChannelCoverageCompleteness::Incomplete,
        ..coverage[0].clone()
    }];
    assert_eq!(path.channel_gaps(&schedule, &partial).unwrap().len(), 1);

    let foreign_epoch = ClockEpochId::from_bytes([6; 16]).unwrap();
    let foreign_schedule = [ScheduledChannelInterval {
        interval: ManualTimeInterval::new(
            MonotonicTimestamp {
                epoch: foreign_epoch,
                nanoseconds: 0,
            },
            MonotonicTimestamp {
                epoch: foreign_epoch,
                nanoseconds: 10,
            },
        )
        .unwrap(),
        ..schedule[0].clone()
    }];
    assert_eq!(
        path.channel_gaps(&foreign_schedule, &[]),
        Err(ManualPathError::WrongClock)
    );

    let oversized_schedule = vec![schedule[0].clone(); 513];
    assert_eq!(
        path.channel_gaps(&oversized_schedule, &[]),
        Err(ManualPathError::Limit)
    );
}

#[test]
fn channel_gaps_do_not_bridge_pause_intervals() {
    let path = path()
        .pause(at(10), p(10.0, 0.0))
        .unwrap()
        .resume(at(20), p(20.0, 0.0))
        .unwrap()
        .stop(at(30), p(30.0, 0.0))
        .unwrap();
    let schedule = [ScheduledChannelInterval {
        evidence_ref: Text::new("schedule").unwrap(),
        frequency: Hertz::new(5_180_000_000.0).unwrap(),
        interval: ManualTimeInterval::new(at(0), at(30)).unwrap(),
    }];
    let gaps = path.channel_gaps(&schedule, &[]).unwrap();
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0].interval.start(), at(0));
    assert_eq!(gaps[0].interval.end_exclusive(), at(10));
    assert_eq!(gaps[1].interval.start(), at(20));
    assert_eq!(gaps[1].interval.end_exclusive(), at(30));
}

#[test]
fn duplicate_observation_and_malformed_serialized_timestamps_are_rejected() {
    let path = path()
        .turn(at(10), p(10.0, 0.0))
        .unwrap()
        .stop(at(20), p(20.0, 0.0))
        .unwrap()
        .record_observation(id(1), at(5), Evidence::Unknown(UnknownReason::NotMeasured))
        .unwrap();
    assert_eq!(
        path.record_observation(id(1), at(6), Evidence::Unknown(UnknownReason::NotMeasured)),
        Err(ManualPathError::DuplicateObservation)
    );
    let bytes = serde_json::to_vec(&path).unwrap();
    assert_eq!(
        serde_json::from_slice::<ManualPathSurvey>(&bytes).unwrap(),
        path
    );

    let mut malformed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    malformed["anchors"][1]["at"]["nanoseconds"] = serde_json::json!(0);
    let malformed = serde_json::to_vec(&malformed).unwrap();
    assert!(serde_json::from_slice::<ManualPathSurvey>(&malformed).is_err());

    let mut oversized: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let anchors = oversized["anchors"].as_array_mut().unwrap();
    let start_anchor = anchors[0].clone();
    while anchors.len() <= 512 {
        anchors.push(start_anchor.clone());
    }
    assert!(serde_json::from_value::<ManualPathSurvey>(oversized).is_err());
}

#[test]
fn stop_cannot_place_the_endpoint_before_a_retained_sample() {
    let path = path()
        .record_observation(id(1), at(5), Evidence::Unknown(UnknownReason::NotMeasured))
        .unwrap();
    assert_eq!(
        path.stop(at(4), p(4.0, 0.0)),
        Err(ManualPathError::OutsideSurveyTime)
    );
}
