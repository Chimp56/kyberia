//! Receipt-based spatial association for observations whose source does not
//! expose a hardware capture timestamp or pose.
//!
//! A native managed-mode scan can be returned while a user is standing at a
//! selected point, even though CoreWLAN does not tell us when the radio heard
//! the beacon or where the radio was at that instant.  This module records
//! that fact as a separate normalized association.  It never edits the
//! observation envelope and never feeds receipt evidence into the strict
//! capture, freshness, dwell or channel-completeness gates in `PointSurvey`.

use crate::*;
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    observation::{CalibrationState, DwellContext, ObservationPayload, ReceivedObservation},
    spatial::PoseReference,
    time::{MonotonicTimestamp, MonotonicWindow},
    units::Seconds,
};
use serde::{Deserialize, Serialize};

const ASSOCIATION_METHOD_VERSION: &str = "point-receipt-anchor/v1";

/// Version of the normalized point-association fact.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PointAssociationSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

/// The evidence used to place an observation in the active point window.
///
/// `ApiWindow` is accepted only when the source does not expose a monotonic
/// return time and the whole request starts inside the active point.  When a
/// source return time is known, `Receipt` is used and the source's response
/// timestamp remains separately available in `source_response`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointAssociationTimeBasis {
    ApiWindow { window: MonotonicWindow },
    Receipt { returned_at: MonotonicTimestamp },
}

/// The spatial assignment is an operator-selected point anchor, rather than
/// a claim about the receiver's physical pose at RF capture time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointAssociationPositionBasis {
    SelectedPointAnchor { method_version: Text },
}

/// A normalized fact linking one immutable observation to a selected point.
///
/// The envelope's actual capture time, dwell, pose and scan cache age are
/// copied as evidence and remain unknown when the source did not provide
/// them.  The canonical observation referenced by `observation_id` remains the
/// authority for the complete raw/normalized record; these fields make the
/// association auditable without making it a second observation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "AssociationWire", into = "AssociationWire")]
pub struct PointObservationAssociation {
    schema_version: PointAssociationSchemaVersion,
    method_version: Text,
    point_id: PointId,
    observation_id: ObservationId,
    session_id: SessionId,
    source_id: SourceId,
    assigned_position: PoseReference,
    position_basis: PointAssociationPositionBasis,
    observation_pose: Evidence<PoseReference>,
    time_basis: PointAssociationTimeBasis,
    source_response: SourceResponseTiming,
    capture_time: CaptureTime,
    dwell: Evidence<DwellContext>,
    result_age: Evidence<Seconds>,
    channel: Evidence<ChannelContext>,
    calibration: Evidence<CalibrationState>,
    raw_source: Evidence<ArtifactReference>,
    source_version: Evidence<Text>,
    parser_version: Text,
    quality: Vec<QualityFlag>,
    /// There is no capture-time error bar when only API receipt evidence is
    /// available.  Keep this explicit rather than treating request duration
    /// as capture uncertainty.
    temporal_uncertainty: Evidence<Seconds>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssociationWire {
    schema_version: PointAssociationSchemaVersion,
    method_version: Text,
    point_id: PointId,
    observation_id: ObservationId,
    session_id: SessionId,
    source_id: SourceId,
    assigned_position: PoseReference,
    position_basis: PointAssociationPositionBasis,
    observation_pose: Evidence<PoseReference>,
    time_basis: PointAssociationTimeBasis,
    source_response: SourceResponseTiming,
    capture_time: CaptureTime,
    dwell: Evidence<DwellContext>,
    result_age: Evidence<Seconds>,
    channel: Evidence<ChannelContext>,
    calibration: Evidence<CalibrationState>,
    raw_source: Evidence<ArtifactReference>,
    source_version: Evidence<Text>,
    parser_version: Text,
    quality: Vec<QualityFlag>,
    temporal_uncertainty: Evidence<Seconds>,
}

impl PointObservationAssociation {
    #[allow(clippy::too_many_arguments)]
    fn new(
        point_id: PointId,
        envelope: &ObservationEnvelope,
        time_basis: PointAssociationTimeBasis,
        source_response: SourceResponseTiming,
        assigned_position: PoseReference,
    ) -> Result<Self, SurveyError> {
        let data = envelope.data();
        let (calibration, result_age) = copied_payload_evidence(&data.payload);
        let association = Self {
            schema_version: PointAssociationSchemaVersion::V1,
            method_version: Text::new(ASSOCIATION_METHOD_VERSION)
                .map_err(|_| SurveyError::InvalidSnapshot)?,
            point_id,
            observation_id: data.id,
            session_id: data.session_id,
            source_id: data.source.source_id,
            assigned_position,
            position_basis: PointAssociationPositionBasis::SelectedPointAnchor {
                method_version: Text::new(ASSOCIATION_METHOD_VERSION)
                    .map_err(|_| SurveyError::InvalidSnapshot)?,
            },
            observation_pose: data.pose.clone(),
            time_basis,
            source_response,
            capture_time: data.time.clone(),
            dwell: data.dwell.clone(),
            result_age,
            channel: data.channel.clone(),
            calibration,
            raw_source: data.raw_source.clone(),
            source_version: data.source.source_version.clone(),
            parser_version: data.source.parser_version.clone(),
            quality: data.quality.clone(),
            temporal_uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
        };
        association.validate_local()?;
        Ok(association)
    }

