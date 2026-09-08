//! The inward bridge from canonical observations and survey assignments to
//! measured RSSI analysis inputs.
//!
//! This crate intentionally knows nothing about SQLite, Parquet, capture
//! adapters, or UI state.  An outer application service loads bounded,
//! immutable envelopes and survey snapshots, then passes them here.  The
//! resulting selection manifest is the evidence boundary for the numerical
//! engine: it records the exact identity, assignment, and policy used to
//! produce [`kyberia_spatial_analysis::Sample`] values.

use kyberia_domain::{
    analysis::ExactU64,
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{
        AdapterId, CollectorId, ContentHash, FloorId, FrameId, MacAddress, ObservationId,
        ProjectId, RadioId, SessionId, SourceId,
    },
    observation::{
        CalibrationState, ChannelContext, DwellContext, EnvelopeData, ObservationEnvelope,
        ObservationPayload, QualityFlag, SourceKind,
    },
    spatial::{Point3, PoseReference, PositionCovariance},
    time::{CaptureTime, MonotonicWindow},
    units::Dbm,
};
use kyberia_spatial_analysis::{
    Config as SpatialConfig, InputEvidencePlane, Inputs as SpatialInputs, Model, Sample, Tile,
};
use kyberia_survey::{
    CaptureMode, PointAssociationTimeBasis, PointConfig, PointId, PointObservationAssociation,
    PointSurvey, PosePolicy, Target,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const SELECTION_SCHEMA: &str = "kyberia.observed-rssi-selection/1";
pub const SELECTION_MEDIA_TYPE: &str = "application/kyberia-observed-rssi-selection+json";
pub const MAX_OBSERVATIONS: usize = 4_096;
pub const MAX_SURVEYS: usize = 1_024;
pub const MAX_SOURCE_CHUNKS: usize = 128;
pub const MAX_MANIFEST_BYTES: usize = 1_048_576;
pub const MAX_MANIFEST_DEPTH: usize = 32;

/// The application request is deliberately explicit about the immutable
/// project revision and spatial scope.  Observation IDs are the bounded
/// query key; a broad BSSID scan cannot accidentally become an unbounded
/// store operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionRequest {
    pub project_id: ProjectId,
    pub project_revision: u64,
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub target_bssid: MacAddress,
    pub observation_ids: Vec<ObservationId>,
    /// A bounded receipt copied from the storage adapter after it has
    /// verified the selected immutable chunks.  The analysis crate records
    /// this claim but cannot verify storage bytes without importing the
    /// storage implementation.
    pub source: SelectionSourceBinding,
    pub session_scope: Option<SessionId>,
    pub source_scope: Option<SourceId>,
    pub adapter_scope: Option<AdapterId>,
    /// An uncalibrated signal is still measured evidence, but callers must
    /// opt into retaining it because no correction is applied here.
    pub allow_uncalibrated: bool,
}

/// Storage-independent provenance copied from a verified indexed observation
/// query.  An outer adapter must construct this only from its validated query
/// receipt; this type validates canonical shape and bounds, not filesystem or
/// artifact bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionSourceBinding {
    pub project_revision: ExactU64,
    pub selected_chunk_hashes: Vec<ContentHash>,
}

impl SelectionSourceBinding {
    pub fn from_verified_query(
        project_revision: u64,
        selected_chunk_hashes: Vec<ContentHash>,
    ) -> Result<Self, SelectionError> {
        if selected_chunk_hashes.is_empty() {
            return Err(SelectionError::InvalidRequest(
                "source receipt has no selected chunks",
            ));
        }
        if selected_chunk_hashes.len() > MAX_SOURCE_CHUNKS {
            return Err(SelectionError::ResourceLimit("source chunks"));
        }
        if selected_chunk_hashes
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(SelectionError::InvalidRequest(
                "source chunk hashes are not strictly sorted",
            ));
        }
        Ok(Self {
            project_revision: ExactU64::new(project_revision),
            selected_chunk_hashes,
        })
    }

    pub const fn project_revision(&self) -> u64 {
        self.project_revision.get()
    }

    pub fn selected_chunk_hashes(&self) -> &[ContentHash] {
        &self.selected_chunk_hashes
    }
}

