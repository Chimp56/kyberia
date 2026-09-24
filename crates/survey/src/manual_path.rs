//! Bounded, deterministic model for operator-anchored continuous-path surveys.
//!
//! This is a pure state model. It does not acquire samples, read a clock, draw
//! a route, or persist observations. Callers pass source-local monotonic time
//! and retain the canonical observation identified by each stored ID.
use crate::*;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

const MAX_PATH_ANCHORS: usize = 512;
const MAX_PATH_OBSERVATIONS: usize = 8192;
const MAX_CHANNEL_SCHEDULE_INTERVALS: usize = 512;
const MAX_CHANNEL_COVERAGE_INTERVALS: usize = 2048;
const MAX_CHANNEL_GAPS: usize = 16_384;
const MAX_CHANNEL_GAP_WORK: usize = 3_000_000;

#[derive(Default)]
struct ChannelGapWork {
    charged: usize,
}

impl ChannelGapWork {
    fn charge(&mut self, units: usize) -> Result<(), ManualPathError> {
        let next = self
            .charged
            .checked_add(units)
            .ok_or(ManualPathError::Limit)?;
        if next > MAX_CHANNEL_GAP_WORK {
            return Err(ManualPathError::Limit);
        }
        self.charged = next;
        Ok(())
    }
}