    /// Check the fields copied from a canonical observation into this
    /// association. Receipt timing remains a separate evidence plane:
    /// `source_response` and its derived `time_basis` deliberately do not
    /// compare with the observation's capture clock. The selected point
    /// anchor is also association context rather than envelope data.
    pub fn matches_canonical_observation(&self, envelope: &ObservationEnvelope) -> bool {
        let data = envelope.data();
        let (calibration, result_age) = copied_payload_evidence(&data.payload);
        self.observation_id == data.id
            && self.session_id == data.session_id
            && self.source_id == data.source.source_id
            && self.observation_pose == data.pose
            && self.capture_time == data.time
            && self.dwell == data.dwell
            && self.result_age == result_age
            && self.channel == data.channel
            && self.calibration == calibration
            && self.raw_source == data.raw_source
            && self.source_version == data.source.source_version
            && self.parser_version == data.source.parser_version
            && self.quality == data.quality
    }

    fn validate_local(&self) -> Result<(), SurveyError> {
        if self.schema_version != PointAssociationSchemaVersion::V1
            || self.method_version.as_str() != ASSOCIATION_METHOD_VERSION
            || self.quality.len() > 32
            || self.temporal_uncertainty != Evidence::Unknown(UnknownReason::NotMeasured)
            || self.quality.iter().any(|flag| {
                matches!(
                    flag,
                    QualityFlag::Malformed | QualityFlag::ContradictorySourceFields
                )
            })
        {
            return Err(SurveyError::InvalidSnapshot);
        }
        match (&self.time_basis, self.source_response.api_window()) {
            (PointAssociationTimeBasis::ApiWindow { window }, Evidence::Known(source_window))
                if window == source_window => {}
            (PointAssociationTimeBasis::ApiWindow { .. }, _) => {
                return Err(SurveyError::InvalidSnapshot);
            }
            (PointAssociationTimeBasis::Receipt { returned_at }, _) => {
                if self.source_response.returned_at().monotonic.as_known() != Some(returned_at) {
                    return Err(SurveyError::InvalidSnapshot);
                }
            }
        }
        if let Some(returned) = self.source_response.returned_at().monotonic.as_known()
            && let PointAssociationTimeBasis::ApiWindow { window } = self.time_basis
            && returned.epoch != window.start().epoch
        {
            return Err(SurveyError::InvalidSnapshot);
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> PointAssociationSchemaVersion {
        self.schema_version
    }

    pub fn method_version(&self) -> &Text {
        &self.method_version
    }

    pub const fn point_id(&self) -> PointId {
        self.point_id
    }

    pub const fn observation_id(&self) -> ObservationId {
        self.observation_id
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }

    pub const fn assigned_position(&self) -> &PoseReference {
        &self.assigned_position
    }

    pub const fn position_basis(&self) -> &PointAssociationPositionBasis {
        &self.position_basis
    }

    pub const fn observation_pose(&self) -> &Evidence<PoseReference> {
        &self.observation_pose
    }

    pub const fn time_basis(&self) -> &PointAssociationTimeBasis {
        &self.time_basis
    }

    pub const fn source_response(&self) -> &SourceResponseTiming {
        &self.source_response
    }

    pub const fn capture_time(&self) -> &CaptureTime {
        &self.capture_time
    }

    pub const fn dwell(&self) -> &Evidence<DwellContext> {
        &self.dwell
    }

    pub const fn result_age(&self) -> &Evidence<Seconds> {
        &self.result_age
    }

    pub const fn channel(&self) -> &Evidence<ChannelContext> {
        &self.channel
    }

    pub const fn calibration(&self) -> &Evidence<CalibrationState> {
        &self.calibration
    }

    pub const fn raw_source(&self) -> &Evidence<ArtifactReference> {
        &self.raw_source
    }

    pub const fn source_version(&self) -> &Evidence<Text> {
        &self.source_version
    }

    pub const fn parser_version(&self) -> &Text {
        &self.parser_version
    }

    pub fn quality(&self) -> &[QualityFlag] {
        &self.quality
    }

    pub const fn temporal_uncertainty(&self) -> &Evidence<Seconds> {
        &self.temporal_uncertainty
    }

    /// Receipt associations do not count as strict point-survey evidence.
    pub const fn counts_toward_strict_point_gate(&self) -> bool {
        false
    }

    pub(crate) fn time_bounds(&self) -> (u64, u64) {
        match self.time_basis {
            PointAssociationTimeBasis::ApiWindow { window } => {
                (window.start().nanoseconds, window.end().nanoseconds)
            }
            PointAssociationTimeBasis::Receipt { returned_at } => {
                (returned_at.nanoseconds, returned_at.nanoseconds)
            }
        }
    }

    pub(crate) fn validate_for_config(&self, config: &PointConfig) -> Result<(), SurveyError> {
        self.validate_local()?;
        let cfg = config.data();
        if self.point_id != cfg.point_id
            || self.session_id != cfg.session_id
            || self.source_id != cfg.source_id
            || self.assigned_position != cfg.anchor
        {
            return Err(SurveyError::InvalidSnapshot);
        }
        if !cfg.allow_synthetic && self.quality.contains(&QualityFlag::SyntheticFixture) {
            return Err(SurveyError::InvalidSnapshot);
        }
        if let PointAssociationPositionBasis::SelectedPointAnchor { method_version } =
            &self.position_basis
            && method_version.as_str() != ASSOCIATION_METHOD_VERSION
        {
            return Err(SurveyError::InvalidSnapshot);
        }
        match self.time_basis {
            PointAssociationTimeBasis::ApiWindow { window }
                if window.start().epoch != cfg.epoch =>
            {
                return Err(SurveyError::InvalidSnapshot);
            }
            PointAssociationTimeBasis::Receipt { returned_at }
                if returned_at.epoch != cfg.epoch =>
            {
                return Err(SurveyError::InvalidSnapshot);
            }
            _ => {}
        }
        if let Evidence::Known(pose) = &self.observation_pose {
            config.check_pose(&Evidence::Known(pose.clone()))?;
        }
        Ok(())
    }
}

fn copied_payload_evidence(
    payload: &ObservationPayload,
) -> (Evidence<CalibrationState>, Evidence<Seconds>) {
    match payload {
        ObservationPayload::Scan(scan) => {
            (scan.signal.calibration.clone(), scan.result_age.clone())
        }
        ObservationPayload::Frame(frame) => (
            frame.signal.calibration.clone(),
            Evidence::Unknown(UnknownReason::NotApplicable),
        ),
        ObservationPayload::Health(_) => (
            Evidence::Unknown(UnknownReason::NotApplicable),
            Evidence::Unknown(UnknownReason::NotApplicable),
        ),
    }
}

impl TryFrom<AssociationWire> for PointObservationAssociation {
    type Error = SurveyError;

    fn try_from(wire: AssociationWire) -> Result<Self, Self::Error> {
        let association = Self {
            schema_version: wire.schema_version,
            method_version: wire.method_version,
            point_id: wire.point_id,
            observation_id: wire.observation_id,
            session_id: wire.session_id,
            source_id: wire.source_id,
            assigned_position: wire.assigned_position,
            position_basis: wire.position_basis,
            observation_pose: wire.observation_pose,
            time_basis: wire.time_basis,
            source_response: wire.source_response,
            capture_time: wire.capture_time,
            dwell: wire.dwell,
            result_age: wire.result_age,
            channel: wire.channel,
            calibration: wire.calibration,
            raw_source: wire.raw_source,
            source_version: wire.source_version,
            parser_version: wire.parser_version,
            quality: wire.quality,
            temporal_uncertainty: wire.temporal_uncertainty,
        };
        association.validate_local()?;
        Ok(association)
    }
}

impl From<PointObservationAssociation> for AssociationWire {
    fn from(association: PointObservationAssociation) -> Self {
        Self {
            schema_version: association.schema_version,
            method_version: association.method_version,
            point_id: association.point_id,
            observation_id: association.observation_id,
            session_id: association.session_id,
            source_id: association.source_id,
            assigned_position: association.assigned_position,
            position_basis: association.position_basis,
            observation_pose: association.observation_pose,
            time_basis: association.time_basis,
            source_response: association.source_response,
            capture_time: association.capture_time,
            dwell: association.dwell,
            result_age: association.result_age,
            channel: association.channel,
            calibration: association.calibration,
            raw_source: association.raw_source,
            source_version: association.source_version,
            parser_version: association.parser_version,
            quality: association.quality,
            temporal_uncertainty: association.temporal_uncertainty,
        }
    }
}

impl PointSurvey {
    /// Normalized receipt associations retained alongside strict evidence.
    pub fn associations(&self) -> &[PointObservationAssociation] {
        &self.0.associations
    }

    /// Associate a source API result with the selected point using only
    /// explicit source receipt/request timing. The returned state is a new
    /// immutable state; strict capture admission remains a separate operation.
    pub fn associate_received(
        &self,
        received: &ReceivedObservation,
    ) -> Result<(Self, PointObservationAssociation), SurveyError> {
        if self.0.phase != PointPhase::Capturing {
            return Err(SurveyError::InvalidTransition);
        }
        if self.0.associations.len() >= MAX_RECORDS {
            return Err(SurveyError::Limit);
        }
        let cfg = self.0.config.data();
        let data = received.envelope().data();
        if data.session_id != cfg.session_id {
            return Err(SurveyError::WrongSession);
        }
        if data.source.source_id != cfg.source_id
            || data.source.collector_id != cfg.collector_id
            || data.source.adapter_version != cfg.adapter_version
        {
            return Err(SurveyError::WrongSource);
        }
        if !cfg.allow_synthetic
            && (data.source.kind == SourceKind::SyntheticFixture
                || data.quality.contains(&QualityFlag::SyntheticFixture))
        {
            return Err(SurveyError::UnusableQuality);
        }
        if data.quality.iter().any(|flag| {
            matches!(
                flag,
                QualityFlag::Malformed | QualityFlag::ContradictorySourceFields
            )
        }) {
            return Err(SurveyError::UnusableQuality);
        }
        if matches!(&data.payload, ObservationPayload::Health(_)) {
            return Err(SurveyError::UnsupportedPayload);
        }
        if let Evidence::Known(pose) = &data.pose {
            self.0.config.check_pose(&Evidence::Known(pose.clone()))?;
        }
        let response = received
            .source_response()
            .as_known()
            .ok_or(SurveyError::AssociationTimeUnavailable)?;
        let active_start = self
            .0
            .windows
            .last()
            .filter(|window| window.end.is_none())
            .ok_or(SurveyError::InvalidSnapshot)?
            .start;
        let (time_basis, progress_time) =
            association_basis(response, cfg.epoch, active_start, self.event_last())?;
        if self
            .0
            .associations
            .iter()
            .any(|association| association.observation_id() == data.id)
        {
            return Err(SurveyError::DuplicateAssociation);
        }
        if self.has_record(data.id) {
            return Err(SurveyError::DuplicateObservation);
        }
        let association = PointObservationAssociation::new(
            cfg.point_id,
            received.envelope(),
            time_basis,
            response.clone(),
            cfg.anchor.clone(),
        )?;
        let mut next = self.clone();
        if progress_time > next.event_last() {
            next.0.event_last = Some(progress_time);
        }
        next.0.associations.push(association.clone());
        Ok((next, association))
    }
}

fn association_basis(
    response: &SourceResponseTiming,
    epoch: ClockEpochId,
    active_start: u64,
    last: u64,
) -> Result<(PointAssociationTimeBasis, u64), SurveyError> {
    let returned = response.returned_at().monotonic.as_known().copied();
    let window = response.api_window().as_known().copied();
    if let Some(returned) = returned
        && returned.epoch != epoch
    {
        return Err(SurveyError::WrongClock);
    }
    if let Some(window) = window
        && window.start().epoch != epoch
    {
        return Err(SurveyError::WrongClock);
    }
    if let Some(returned) = returned {
        if returned.nanoseconds < active_start {
            return Err(SurveyError::AssociationOutsidePoint);
        }
        if returned.nanoseconds < last {
            return Err(SurveyError::ReversedTime);
        }
        // A known source return is the least ambiguous association evidence;
        // API request bounds remain in source_response for audit.
        return Ok((
            PointAssociationTimeBasis::Receipt {
                returned_at: returned,
            },
            returned.nanoseconds,
        ));
    }
    let Some(window) = window else {
        return Err(SurveyError::AssociationTimeUnavailable);
    };
    if window.end().nanoseconds <= active_start {
        return Err(SurveyError::AssociationOutsidePoint);
    }
    // A request spanning the point boundary cannot establish that the result
    // belongs to this point when the source supplies no response timestamp.
    if window.start().nanoseconds < active_start {
        return Err(SurveyError::AssociationAmbiguous);
    }
    if window.end().nanoseconds < last {
        return Err(SurveyError::ReversedTime);
    }
    Ok((
        PointAssociationTimeBasis::ApiWindow { window },
        window.end().nanoseconds,
    ))
}