/// A point survey paired with its explicit floor binding.  PointConfig has a
/// coordinate frame but does not own floor identity, so silently inferring a
/// floor would violate the canonical geometry boundary.
#[derive(Clone, Copy, Debug)]
pub struct SurveyInput<'a> {
    pub survey: &'a PointSurvey,
    pub floor_id: FloorId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionError {
    InvalidRequest(&'static str),
    ResourceLimit(&'static str),
    DuplicateObservation(ObservationId),
    MissingObservation(ObservationId),
    DuplicateSurveyObservation(ObservationId),
    SurveyMismatch(&'static str),
    ObservationMismatch(ObservationId, &'static str),
    UnsupportedEvidence(ObservationId, UnknownReason),
    InvalidCalibration(ObservationId),
    InvalidManifest(&'static str),
    Serialization,
    Spatial(kyberia_spatial_analysis::Error),
}
impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SelectionError {}
impl From<kyberia_spatial_analysis::Error> for SelectionError {
    fn from(error: kyberia_spatial_analysis::Error) -> Self {
        Self::Spatial(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionEvidencePlane {
    Measured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionBasis {
    ReportedCapturePose,
    SelectedPointAnchor,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssociationReference {
    pub point_id: PointId,
    pub observation_id: ObservationId,
    pub association_hash: ContentHash,
    pub time_basis: AssociationTimeKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationTimeKind {
    ApiWindow,
    Receipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedRssiRecord {
    pub observation_id: ObservationId,
    pub bssid: MacAddress,
    pub session_id: SessionId,
    pub source_id: SourceId,
    pub collector_id: CollectorId,
    pub adapter_id: Evidence<AdapterId>,
    pub radio_id: Evidence<RadioId>,
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub point_id: PointId,
    pub capture_mode: CaptureMode,
    pub maximum_scan_age: kyberia_domain::units::Seconds,
    pub assignment_pose: PoseReference,
    pub pose_policy: PosePolicy,
    pub position: Point3,
    pub position_covariance: Evidence<PositionCovariance>,
    pub position_basis: PositionBasis,
    pub association: Option<AssociationReference>,
    pub observation_pose: Evidence<PoseReference>,
    pub capture_time: CaptureTime,
    pub association_time: Option<AssociationTime>,
    pub result_age: Evidence<kyberia_domain::units::Seconds>,
    pub calibration: Evidence<CalibrationState>,
    pub channel: Evidence<ChannelContext>,
    pub dwell: Evidence<DwellContext>,
    pub raw_source: Evidence<ArtifactReference>,
    pub source_version: Evidence<kyberia_domain::identity::Text>,
    pub source_schema_version: kyberia_domain::identity::Text,
    pub adapter_version: kyberia_domain::identity::Text,
    pub parser_version: kyberia_domain::identity::Text,
    pub quality: Vec<QualityFlag>,
    pub measurement_method: kyberia_domain::identity::Text,
    pub rssi_dbm: Dbm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssociationTime {
    pub basis: AssociationTimeKind,
    pub api_window: Option<MonotonicWindow>,
    pub returned_at: Option<CaptureTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "reason")]
pub enum RejectionReason {
    MissingObservation,
    MissingAssociation,
    AdmissionMismatch,
    WrongBssid,
    UnknownBssid(UnknownReason),
    UnknownRssi(UnknownReason),
    ResultAgeUnavailable,
    StaleScan,
    UnsupportedPayload,
    UnusableQuality,
    Uncalibrated,
    ScopeMismatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedEvidence {
    pub observation_id: ObservationId,
    pub association_hash: Option<ContentHash>,
    pub reason: RejectionReason,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricReference {
    pub artifact: kyberia_domain::analysis::VersionedArtifact,
    pub definition_hash: ContentHash,
    pub spatial_method: kyberia_spatial_analysis::SpatialMethod,
    pub signal_aggregation: kyberia_spatial_analysis::SignalAggregationSelection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionPolicy {
    pub target_bssid: MacAddress,
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub project_revision: ExactU64,
    pub source: SelectionSourceBinding,
    pub observation_ids: Vec<ObservationId>,
    pub session_scope: Option<SessionId>,
    pub source_scope: Option<SourceId>,
    pub adapter_scope: Option<AdapterId>,
    pub allow_uncalibrated: bool,
    pub assignment_policy: String,
    pub unknown_policy: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionManifest {
    pub schema: SelectionManifestVersion,
    pub project_id: ProjectId,
    pub evidence_plane: SelectionEvidencePlane,
    pub policy: SelectionPolicy,
    pub metric: MetricReference,
    pub spatial_configuration: SpatialConfig,
    pub selected: Vec<SelectedRssiRecord>,
    pub rejected: Vec<RejectedEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionManifestVersion {
    #[serde(rename = "kyberia.observed-rssi-selection/1")]
    V1,
}

impl SelectionManifest {
    /// Parse only the exact V1 wire representation.  Re-encoding is required
    /// so semantically equivalent but differently ordered JSON cannot obtain a
    /// different evidence identity.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, SelectionError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(SelectionError::ResourceLimit("selection manifest bytes"));
        }
        validate_json_bounds(bytes)?;
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|_| SelectionError::InvalidManifest("malformed selection manifest"))?;
        manifest.validate()?;
        if canonical_manifest(&manifest)? != bytes {
            return Err(SelectionError::InvalidManifest("noncanonical selection"));
        }
        Ok(manifest)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SelectionError> {
        self.validate()?;
        canonical_manifest(self)
    }

    pub fn content_hash(&self) -> Result<ContentHash, SelectionError> {
        Ok(ContentHash::from_sha256(
            Sha256::digest(self.canonical_bytes()?).into(),
        ))
    }

    fn validate(&self) -> Result<(), SelectionError> {
        if self.schema != SelectionManifestVersion::V1
            || self.evidence_plane != SelectionEvidencePlane::Measured
            || self.policy.assignment_policy != "survey-assignment/v1"
            || self.policy.unknown_policy != "preserve-rejection-reason/v1"
            || self.metric.artifact.media_type.as_str()
                != kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE
            || self.metric.definition_hash != self.metric.artifact.sha256
        {
            return Err(SelectionError::InvalidManifest("schema or policy"));
        }
        if self.policy.source.project_revision != self.policy.project_revision {
            return Err(SelectionError::InvalidManifest("source revision"));
        }
        self.spatial_configuration
            .validate()
            .map_err(|_| SelectionError::InvalidManifest("spatial configuration"))?;
        if self.metric.spatial_method != config_method(self.spatial_configuration) {
            return Err(SelectionError::InvalidManifest("spatial method"));
        }
        let expected_metric = canonical_observed_rssi(self.spatial_configuration)
            .map_err(|_| SelectionError::InvalidManifest("canonical RSSI metric unavailable"))?;
        if !metric_reference_matches(&self.metric, &expected_metric)? {
            return Err(SelectionError::InvalidManifest("metric identity"));
        }
        if self.policy.observation_ids.is_empty()
            || self.policy.observation_ids.len() > MAX_OBSERVATIONS
            || self.selected.len() > MAX_OBSERVATIONS
            || self.rejected.len() > MAX_OBSERVATIONS
        {
            return Err(SelectionError::ResourceLimit("selection records"));
        }
        if self.policy.source.selected_chunk_hashes.is_empty()
            || self.policy.source.selected_chunk_hashes.len() > MAX_SOURCE_CHUNKS
            || self
                .policy
                .source
                .selected_chunk_hashes
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(SelectionError::InvalidManifest("source chunk binding"));
        }
        if self
            .policy
            .observation_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(SelectionError::InvalidManifest("unsorted observation IDs"));
        }
        if self
            .selected
            .windows(2)
            .any(|pair| pair[0].observation_id >= pair[1].observation_id)
            || self
                .rejected
                .windows(2)
                .any(|pair| pair[0].observation_id >= pair[1].observation_id)
        {
            return Err(SelectionError::InvalidManifest(
                "unsorted selection results",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for record in &self.selected {
            if record.bssid != self.policy.target_bssid
                || record.floor_id != self.policy.floor_id
                || record.frame_id != self.policy.frame_id
                || self
                    .policy
                    .session_scope
                    .is_some_and(|session| record.session_id != session)
                || self
                    .policy
                    .source_scope
                    .is_some_and(|source| record.source_id != source)
                || self
                    .policy
                    .adapter_scope
                    .is_some_and(|adapter| record.adapter_id.as_known() != Some(&adapter))
                || !self
                    .policy
                    .observation_ids
                    .binary_search(&record.observation_id)
                    .is_ok()
                || !seen.insert(record.observation_id)
                || record.association.as_ref().is_some_and(|a| {
                    a.observation_id != record.observation_id || a.point_id != record.point_id
                })
            {
                return Err(SelectionError::InvalidManifest("selected record scope"));
            }
            if record.assignment_pose.frame_id != record.frame_id
                || record.assignment_pose.position != record.position
                || record.assignment_pose.covariance != record.position_covariance
            {
                return Err(SelectionError::InvalidManifest("assignment pose mismatch"));
            }
            let receipt = record.association.is_some();
            if !receipt
                && record.position_basis == PositionBasis::SelectedPointAnchor
                && !matches!(record.pose_policy, PosePolicy::ManualAnchor { .. })
            {
                return Err(SelectionError::InvalidManifest(
                    "manual anchor policy provenance",
                ));
            }
            if record.quality.iter().any(|flag| {
                if receipt {
                    matches!(
                        flag,
                        QualityFlag::Malformed | QualityFlag::ContradictorySourceFields
                    )
                } else {
                    unusable_quality(flag)
                }
            }) {
                return Err(SelectionError::InvalidManifest("unusable selected quality"));
            }
            if !calibration_allowed(&record.calibration, self.policy.allow_uncalibrated) {
                return Err(SelectionError::InvalidManifest("calibration policy"));
            }
            match record.capture_mode {
                CaptureMode::Scan => {
                    if !receipt && record.result_age.as_known().is_none() {
                        return Err(SelectionError::InvalidManifest("strict scan age"));
                    }
                    if record
                        .result_age
                        .as_known()
                        .is_some_and(|age| *age > record.maximum_scan_age)
                    {
                        return Err(SelectionError::InvalidManifest("stale scan"));
                    }
                }
                CaptureMode::Frame => {
                    if record.result_age != Evidence::Unknown(UnknownReason::NotApplicable) {
                        return Err(SelectionError::InvalidManifest("frame result age"));
                    }
                }
            }
            match record.position_basis {
                PositionBasis::ReportedCapturePose => {
                    if receipt
                        || record.observation_pose.as_known() != Some(&record.assignment_pose)
                    {
                        return Err(SelectionError::InvalidManifest("reported pose provenance"));
                    }
                }
                PositionBasis::SelectedPointAnchor => {
                    if !receipt && record.observation_pose.as_known().is_some() {
                        return Err(SelectionError::InvalidManifest("manual anchor provenance"));
                    }
                }
            }
            if record.association.is_some() != record.association_time.is_some()
                || (record.position_basis == PositionBasis::ReportedCapturePose
                    && record.association.is_some())
                || record.association.as_ref().is_some_and(|association| {
                    record
                        .association_time
                        .as_ref()
                        .is_none_or(|time| time.basis != association.time_basis)
                })
            {
                return Err(SelectionError::InvalidManifest("assignment provenance"));
            }
            if let Some(time) = &record.association_time {
                match time.basis {
                    AssociationTimeKind::ApiWindow if time.api_window.is_none() => {
                        return Err(SelectionError::InvalidManifest("missing API window"));
                    }
                    AssociationTimeKind::Receipt if time.returned_at.is_none() => {
                        return Err(SelectionError::InvalidManifest("missing receipt time"));
                    }
                    _ => {}
                }
            }
        }
        for record in &self.rejected {
            if !self
                .policy
                .observation_ids
                .binary_search(&record.observation_id)
                .is_ok()
                || !seen.insert(record.observation_id)
            {
                return Err(SelectionError::InvalidManifest("rejected record scope"));
            }
        }
        if seen.len() != self.policy.observation_ids.len() {
            return Err(SelectionError::InvalidManifest(
                "incomplete selection result",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedRssiSample {
    sample: Sample,
    record: SelectedRssiRecord,
}
impl ObservedRssiSample {
    pub fn sample(&self) -> &Sample {
        &self.sample
    }
    pub fn record(&self) -> &SelectedRssiRecord {
        &self.record
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedObservedRssiSet {
    manifest: SelectionManifest,
    canonical_manifest: Vec<u8>,
    artifact: ArtifactReference,
    metric: kyberia_spatial_analysis::MetricDefinitionBinding,
    samples: Vec<ObservedRssiSample>,
}

impl ValidatedObservedRssiSet {
    /// Select and validate only the supplied canonical observations.  The
    /// caller is responsible for obtaining them through a bounded, revision
    /// consistent query port; this pure function never opens a store.
    pub fn build(
        request: SelectionRequest,
        metric: kyberia_spatial_analysis::MetricDefinitionBinding,
        spatial_configuration: SpatialConfig,
        surveys: &[SurveyInput<'_>],
        observations: &[ObservationEnvelope],
    ) -> Result<Self, SelectionError> {
        validate_request(&request)?;
        if surveys.len() > MAX_SURVEYS {
            return Err(SelectionError::ResourceLimit("surveys"));
        }
        validate_metric(&metric, spatial_configuration)?;

        let mut requested = request.observation_ids.clone();
        requested.sort_unstable();
        if let Some(pair) = requested.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(SelectionError::DuplicateObservation(pair[0]));
        }

        if observations.len() > MAX_OBSERVATIONS {
            return Err(SelectionError::ResourceLimit("observations"));
        }
        let mut by_id = BTreeMap::new();
        for observation in observations {
            if by_id.insert(observation.data().id, observation).is_some() {
                return Err(SelectionError::DuplicateObservation(observation.data().id));
            }
        }
        let mut candidates = BTreeMap::<ObservationId, Candidate<'_>>::new();
        for input in surveys {
            if input.floor_id != request.floor_id
                || input.survey.config().data().anchor.frame_id != request.frame_id
            {
                return Err(SelectionError::SurveyMismatch("floor/frame binding"));
            }
            let config = input.survey.config().data();
            if matches!(config.target, Target::Bssid(b) if b != request.target_bssid) {
                return Err(SelectionError::SurveyMismatch("target BSSID"));
            }
            if let Some(session) = request.session_scope
                && config.session_id != session
            {
                return Err(SelectionError::SurveyMismatch("session scope"));
            }
            if let Some(source) = request.source_scope
                && config.source_id != source
            {
                return Err(SelectionError::SurveyMismatch("source scope"));
            }
            for id in input.survey.progress().observation_ids {
                if requested.binary_search(&id).is_ok()
                    && candidates
                        .insert(id, Candidate::Strict { input: *input })
                        .is_some()
                {
                    return Err(SelectionError::DuplicateSurveyObservation(id));
                }
            }
            for association in input.survey.associations() {
                let id = association.observation_id();
                if requested.binary_search(&id).is_ok()
                    && candidates
                        .insert(
                            id,
                            Candidate::Receipt {
                                input: *input,
                                association,
                            },
                        )
                        .is_some()
                {
                    return Err(SelectionError::DuplicateSurveyObservation(id));
                }
            }
        }

        let mut selected = Vec::new();
        let mut rejected = Vec::new();
        for id in requested.iter().copied() {
            let Some(candidate) = candidates.get(&id) else {
                rejected.push(RejectedEvidence {
                    observation_id: id,
                    association_hash: None,
                    reason: if by_id.contains_key(&id) {
                        RejectionReason::MissingAssociation
                    } else {
                        RejectionReason::MissingObservation
                    },
                });
                continue;
            };
            let Some(observation) = by_id.get(&id).copied() else {
                rejected.push(RejectedEvidence {
                    observation_id: id,
                    association_hash: candidate.association_hash().ok(),
                    reason: RejectionReason::MissingObservation,
                });
                continue;
            };
            match select_one(&request, candidate, observation) {
                Ok(record) => selected.push(record),
                Err(SelectionFailure {
                    reason,
                    association_hash,
                }) => rejected.push(RejectedEvidence {
                    observation_id: id,
                    association_hash,
                    reason,
                }),
            }
        }

        let selected_records = selected.clone();
        let manifest =
            SelectionManifest {
                schema: SelectionManifestVersion::V1,
                project_id: request.project_id,
                evidence_plane: SelectionEvidencePlane::Measured,
                policy: SelectionPolicy {
                    target_bssid: request.target_bssid,
                    floor_id: request.floor_id,
                    frame_id: request.frame_id,
                    project_revision: ExactU64::new(request.project_revision),
                    source: request.source.clone(),
                    observation_ids: requested,
                    session_scope: request.session_scope,
                    source_scope: request.source_scope,
                    adapter_scope: request.adapter_scope,
                    allow_uncalibrated: request.allow_uncalibrated,
                    assignment_policy: "survey-assignment/v1".to_owned(),
                    unknown_policy: "preserve-rejection-reason/v1".to_owned(),
                },
                metric: MetricReference {
                    artifact: metric.artifact().clone(),
                    definition_hash: metric
                        .definition_hash()
                        .map_err(|_| SelectionError::InvalidManifest("metric hash"))?,
                    spatial_method: metric.spatial_method().ok_or(
                        SelectionError::InvalidManifest("legacy metric lacks spatial method"),
                    )?,
                    signal_aggregation: metric.signal_aggregation(),
                },
                spatial_configuration,
                selected: selected_records,
                rejected,
            };
        let canonical_manifest = canonical_manifest(&manifest)?;
        let artifact = artifact_reference(&canonical_manifest)?;
        let samples = selected
            .into_iter()
            .map(|record| ObservedRssiSample {
                sample: spatial_sample(&record),
                record,
            })
            .collect();
        let result = Self {
            manifest,
            canonical_manifest,
            artifact,
            metric,
            samples,
        };
        result.validate_binding()?;
        Ok(result)
    }

    pub fn manifest(&self) -> &SelectionManifest {
        &self.manifest
    }
    pub fn canonical_manifest(&self) -> &[u8] {
        &self.canonical_manifest
    }
    pub fn artifact(&self) -> &ArtifactReference {
        &self.artifact
    }
    pub fn samples(&self) -> impl Iterator<Item = &ObservedRssiSample> {
        self.samples.iter()
    }
    pub fn rejected(&self) -> &[RejectedEvidence] {
        &self.manifest.rejected
    }
    pub fn spatial_inputs(&self) -> SpatialInputs {
        SpatialInputs {
            floor_id: self.manifest.policy.floor_id,
            frame_id: self.manifest.policy.frame_id,
            evidence_plane: InputEvidencePlane::Measured,
            metric_definition: self.metric.clone(),
            source_artifact: self.artifact.clone(),
            samples: self.samples.iter().map(|s| s.sample.clone()).collect(),
        }
    }
    pub fn tile(
        &self,
        grid: kyberia_spatial_analysis::Grid,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Tile, SelectionError> {
        let model = Model::new(self.spatial_inputs(), self.manifest.spatial_configuration)?;
        Ok(model.tile(grid, cancelled)?)
    }

    fn validate_binding(&self) -> Result<(), SelectionError> {
        self.manifest.validate()?;
        if self.manifest.schema != SelectionManifestVersion::V1
            || self.artifact.media_type.as_str() != SELECTION_MEDIA_TYPE
            || self.artifact.byte_length != self.canonical_manifest.len() as u64
            || self.artifact.sha256.bytes()
                != <[u8; 32]>::from(Sha256::digest(&self.canonical_manifest))
        {
            return Err(SelectionError::InvalidManifest(
                "selection artifact binding",
            ));
        }
        let expected = canonical_manifest(&self.manifest)?;
        if expected != self.canonical_manifest {
            return Err(SelectionError::InvalidManifest("noncanonical selection"));
        }
        if self.manifest.selected.len() != self.samples.len()
            || self
                .samples
                .iter()
                .zip(&self.manifest.selected)
                .any(|(sample, record)| sample.record != *record)
        {
            return Err(SelectionError::InvalidManifest("sample manifest mismatch"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Candidate<'a> {
    Strict {
        input: SurveyInput<'a>,
    },
    Receipt {
        input: SurveyInput<'a>,
        association: &'a PointObservationAssociation,
    },
}
impl Candidate<'_> {
    fn association_hash(&self) -> Result<ContentHash, SelectionError> {
        match self {
            Self::Strict { .. } => Err(SelectionError::InvalidManifest("strict association")),
            Self::Receipt { association, .. } => hash_serialized(association),
        }
    }
}

struct SelectionFailure {
    reason: RejectionReason,
    association_hash: Option<ContentHash>,
}
fn failure(candidate: &Candidate<'_>, reason: RejectionReason) -> SelectionFailure {
    SelectionFailure {
        reason,
        association_hash: candidate.association_hash().ok(),
    }
}

fn select_one(
    request: &SelectionRequest,
    candidate: &Candidate<'_>,
    observation: &ObservationEnvelope,
) -> Result<SelectedRssiRecord, SelectionFailure> {
    let data = observation.data();
    let association_hash = candidate.association_hash().ok();
    if data.source.kind == SourceKind::SyntheticFixture {
        return Err(SelectionFailure {
            // Preserve the V1 rejection vocabulary: a synthetic source is an
            // unsupported source plane for this measured selector.
            reason: RejectionReason::UnsupportedPayload,
            association_hash,
        });
    }
    if data.quality.contains(&QualityFlag::SyntheticFixture) {
        return Err(SelectionFailure {
            // Synthetic quality is an unusable evidence flag.  Using the
            // existing V1 reason avoids silently introducing a new wire enum.
            reason: RejectionReason::UnusableQuality,
            association_hash,
        });
    }
    if let Candidate::Strict { input } = candidate
        && input
            .survey
            .validate_admitted_observation(observation)
            .is_err()
    {
        return Err(SelectionFailure {
            reason: RejectionReason::AdmissionMismatch,
            association_hash,
        });
    }
    let config = candidate_config(candidate);
    let configured = config.data();
    if request
        .session_scope
        .is_some_and(|session| session != data.session_id)
        || request
            .source_scope
            .is_some_and(|source| source != data.source.source_id)
        || request
            .adapter_scope
            .is_some_and(|adapter| data.source.adapter_id.as_known() != Some(&adapter))
        || data.session_id != configured.session_id
        || data.source.source_id != configured.source_id
        || data.source.collector_id != configured.collector_id
        || data.source.adapter_version != configured.adapter_version
    {
        return Err(SelectionFailure {
            reason: RejectionReason::ScopeMismatch,
            association_hash,
        });
    }
    let (identity, signal, result_age) = match (&data.payload, config.data().mode) {
        (ObservationPayload::Scan(scan), kyberia_survey::CaptureMode::Scan) => {
            (&scan.identity, &scan.signal, scan.result_age.clone())
        }
        (ObservationPayload::Frame(frame), kyberia_survey::CaptureMode::Frame) => (
            &frame.identity,
            &frame.signal,
            Evidence::Unknown(UnknownReason::NotApplicable),
        ),
        (ObservationPayload::Scan(_) | ObservationPayload::Frame(_), _) => {
            return Err(failure(candidate, RejectionReason::UnsupportedPayload));
        }
        (ObservationPayload::Health(_), _) => {
            return Err(failure(candidate, RejectionReason::UnsupportedPayload));
        }
    };
    let Some(bssid) = identity.bssid.as_known().copied() else {
        let reason = match &identity.bssid {
            Evidence::Unknown(reason) => reason.clone(),
            Evidence::Known(_) => unreachable!(),
        };
        return Err(failure(candidate, RejectionReason::UnknownBssid(reason)));
    };
    if bssid != request.target_bssid {
        return Err(failure(candidate, RejectionReason::WrongBssid));
    }
    let Some(rssi) = signal.rssi_dbm.as_known().copied() else {
        let reason = match &signal.rssi_dbm {
            Evidence::Unknown(reason) => reason.clone(),
            Evidence::Known(_) => unreachable!(),
        };
        return Err(failure(candidate, RejectionReason::UnknownRssi(reason)));
    };
    let receipt_association = matches!(candidate, Candidate::Receipt { .. });
    if let Candidate::Receipt { association, .. } = candidate
        && (association.dwell() != &data.dwell
            || association.result_age() != &result_age
            || association.channel() != &data.channel
            || association.calibration() != &signal.calibration
            || association.raw_source() != &data.raw_source
            || association.source_version() != &data.source.source_version
            || association.parser_version() != &data.source.parser_version
            || association.quality() != data.quality.as_slice())
    {
        return Err(SelectionFailure {
            reason: RejectionReason::ScopeMismatch,
            association_hash,
        });
    }
    if data.quality.iter().any(|flag| {
        if receipt_association {
            matches!(
                flag,
                QualityFlag::Malformed | QualityFlag::ContradictorySourceFields
            )
        } else {
            unusable_quality(flag)
        }
    }) {
        return Err(failure(candidate, RejectionReason::UnusableQuality));
    }
    if !calibration_allowed(&signal.calibration, request.allow_uncalibrated) {
        return Err(failure(candidate, RejectionReason::Uncalibrated));
    }
    let assignment = match candidate_position(candidate, data, request.frame_id) {
        Ok(value) => value,
        Err(reason) => return Err(failure(candidate, reason)),
    };
    if !receipt_association
        && config.data().mode == kyberia_survey::CaptureMode::Scan
        && result_age.as_known().is_none()
    {
        return Err(failure(candidate, RejectionReason::ResultAgeUnavailable));
    }
    if let Some(age) = result_age.as_known()
        && *age > config.data().maximum_scan_age
    {
        return Err(failure(candidate, RejectionReason::StaleScan));
    }
    Ok(SelectedRssiRecord {
        observation_id: data.id,
        bssid,
        session_id: data.session_id,
        source_id: data.source.source_id,
        collector_id: data.source.collector_id,
        adapter_id: data.source.adapter_id.clone(),
        radio_id: identity.radio.clone(),
        floor_id: request.floor_id,
        frame_id: request.frame_id,
        point_id: assignment.point_id,
        capture_mode: config.data().mode,
        maximum_scan_age: config.data().maximum_scan_age,
        assignment_pose: assignment.assignment_pose,
        pose_policy: config.data().pose_policy.clone(),
        position: assignment.position,
        position_covariance: assignment.covariance,
        position_basis: assignment.position_basis,
        association: association_hash.map(|hash| AssociationReference {
            point_id: assignment.point_id,
            observation_id: data.id,
            association_hash: hash,
            time_basis: assignment
                .association_time
                .as_ref()
                .map_or(AssociationTimeKind::Receipt, |time| time.basis),
        }),
        observation_pose: data.pose.clone(),
        capture_time: data.time.clone(),
        association_time: assignment.association_time,
        result_age,
        calibration: signal.calibration.clone(),
        channel: data.channel.clone(),
        dwell: data.dwell.clone(),
        raw_source: data.raw_source.clone(),
        source_version: data.source.source_version.clone(),
        source_schema_version: data.source.source_schema_version.clone(),
        adapter_version: data.source.adapter_version.clone(),
        parser_version: data.source.parser_version.clone(),
        quality: data.quality.clone(),
        measurement_method: signal.measurement_method.clone(),
        rssi_dbm: rssi,
    })
}

fn candidate_config<'a>(candidate: &'a Candidate<'a>) -> &'a PointConfig {
    match candidate {
        Candidate::Strict { input } | Candidate::Receipt { input, .. } => input.survey.config(),
    }
}

struct PositionAssignment {
    position: Point3,
    covariance: Evidence<PositionCovariance>,
    position_basis: PositionBasis,
    association_time: Option<AssociationTime>,
    point_id: PointId,
    assignment_pose: PoseReference,
}

fn candidate_position(
    candidate: &Candidate<'_>,
    data: &EnvelopeData,
    frame_id: FrameId,
) -> Result<PositionAssignment, RejectionReason> {
    match candidate {
        Candidate::Strict { input } => {
            let config = input.survey.config().data();
            if let Evidence::Known(pose) = &data.pose {
                if pose.frame_id != frame_id {
                    return Err(RejectionReason::ScopeMismatch);
                }
                Ok(PositionAssignment {
                    position: pose.position,
                    covariance: pose.covariance.clone(),
                    position_basis: PositionBasis::ReportedCapturePose,
                    association_time: None,
                    point_id: config.point_id,
                    assignment_pose: pose.clone(),
                })
            } else if matches!(config.pose_policy, PosePolicy::ManualAnchor { .. }) {
                Ok(PositionAssignment {
                    position: config.anchor.position,
                    covariance: config.anchor.covariance.clone(),
                    position_basis: PositionBasis::SelectedPointAnchor,
                    association_time: None,
                    point_id: config.point_id,
                    assignment_pose: config.anchor.clone(),
                })
            } else {
                Err(RejectionReason::ScopeMismatch)
            }
        }
        Candidate::Receipt { input, association } => {
            if association.observation_id() != data.id
                || association.session_id() != data.session_id
                || association.source_id() != data.source.source_id
                || association.capture_time() != &data.time
                || association.observation_pose() != &data.pose
                || association.raw_source() != &data.raw_source
                || association.point_id() != input.survey.config().data().point_id
                || association.assigned_position() != &input.survey.config().data().anchor
                || association.assigned_position().frame_id != frame_id
            {
                return Err(RejectionReason::ScopeMismatch);
            }
            let time = match association.time_basis() {
                PointAssociationTimeBasis::ApiWindow { window } => AssociationTime {
                    basis: AssociationTimeKind::ApiWindow,
                    api_window: Some(*window),
                    returned_at: Some(association.source_response().returned_at().clone()),
                },
                PointAssociationTimeBasis::Receipt { .. } => AssociationTime {
                    basis: AssociationTimeKind::Receipt,
                    api_window: association
                        .source_response()
                        .api_window()
                        .as_known()
                        .copied(),
                    returned_at: Some(association.source_response().returned_at().clone()),
                },
            };
            Ok(PositionAssignment {
                position: association.assigned_position().position,
                covariance: association.assigned_position().covariance.clone(),
                position_basis: PositionBasis::SelectedPointAnchor,
                association_time: Some(time),
                point_id: association.point_id(),
                assignment_pose: association.assigned_position().clone(),
            })
        }
    }
}

fn spatial_sample(record: &SelectedRssiRecord) -> Sample {
    Sample {
        observation_id: record.observation_id,
        floor_id: record.floor_id,
        frame_id: record.frame_id,
        position: kyberia_spatial_analysis::Point2 {
            x: record.position.x,
            y: record.position.y,
        },
        value: Evidence::Known(record.rssi_dbm),
        position_covariance: record.position_covariance.clone(),
    }
}

fn validate_request(request: &SelectionRequest) -> Result<(), SelectionError> {
    if request.observation_ids.is_empty() {
        return Err(SelectionError::InvalidRequest("at least one observation"));
    }
    if request.observation_ids.len() > MAX_OBSERVATIONS {
        return Err(SelectionError::ResourceLimit("observation IDs"));
    }
    Ok(())
}

fn validate_metric(
    metric: &kyberia_spatial_analysis::MetricDefinitionBinding,
    config: SpatialConfig,
) -> Result<(), SelectionError> {
    config
        .validate()
        .map_err(|_| SelectionError::InvalidRequest("invalid spatial configuration"))?;
    let canonical_rssi = canonical_observed_rssi(config)
        .map_err(|_| SelectionError::InvalidRequest("canonical RSSI metric unavailable"))?;
    if metric.definition() != &canonical_rssi
        || metric.spatial_method() != Some(config_method(config))
    {
        return Err(SelectionError::InvalidRequest(
            "metric is not canonical observed RSSI",
        ));
    }
    Ok(())
}

fn canonical_observed_rssi(
    config: SpatialConfig,
) -> Result<
    kyberia_spatial_analysis::MetricDefinition,
    kyberia_spatial_analysis::MetricDefinitionError,
> {
    kyberia_spatial_analysis::MetricDefinition::observed_rssi(config_method(config))
}

fn metric_reference_matches(
    reference: &MetricReference,
    expected: &kyberia_spatial_analysis::MetricDefinition,
) -> Result<bool, SelectionError> {
    let bytes = expected
        .canonical_bytes()
        .map_err(|_| SelectionError::InvalidManifest("canonical RSSI metric unavailable"))?;
    let expected_hash = ContentHash::from_sha256(Sha256::digest(&bytes).into());
    Ok(reference.definition_hash == expected_hash
        && reference.artifact.sha256 == expected_hash
        && reference.artifact.version == *expected.version()
        && reference.artifact.byte_length.get() == bytes.len() as u64
        && reference.artifact.media_type.as_str()
            == kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE
        && reference.spatial_method == expected.spatial_method()
        && reference.signal_aggregation == expected.signal_aggregation())
}

fn config_method(config: SpatialConfig) -> kyberia_spatial_analysis::SpatialMethod {
    match config.method {
        kyberia_spatial_analysis::Method::PointValue => {
            kyberia_spatial_analysis::SpatialMethod::PointValue
        }
        kyberia_spatial_analysis::Method::Nearest => {
            kyberia_spatial_analysis::SpatialMethod::Nearest
        }
        kyberia_spatial_analysis::Method::Idw { .. } => {
            kyberia_spatial_analysis::SpatialMethod::InverseDistanceWeighted
        }
    }
}

fn unusable_quality(flag: &QualityFlag) -> bool {
    matches!(
        flag,
        QualityFlag::SyntheticFixture
            | QualityFlag::Stale
            | QualityFlag::ContradictorySourceFields
            | QualityFlag::Malformed
            | QualityFlag::Saturated
            | QualityFlag::InferredTimestamp
            | QualityFlag::ClockUncertain
            | QualityFlag::DroppedEvents
            | QualityFlag::Disconnected
            | QualityFlag::PartialCapture
    )
}

fn calibration_allowed(calibration: &Evidence<CalibrationState>, allow_uncalibrated: bool) -> bool {
    match calibration {
        Evidence::Known(CalibrationState::Reference { .. }) => true,
        Evidence::Known(CalibrationState::Uncalibrated) | Evidence::Unknown(_)
            if allow_uncalibrated =>
        {
            true
        }
        Evidence::Known(CalibrationState::Uncalibrated) | Evidence::Unknown(_) => false,
        Evidence::Known(CalibrationState::OutsideValidRange { .. }) => false,
    }
}

fn canonical_manifest(manifest: &SelectionManifest) -> Result<Vec<u8>, SelectionError> {
    let bytes = serde_json::to_vec(manifest).map_err(|_| SelectionError::Serialization)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(SelectionError::ResourceLimit("selection manifest bytes"));
    }
    Ok(bytes)
}

fn validate_json_bounds(bytes: &[u8]) -> Result<(), SelectionError> {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_MANIFEST_DEPTH {
                    return Err(SelectionError::ResourceLimit("selection manifest depth"));
                }
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Err(SelectionError::InvalidManifest("malformed JSON"));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    if in_string || escaped || depth != 0 {
        return Err(SelectionError::InvalidManifest("malformed JSON"));
    }
    Ok(())
}

fn artifact_reference(bytes: &[u8]) -> Result<ArtifactReference, SelectionError> {
    Ok(ArtifactReference {
        sha256: ContentHash::from_sha256(Sha256::digest(bytes).into()),
        media_type: kyberia_domain::identity::Text::new(SELECTION_MEDIA_TYPE)
            .map_err(|_| SelectionError::Serialization)?,
        byte_length: bytes.len() as u64,
    })
}

fn hash_serialized<T: Serialize>(value: &T) -> Result<ContentHash, SelectionError> {
    let bytes = serde_json::to_vec(value).map_err(|_| SelectionError::Serialization)?;
    Ok(ContentHash::from_sha256(Sha256::digest(bytes).into()))
}

#[cfg(test)]
mod tests;