fn sort_channel_intervals(
    intervals: &mut [ManualTimeInterval],
    work: &mut ChannelGapWork,
) -> Result<(), ManualPathError> {
    for index in 1..intervals.len() {
        let interval = intervals[index];
        let mut insertion = index;
        while insertion > 0 {
            work.charge(1)?;
            let previous = intervals[insertion - 1];
            let ordering = previous
                .start
                .nanoseconds
                .cmp(&interval.start.nanoseconds)
                .then_with(|| {
                    previous
                        .end_exclusive
                        .nanoseconds
                        .cmp(&interval.end_exclusive.nanoseconds)
                });
            if ordering != std::cmp::Ordering::Greater {
                break;
            }
            work.charge(1)?;
            intervals[insertion] = previous;
            insertion -= 1;
        }
        if insertion != index {
            work.charge(1)?;
            intervals[insertion] = interval;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManualPathError {
    InvalidConfiguration,
    InvalidTransition,
    WrongClock,
    WrongFrame,
    ReversedTime,
    DuplicateAnchorTimestamp,
    DuplicateObservation,
    OutsideSurveyTime,
    UnknownAnchor,
    NotStopped,
    InvalidSnapshot,
    Limit,
    ArithmeticOverflow,
}

impl std::fmt::Display for ManualPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ManualPathError {}

/// Finite nonnegative speed in meters per second.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct MetersPerSecond(f64);

impl MetersPerSecond {
    pub fn new(value: f64) -> Result<Self, ManualPathError> {
        if !value.is_finite() || value < 0.0 {
            return Err(ManualPathError::InvalidConfiguration);
        }
        Ok(Self(if value == 0.0 { 0.0 } else { value }))
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for MetersPerSecond {
    type Error = ManualPathError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<MetersPerSecond> for f64 {
    fn from(value: MetersPerSecond) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ManualPathConfig {
    frame_id: FrameId,
    epoch: ClockEpochId,
    maximum_speed: MetersPerSecond,
    sharp_turn_threshold: Radians,
    maximum_reported_pose_axis_stddev: Meters,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualPathConfigWire {
    frame_id: FrameId,
    epoch: ClockEpochId,
    maximum_speed: MetersPerSecond,
    sharp_turn_threshold: Radians,
    maximum_reported_pose_axis_stddev: Meters,
}

impl ManualPathConfig {
    pub fn new(
        frame_id: FrameId,
        epoch: ClockEpochId,
        maximum_speed: MetersPerSecond,
        sharp_turn_threshold: Radians,
        maximum_reported_pose_axis_stddev: Meters,
    ) -> Result<Self, ManualPathError> {
        if maximum_speed.get() <= 0.0
            || sharp_turn_threshold.get() <= 0.0
            || sharp_turn_threshold.get() > std::f64::consts::PI
            || !sharp_turn_threshold.get().is_finite()
            || !maximum_reported_pose_axis_stddev.get().is_finite()
        {
            return Err(ManualPathError::InvalidConfiguration);
        }
        Ok(Self {
            frame_id,
            epoch,
            maximum_speed,
            sharp_turn_threshold,
            maximum_reported_pose_axis_stddev,
        })
    }

    pub const fn frame_id(&self) -> FrameId {
        self.frame_id
    }
    pub const fn epoch(&self) -> ClockEpochId {
        self.epoch
    }
}

impl<'de> Deserialize<'de> for ManualPathConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ManualPathConfigWire::deserialize(deserializer)?;
        Self::new(
            wire.frame_id,
            wire.epoch,
            wire.maximum_speed,
            wire.sharp_turn_threshold,
            wire.maximum_reported_pose_axis_stddev,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManualPathSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualPathPhase {
    Capturing,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualPathAnchorKind {
    Start,
    Turn,
    Pause,
    Resume,
    Stop,
}

/// Stable within one path; corrections never change the identity or original
/// operator click.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PathAnchorId(u32);

impl PathAnchorId {
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualPathAnchor {
    id: PathAnchorId,
    at: MonotonicTimestamp,
    leg: u32,
    kind: ManualPathAnchorKind,
    original_position: Point3,
    edited_position: Option<Point3>,
}

impl ManualPathAnchor {
    pub const fn id(&self) -> PathAnchorId {
        self.id
    }
    pub const fn at(&self) -> MonotonicTimestamp {
        self.at
    }
    pub const fn leg(&self) -> u32 {
        self.leg
    }
    pub const fn kind(&self) -> ManualPathAnchorKind {
        self.kind
    }
    pub const fn original_position(&self) -> Point3 {
        self.original_position
    }
    pub const fn position(&self) -> Point3 {
        match self.edited_position {
            Some(position) => position,
            None => self.original_position,
        }
    }
    pub const fn edited_position(&self) -> Option<Point3> {
        self.edited_position
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualPathObservation {
    observation_id: ObservationId,
    captured_at: MonotonicTimestamp,
    reported_pose: Evidence<PoseReference>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManualPathSurvey {
    schema_version: ManualPathSchemaVersion,
    config: ManualPathConfig,
    phase: ManualPathPhase,
    anchors: Vec<ManualPathAnchor>,
    observations: Vec<ManualPathObservation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualPathSurveyWire {
    schema_version: ManualPathSchemaVersion,
    config: ManualPathConfig,
    phase: ManualPathPhase,
    #[serde(deserialize_with = "deserialize_anchors")]
    anchors: Vec<ManualPathAnchor>,
    #[serde(deserialize_with = "deserialize_observations")]
    observations: Vec<ManualPathObservation>,
}

fn deserialize_anchors<'de, D>(deserializer: D) -> Result<Vec<ManualPathAnchor>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec(deserializer, MAX_PATH_ANCHORS)
}

fn deserialize_observations<'de, D>(deserializer: D) -> Result<Vec<ManualPathObservation>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec(deserializer, MAX_PATH_OBSERVATIONS)
}

fn deserialize_bounded_vec<'de, D, T>(deserializer: D, limit: usize) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedSequence<T> {
        limit: usize,
        marker: std::marker::PhantomData<T>,
    }
    impl<'de, T> serde::de::Visitor<'de> for BoundedSequence<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "at most {} bounded survey records", self.limit)
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(self.limit));
            while let Some(value) = sequence.next_element()? {
                if values.len() == self.limit {
                    return Err(serde::de::Error::custom("survey record limit exceeded"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(BoundedSequence {
        limit,
        marker: std::marker::PhantomData,
    })
}

impl<'de> Deserialize<'de> for ManualPathSurvey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ManualPathSurveyWire::deserialize(deserializer)?;
        let survey = Self {
            schema_version: wire.schema_version,
            config: wire.config,
            phase: wire.phase,
            anchors: wire.anchors,
            observations: wire.observations,
        };
        survey
            .validate_snapshot()
            .map_err(serde::de::Error::custom)?;
        Ok(survey)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathSegmentId {
    pub from: PathAnchorId,
    pub to: PathAnchorId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathDiagnostic {
    ImplausibleSpeed {
        segment: PathSegmentId,
        measured: MetersPerSecond,
        configured_limit: MetersPerSecond,
    },
    SharpTurn {
        anchor: PathAnchorId,
        heading_change: Radians,
        configured_limit: Radians,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoseDecision {
    UsedReportedPose,
    NoKnownPose,
    CovarianceUnknown,
    UncertaintyAboveThreshold,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PositionAssignment {
    ReportedPose {
        pose_id: PoseId,
        position: Point3,
    },
    ManualAnchor {
        anchor: PathAnchorId,
        position: Point3,
    },
    ManualUniformMotion {
        segment: PathSegmentId,
        interpolation_fraction: f64,
        position: Point3,
    },
    Unavailable {
        reason: PositionUnavailableReason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionUnavailableReason {
    PauseGap,
    AwaitingNextAnchor,
    OutsideAnchoredPath,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PositionedObservation {
    pub observation_id: ObservationId,
    pub captured_at: MonotonicTimestamp,
    pub pose_decision: PoseDecision,
    pub assignment: PositionAssignment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ManualTimeIntervalWire", into = "ManualTimeIntervalWire")]
pub struct ManualTimeInterval {
    start: MonotonicTimestamp,
    end_exclusive: MonotonicTimestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ManualTimeIntervalWire {
    start: MonotonicTimestamp,
    end_exclusive: MonotonicTimestamp,
}

impl ManualTimeInterval {
    pub fn new(
        start: MonotonicTimestamp,
        end_exclusive: MonotonicTimestamp,
    ) -> Result<Self, ManualPathError> {
        if start.epoch != end_exclusive.epoch {
            return Err(ManualPathError::WrongClock);
        }
        if start.nanoseconds >= end_exclusive.nanoseconds {
            return Err(ManualPathError::ReversedTime);
        }
        Ok(Self {
            start,
            end_exclusive,
        })
    }
    pub const fn start(self) -> MonotonicTimestamp {
        self.start
    }
    pub const fn end_exclusive(self) -> MonotonicTimestamp {
        self.end_exclusive
    }
}

impl TryFrom<ManualTimeIntervalWire> for ManualTimeInterval {
    type Error = ManualPathError;
    fn try_from(value: ManualTimeIntervalWire) -> Result<Self, Self::Error> {
        Self::new(value.start, value.end_exclusive)
    }
}
impl From<ManualTimeInterval> for ManualTimeIntervalWire {
    fn from(value: ManualTimeInterval) -> Self {
        Self {
            start: value.start,
            end_exclusive: value.end_exclusive,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledChannelInterval {
    pub evidence_ref: Text,
    pub frequency: Hertz,
    pub interval: ManualTimeInterval,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelCoverageCompleteness {
    Complete,
    Incomplete,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelCoverageInterval {
    pub evidence_ref: Text,
    pub frequency: Hertz,
    pub interval: ManualTimeInterval,
    pub completeness: ChannelCoverageCompleteness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelGapInterpretation {
    ScheduledIntervalWithoutCompleteCoverage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelGap {
    pub segment: PathSegmentId,
    pub schedule_evidence_ref: Text,
    pub frequency: Hertz,
    pub interval: ManualTimeInterval,
    pub start_position: Point3,
    pub end_position: Point3,
    pub interpretation: ChannelGapInterpretation,
}

impl ManualPathSurvey {
    pub fn start(
        config: ManualPathConfig,
        at: MonotonicTimestamp,
        position: Point3,
    ) -> Result<Self, ManualPathError> {
        if at.epoch != config.epoch {
            return Err(ManualPathError::WrongClock);
        }
        check_position(position)?;
        Ok(Self {
            schema_version: ManualPathSchemaVersion::V1,
            config,
            phase: ManualPathPhase::Capturing,
            anchors: vec![ManualPathAnchor {
                id: PathAnchorId(1),
                at,
                leg: 0,
                kind: ManualPathAnchorKind::Start,
                original_position: position,
                edited_position: None,
            }],
            observations: Vec::new(),
        })
    }

    pub const fn config(&self) -> &ManualPathConfig {
        &self.config
    }
    pub const fn phase(&self) -> ManualPathPhase {
        self.phase
    }
    pub fn anchors(&self) -> &[ManualPathAnchor] {
        &self.anchors
    }
    pub fn observation_count(&self) -> usize {
        self.observations.len()
    }

    pub fn turn(&self, at: MonotonicTimestamp, position: Point3) -> Result<Self, ManualPathError> {
        self.append_active_anchor(at, position, ManualPathAnchorKind::Turn)
    }

    pub fn pause(&self, at: MonotonicTimestamp, position: Point3) -> Result<Self, ManualPathError> {
        let mut next = self.append_active_anchor(at, position, ManualPathAnchorKind::Pause)?;
        next.phase = ManualPathPhase::Paused;
        Ok(next)
    }

    pub fn resume(
        &self,
        at: MonotonicTimestamp,
        position: Point3,
    ) -> Result<Self, ManualPathError> {
        if self.phase != ManualPathPhase::Paused {
            return Err(ManualPathError::InvalidTransition);
        }
        let leg = self
            .anchors
            .last()
            .ok_or(ManualPathError::InvalidSnapshot)?
            .leg
            .checked_add(1)
            .ok_or(ManualPathError::Limit)?;
        let mut next = self.append_anchor(at, position, leg, ManualPathAnchorKind::Resume)?;
        next.phase = ManualPathPhase::Capturing;
        Ok(next)
    }

    /// Stops from either capturing or paused state. Stopping while paused adds
    /// an isolated endpoint on a new leg; it never creates a path segment over
    /// the pause interval.
    pub fn stop(&self, at: MonotonicTimestamp, position: Point3) -> Result<Self, ManualPathError> {
        let last = self
            .anchors
            .last()
            .ok_or(ManualPathError::InvalidSnapshot)?;
        if self
            .observations
            .iter()
            .any(|sample| sample.captured_at.nanoseconds > at.nanoseconds)
        {
            return Err(ManualPathError::OutsideSurveyTime);
        }
        let leg = match self.phase {
            ManualPathPhase::Capturing => last.leg,
            ManualPathPhase::Paused => last.leg.checked_add(1).ok_or(ManualPathError::Limit)?,
            ManualPathPhase::Stopped => return Err(ManualPathError::InvalidTransition),
        };
        let mut next = self.append_anchor(at, position, leg, ManualPathAnchorKind::Stop)?;
        next.phase = ManualPathPhase::Stopped;
        Ok(next)
    }

    fn append_active_anchor(
        &self,
        at: MonotonicTimestamp,
        position: Point3,
        kind: ManualPathAnchorKind,
    ) -> Result<Self, ManualPathError> {
        if self.phase != ManualPathPhase::Capturing {
            return Err(ManualPathError::InvalidTransition);
        }
        let leg = self
            .anchors
            .last()
            .ok_or(ManualPathError::InvalidSnapshot)?
            .leg;
        self.append_anchor(at, position, leg, kind)
    }

    fn append_anchor(
        &self,
        at: MonotonicTimestamp,
        position: Point3,
        leg: u32,
        kind: ManualPathAnchorKind,
    ) -> Result<Self, ManualPathError> {
        if self.phase == ManualPathPhase::Stopped {
            return Err(ManualPathError::InvalidTransition);
        }
        if at.epoch != self.config.epoch {
            return Err(ManualPathError::WrongClock);
        }
        if self.anchors.len() >= MAX_PATH_ANCHORS {
            return Err(ManualPathError::Limit);
        }
        let last = self
            .anchors
            .last()
            .ok_or(ManualPathError::InvalidSnapshot)?;
        if at.nanoseconds < last.at.nanoseconds {
            return Err(ManualPathError::ReversedTime);
        }
        if at.nanoseconds == last.at.nanoseconds {
            return Err(ManualPathError::DuplicateAnchorTimestamp);
        }
        check_position(position)?;
        let mut next = self.clone();
        let id = u32::try_from(next.anchors.len() + 1).map_err(|_| ManualPathError::Limit)?;
        next.anchors.push(ManualPathAnchor {
            id: PathAnchorId(id),
            at,
            leg,
            kind,
            original_position: position,
            edited_position: None,
        });
        next.validate_derived()?;
        Ok(next)
    }

    /// Retains the raw observation ID, its source-local timestamp, and any
    /// reported pose as supplied. Equal observation timestamps are allowed;
    /// only repeated IDs are ambiguous.
    pub fn record_observation(
        &self,
        observation_id: ObservationId,
        captured_at: MonotonicTimestamp,
        reported_pose: Evidence<PoseReference>,
    ) -> Result<Self, ManualPathError> {
        if captured_at.epoch != self.config.epoch {
            return Err(ManualPathError::WrongClock);
        }
        if captured_at.nanoseconds < self.anchors[0].at.nanoseconds {
            return Err(ManualPathError::OutsideSurveyTime);
        }
        if self.phase == ManualPathPhase::Stopped
            && captured_at.nanoseconds > self.anchors.last().unwrap().at.nanoseconds
        {
            return Err(ManualPathError::OutsideSurveyTime);
        }
        if self.observations.len() >= MAX_PATH_OBSERVATIONS {
            return Err(ManualPathError::Limit);
        }
        if self
            .observations
            .iter()
            .any(|sample| sample.observation_id == observation_id)
        {
            return Err(ManualPathError::DuplicateObservation);
        }
        if let Evidence::Known(pose) = &reported_pose
            && pose.frame_id != self.config.frame_id
        {
            return Err(ManualPathError::WrongFrame);
        }
        let mut next = self.clone();
        next.observations.push(ManualPathObservation {
            observation_id,
            captured_at,
            reported_pose,
        });
        next.observations
            .sort_by_key(|sample| (sample.captured_at.nanoseconds, sample.observation_id));
        Ok(next)
    }

    /// Edits only the current spatial anchor. The original click, anchor ID,
    /// all event times, and all raw observation IDs/times remain unchanged.
    pub fn edit_anchor(
        &self,
        id: PathAnchorId,
        corrected_position: Point3,
    ) -> Result<Self, ManualPathError> {
        if self.phase != ManualPathPhase::Stopped {
            return Err(ManualPathError::NotStopped);
        }
        check_position(corrected_position)?;
        let mut next = self.clone();
        let anchor = next
            .anchors
            .iter_mut()
            .find(|anchor| anchor.id == id)
            .ok_or(ManualPathError::UnknownAnchor)?;
        anchor.edited_position = Some(corrected_position);
        next.validate_derived()?;
        Ok(next)
    }

    pub fn positioned_observations(&self) -> Result<Vec<PositionedObservation>, ManualPathError> {
        self.observations
            .iter()
            .map(|sample| self.position_observation(sample))
            .collect()
    }

    fn position_observation(
        &self,
        sample: &ManualPathObservation,
    ) -> Result<PositionedObservation, ManualPathError> {
        let (trusted_pose, pose_decision) = self.trusted_pose(&sample.reported_pose)?;
        let assignment = if let Some(pose) = trusted_pose {
            PositionAssignment::ReportedPose {
                pose_id: pose.pose_id,
                position: pose.position,
            }
        } else {
            self.manual_assignment(sample.captured_at)?
        };
        Ok(PositionedObservation {
            observation_id: sample.observation_id,
            captured_at: sample.captured_at,
            pose_decision,
            assignment,
        })
    }

    fn trusted_pose<'a>(
        &self,
        evidence: &'a Evidence<PoseReference>,
    ) -> Result<(Option<&'a PoseReference>, PoseDecision), ManualPathError> {
        let Evidence::Known(pose) = evidence else {
            return Ok((None, PoseDecision::NoKnownPose));
        };
        if pose.frame_id != self.config.frame_id {
            return Err(ManualPathError::WrongFrame);
        }
        let Evidence::Known(covariance) = &pose.covariance else {
            return Ok((None, PoseDecision::CovarianceUnknown));
        };
        let diagonal = covariance.packed();
        let maximum_axis_stddev = diagonal[0].max(diagonal[3]).max(diagonal[5]).sqrt();
        if !maximum_axis_stddev.is_finite() {
            return Err(ManualPathError::ArithmeticOverflow);
        }
        if maximum_axis_stddev > self.config.maximum_reported_pose_axis_stddev.get() {
            return Ok((None, PoseDecision::UncertaintyAboveThreshold));
        }
        check_position(pose.position)?;
        Ok((Some(pose), PoseDecision::UsedReportedPose))
    }

    fn manual_assignment(
        &self,
        at: MonotonicTimestamp,
    ) -> Result<PositionAssignment, ManualPathError> {
        for anchor in &self.anchors {
            if anchor.at == at {
                return Ok(PositionAssignment::ManualAnchor {
                    anchor: anchor.id,
                    position: anchor.position(),
                });
            }
        }
        for pair in self.anchors.windows(2) {
            if pair[0].leg == pair[1].leg
                && at.nanoseconds > pair[0].at.nanoseconds
                && at.nanoseconds < pair[1].at.nanoseconds
            {
                let (position, fraction) = interpolate(&pair[0], &pair[1], at.nanoseconds)?;
                return Ok(PositionAssignment::ManualUniformMotion {
                    segment: PathSegmentId {
                        from: pair[0].id,
                        to: pair[1].id,
                    },
                    interpolation_fraction: fraction,
                    position,
                });
            }
        }
        let reason = if self.in_pause_gap(at.nanoseconds) {
            PositionUnavailableReason::PauseGap
        } else if self.phase == ManualPathPhase::Capturing
            && at.nanoseconds > self.anchors.last().unwrap().at.nanoseconds
        {
            PositionUnavailableReason::AwaitingNextAnchor
        } else {
            PositionUnavailableReason::OutsideAnchoredPath
        };
        Ok(PositionAssignment::Unavailable { reason })
    }

    fn in_pause_gap(&self, nanos: u64) -> bool {
        let mut paused_at = None;
        for anchor in &self.anchors {
            if anchor.kind == ManualPathAnchorKind::Pause {
                paused_at = Some(anchor.at.nanoseconds);
            } else if let Some(start) = paused_at {
                if nanos > start && nanos < anchor.at.nanoseconds {
                    return true;
                }
                paused_at = None;
            }
        }
        paused_at.is_some_and(|start| nanos > start)
    }

    /// Speed and turn checks are diagnostics, not hard rejection gates. They
    /// are recomputed from corrected anchors, so a route edit cannot leave stale
    /// warnings behind.
    pub fn diagnostics(&self) -> Result<Vec<PathDiagnostic>, ManualPathError> {
        let mut result = Vec::new();
        for pair in self.anchors.windows(2) {
            if pair[0].leg != pair[1].leg {
                continue;
            }
            let distance = distance_3d(pair[0].position(), pair[1].position())?;
            let elapsed = (pair[1].at.nanoseconds - pair[0].at.nanoseconds) as f64 / 1e9;
            let measured = MetersPerSecond::new(distance / elapsed)
                .map_err(|_| ManualPathError::ArithmeticOverflow)?;
            if measured.get() > self.config.maximum_speed.get() {
                result.push(PathDiagnostic::ImplausibleSpeed {
                    segment: PathSegmentId {
                        from: pair[0].id,
                        to: pair[1].id,
                    },
                    measured,
                    configured_limit: self.config.maximum_speed,
                });
            }
        }
        for triple in self.anchors.windows(3) {
            if triple[0].leg != triple[1].leg || triple[1].leg != triple[2].leg {
                continue;
            }
            let Some(change) = heading_change(
                triple[0].position(),
                triple[1].position(),
                triple[2].position(),
            )?
            else {
                continue;
            };
            if change.get() > self.config.sharp_turn_threshold.get() {
                result.push(PathDiagnostic::SharpTurn {
                    anchor: triple[1].id,
                    heading_change: change,
                    configured_limit: self.config.sharp_turn_threshold,
                });
            }
        }
        Ok(result)
    }

    /// Computes only scheduled intervals lacking caller-supplied complete
    /// channel coverage. Time ranges are half-open `[start, end)`. A gap means
    /// unobserved scheduled time, never absence of an access point.
    pub fn channel_gaps(
        &self,
        schedule: &[ScheduledChannelInterval],
        coverage: &[ChannelCoverageInterval],
    ) -> Result<Vec<ChannelGap>, ManualPathError> {
        if schedule.len() > MAX_CHANNEL_SCHEDULE_INTERVALS
            || coverage.len() > MAX_CHANNEL_COVERAGE_INTERVALS
        {
            return Err(ManualPathError::Limit);
        }
        let mut work = ChannelGapWork::default();
        for entry in schedule {
            work.charge(1)?;
            self.check_interval(entry.interval)?;
        }
        for entry in coverage {
            work.charge(1)?;
            self.check_interval(entry.interval)?;
        }

        let mut gaps = Vec::new();
        for scheduled in schedule {
            work.charge(1)?;
            let mut complete = Vec::new();
            for item in coverage {
                work.charge(1)?;
                if item.frequency == scheduled.frequency
                    && item.completeness == ChannelCoverageCompleteness::Complete
                {
                    complete.push(item.interval);
                }
            }
            sort_channel_intervals(&mut complete, &mut work)?;

            for pair in self.anchors.windows(2) {
                work.charge(1)?;
                if pair[0].leg != pair[1].leg {
                    continue;
                }
                let start = scheduled
                    .interval
                    .start
                    .nanoseconds
                    .max(pair[0].at.nanoseconds);
                let end = scheduled
                    .interval
                    .end_exclusive
                    .nanoseconds
                    .min(pair[1].at.nanoseconds);
                if start >= end {
                    continue;
                }
                let mut cursor = start;
                for observed in &complete {
                    work.charge(1)?;
                    let observed_start = observed.start.nanoseconds.max(start);
                    let observed_end = observed.end_exclusive.nanoseconds.min(end);
                    if observed_start >= observed_end || observed_end <= cursor {
                        continue;
                    }
                    if observed_start > cursor {
                        work.charge(1)?;
                        self.push_channel_gap(&mut gaps, pair, scheduled, cursor, observed_start)?;
                    }
                    cursor = cursor.max(observed_end);
                    if cursor >= end {
                        break;
                    }
                }
                if cursor < end {
                    work.charge(1)?;
                    self.push_channel_gap(&mut gaps, pair, scheduled, cursor, end)?;
                }
            }
        }
        Ok(gaps)
    }

    fn check_interval(&self, interval: ManualTimeInterval) -> Result<(), ManualPathError> {
        if interval.start.epoch != self.config.epoch
            || interval.end_exclusive.epoch != self.config.epoch
        {
            return Err(ManualPathError::WrongClock);
        }
        if interval.start.nanoseconds >= interval.end_exclusive.nanoseconds {
            return Err(ManualPathError::ReversedTime);
        }
        Ok(())
    }

    fn push_channel_gap(
        &self,
        output: &mut Vec<ChannelGap>,
        segment: &[ManualPathAnchor],
        scheduled: &ScheduledChannelInterval,
        start: u64,
        end: u64,
    ) -> Result<(), ManualPathError> {
        if output.len() >= MAX_CHANNEL_GAPS {
            return Err(ManualPathError::Limit);
        }
        let (start_position, _) = interpolate(&segment[0], &segment[1], start)?;
        let (end_position, _) = interpolate(&segment[0], &segment[1], end)?;
        let start = MonotonicTimestamp {
            epoch: self.config.epoch,
            nanoseconds: start,
        };
        let end = MonotonicTimestamp {
            epoch: self.config.epoch,
            nanoseconds: end,
        };
        output.push(ChannelGap {
            segment: PathSegmentId {
                from: segment[0].id,
                to: segment[1].id,
            },
            schedule_evidence_ref: scheduled.evidence_ref.clone(),
            frequency: scheduled.frequency,
            interval: ManualTimeInterval::new(start, end)?,
            start_position,
            end_position,
            interpretation: ChannelGapInterpretation::ScheduledIntervalWithoutCompleteCoverage,
        });
        Ok(())
    }

    fn validate_snapshot(&self) -> Result<(), ManualPathError> {
        if self.schema_version != ManualPathSchemaVersion::V1
            || self.anchors.is_empty()
            || self.anchors.len() > MAX_PATH_ANCHORS
            || self.observations.len() > MAX_PATH_OBSERVATIONS
            || self.anchors[0].kind != ManualPathAnchorKind::Start
            || self.anchors[0].leg != 0
            || self.anchors[0].id != PathAnchorId(1)
            || self.anchors[0].at.epoch != self.config.epoch
        {
            return Err(ManualPathError::InvalidSnapshot);
        }
        let mut expected_phase = ManualPathPhase::Capturing;
        for (index, anchor) in self.anchors.iter().enumerate() {
            if anchor.id.0 != u32::try_from(index + 1).map_err(|_| ManualPathError::Limit)?
                || anchor.at.epoch != self.config.epoch
            {
                return Err(ManualPathError::InvalidSnapshot);
            }
            check_position(anchor.original_position)?;
            if let Some(position) = anchor.edited_position {
                check_position(position)?;
            }
            if index == 0 {
                continue;
            }
            let previous = &self.anchors[index - 1];
            if anchor.at.nanoseconds <= previous.at.nanoseconds {
                return Err(ManualPathError::InvalidSnapshot);
            }
            match (expected_phase, anchor.kind) {
                (ManualPathPhase::Capturing, ManualPathAnchorKind::Turn)
                    if anchor.leg == previous.leg => {}
                (ManualPathPhase::Capturing, ManualPathAnchorKind::Pause)
                    if anchor.leg == previous.leg =>
                {
                    expected_phase = ManualPathPhase::Paused
                }
                (ManualPathPhase::Capturing, ManualPathAnchorKind::Stop)
                    if anchor.leg == previous.leg =>
                {
                    expected_phase = ManualPathPhase::Stopped
                }
                (ManualPathPhase::Paused, ManualPathAnchorKind::Resume)
                    if previous.leg.checked_add(1) == Some(anchor.leg) =>
                {
                    expected_phase = ManualPathPhase::Capturing;
                }
                (ManualPathPhase::Paused, ManualPathAnchorKind::Stop)
                    if previous.leg.checked_add(1) == Some(anchor.leg) =>
                {
                    expected_phase = ManualPathPhase::Stopped;
                }
                _ => return Err(ManualPathError::InvalidSnapshot),
            }
            if expected_phase == ManualPathPhase::Stopped && index + 1 != self.anchors.len() {
                return Err(ManualPathError::InvalidSnapshot);
            }
        }
        if self.phase != expected_phase {
            return Err(ManualPathError::InvalidSnapshot);
        }
        let first = self.anchors[0].at.nanoseconds;
        let last = self.anchors.last().unwrap().at.nanoseconds;
        let mut ids = BTreeSet::new();
        let mut previous_key = None;
        for sample in &self.observations {
            if sample.captured_at.epoch != self.config.epoch
                || sample.captured_at.nanoseconds < first
                || (self.phase == ManualPathPhase::Stopped && sample.captured_at.nanoseconds > last)
                || !ids.insert(sample.observation_id)
            {
                return Err(ManualPathError::InvalidSnapshot);
            }
            if let Evidence::Known(pose) = &sample.reported_pose
                && pose.frame_id != self.config.frame_id
            {
                return Err(ManualPathError::WrongFrame);
            }
            let key = (sample.captured_at.nanoseconds, sample.observation_id);
            if previous_key.is_some_and(|previous| previous > key) {
                return Err(ManualPathError::InvalidSnapshot);
            }
            previous_key = Some(key);
        }
        self.validate_derived()
    }

    fn validate_derived(&self) -> Result<(), ManualPathError> {
        self.diagnostics()?;
        for sample in &self.observations {
            self.position_observation(sample)?;
        }
        Ok(())
    }
}

fn check_position(position: Point3) -> Result<(), ManualPathError> {
    if !position.x.get().is_finite()
        || !position.y.get().is_finite()
        || !position.z.get().is_finite()
    {
        return Err(ManualPathError::ArithmeticOverflow);
    }
    Ok(())
}

fn interpolate(
    start: &ManualPathAnchor,
    end: &ManualPathAnchor,
    at: u64,
) -> Result<(Point3, f64), ManualPathError> {
    let total = end
        .at
        .nanoseconds
        .checked_sub(start.at.nanoseconds)
        .ok_or(ManualPathError::ArithmeticOverflow)?;
    let elapsed = at
        .checked_sub(start.at.nanoseconds)
        .ok_or(ManualPathError::ArithmeticOverflow)?;
    if total == 0 || elapsed > total || start.leg != end.leg {
        return Err(ManualPathError::InvalidSnapshot);
    }
    let fraction = timestamp_fraction(elapsed, total)?;
    let a = start.position();
    let b = end.position();
    let position = Point3 {
        x: CoordinateMeters::new(
            lerp(a.x.get(), b.x.get(), fraction).ok_or(ManualPathError::ArithmeticOverflow)?,
        )
        .map_err(|_| ManualPathError::ArithmeticOverflow)?,
        y: CoordinateMeters::new(
            lerp(a.y.get(), b.y.get(), fraction).ok_or(ManualPathError::ArithmeticOverflow)?,
        )
        .map_err(|_| ManualPathError::ArithmeticOverflow)?,
        z: CoordinateMeters::new(
            lerp(a.z.get(), b.z.get(), fraction).ok_or(ManualPathError::ArithmeticOverflow)?,
        )
        .map_err(|_| ManualPathError::ArithmeticOverflow)?,
    };
    Ok((position, fraction))
}

fn timestamp_fraction(elapsed: u64, total: u64) -> Result<f64, ManualPathError> {
    if total == 0 || elapsed > total {
        return Err(ManualPathError::InvalidSnapshot);
    }
    if elapsed == 0 {
        return Ok(0.0);
    }
    if elapsed == total {
        return Ok(1.0);
    }
    let fraction = elapsed as f64 / total as f64;
    if !fraction.is_finite() || fraction <= 0.0 {
        return Err(ManualPathError::ArithmeticOverflow);
    }
    // Adjacent interior u64 timestamps near a 64-bit-scale interval may round
    // to 1.0 as f64. Keep interior timestamps strictly interior so they never
    // collapse to an endpoint (the output coordinate still has f64 precision).
    Ok(if fraction >= 1.0 {
        f64::from_bits(1.0_f64.to_bits() - 1)
    } else {
        fraction
    })
}

fn lerp(a: f64, b: f64, fraction: f64) -> Option<f64> {
    let value = a * (1.0 - fraction) + b * fraction;
    value.is_finite().then_some(value)
}

fn distance_3d(a: Point3, b: Point3) -> Result<f64, ManualPathError> {
    let dx = b.x.get() - a.x.get();
    let dy = b.y.get() - a.y.get();
    let dz = b.z.get() - a.z.get();
    if !dx.is_finite() || !dy.is_finite() || !dz.is_finite() {
        return Err(ManualPathError::ArithmeticOverflow);
    }
    let distance = dx.hypot(dy).hypot(dz);
    if distance.is_finite() {
        Ok(distance)
    } else {
        Err(ManualPathError::ArithmeticOverflow)
    }
}

fn heading_change(
    before: Point3,
    at: Point3,
    after: Point3,
) -> Result<Option<Radians>, ManualPathError> {
    let ix = at.x.get() - before.x.get();
    let iy = at.y.get() - before.y.get();
    let ox = after.x.get() - at.x.get();
    let oy = after.y.get() - at.y.get();
    if ![ix, iy, ox, oy].into_iter().all(f64::is_finite) {
        return Err(ManualPathError::ArithmeticOverflow);
    }
    let in_length = ix.hypot(iy);
    let out_length = ox.hypot(oy);
    if in_length == 0.0 || out_length == 0.0 {
        return Ok(None);
    }
    let ix = ix / in_length;
    let iy = iy / in_length;
    let ox = ox / out_length;
    let oy = oy / out_length;
    let cross = ix * oy - iy * ox;
    let dot = (ix * ox + iy * oy).clamp(-1.0, 1.0);
    let angle = cross.abs().atan2(dot);
    if !angle.is_finite() {
        return Err(ManualPathError::ArithmeticOverflow);
    }
    Ok(Some(
        Radians::new(angle).map_err(|_| ManualPathError::ArithmeticOverflow)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_domain::evidence::UnknownReason;

    fn epoch() -> ClockEpochId {
        ClockEpochId::from_bytes([1; 16]).unwrap()
    }

    fn timestamp(nanoseconds: u64) -> MonotonicTimestamp {
        MonotonicTimestamp {
            epoch: epoch(),
            nanoseconds,
        }
    }

    fn position(x: f64) -> Point3 {
        Point3 {
            x: CoordinateMeters::new(x).unwrap(),
            y: CoordinateMeters::new(0.0).unwrap(),
            z: CoordinateMeters::new(0.0).unwrap(),
        }
    }

    fn survey() -> ManualPathSurvey {
        ManualPathSurvey::start(
            ManualPathConfig::new(
                FrameId::from_bytes([2; 16]).unwrap(),
                epoch(),
                MetersPerSecond::new(1.0).unwrap(),
                Radians::new(1.0).unwrap(),
                Meters::new(0.1).unwrap(),
            )
            .unwrap(),
            timestamp(0),
            position(0.0),
        )
        .unwrap()
    }

    #[test]
    fn anchor_and_observation_limits_are_checked_before_growth() {
        let mut anchors_full = survey();
        let start = anchors_full.anchors[0].clone();
        anchors_full.anchors.resize(MAX_PATH_ANCHORS, start);
        assert_eq!(
            anchors_full.turn(timestamp(1), position(1.0)),
            Err(ManualPathError::Limit)
        );

        let mut observations_full = survey();
        let sample = ManualPathObservation {
            observation_id: ObservationId::from_bytes([3; 16]).unwrap(),
            captured_at: timestamp(1),
            reported_pose: Evidence::Unknown(UnknownReason::NotMeasured),
        };
        observations_full
            .observations
            .resize(MAX_PATH_OBSERVATIONS, sample);
        assert_eq!(
            observations_full.record_observation(
                ObservationId::from_bytes([4; 16]).unwrap(),
                timestamp(2),
                Evidence::Unknown(UnknownReason::NotMeasured),
            ),
            Err(ManualPathError::Limit)
        );
    }
}
