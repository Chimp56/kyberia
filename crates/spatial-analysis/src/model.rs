use crate::*;
use kyberia_domain::{
    analysis::{AnalysisManifest, VersionedArtifact},
    evidence::{ArtifactReference, UnknownReason},
    identity::{FloorId, FrameId, ObservationId, Text},
    units::{Db, Meters, Probability},
};
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;

pub const SIGNAL_METRIC_DEFINITION_SCHEMA: &str = "kyberia.signal-metric-definition/1";
pub const SIGNAL_METRIC_DEFINITION_MEDIA_TYPE: &str =
    "application/kyberia-signal-metric-definition+json";
pub const MAX_SIGNAL_METRIC_DEFINITION_BYTES: usize = 16 * 1024;
pub const MAX_SIGNAL_METRIC_DEFINITION_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Method {
    Nearest,
    Idw { power: f64 },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub enum Extrapolation {
    #[default]
    Disabled,
    WithinRadius(Meters),
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub method: Method,
    /// Model policy, not a measured confidence bound or convex hull.
    pub support_radius: Meters,
    pub minimum_locations: usize,
    pub maximum_neighbors: usize,
    pub extrapolation: Extrapolation,
}
impl Config {
    pub fn validate(self) -> Result<(), Error> {
        if self.support_radius.get() <= 0.0 {
            return Err(Error::InvalidConfiguration(
                "positive support radius required",
            ));
        }
        if self.minimum_locations == 0
            || self.maximum_neighbors < self.minimum_locations
            || self.maximum_neighbors > MAX_NEIGHBORS
        {
            return Err(Error::InvalidConfiguration("neighbor limits"));
        }
        if let Method::Idw { power } = self.method
            && (!power.is_finite() || power <= 0.0 || power > 64.0)
        {
            return Err(Error::InvalidConfiguration("IDW power must be in (0, 64]"));
        }
        if let Extrapolation::WithinRadius(radius) = self.extrapolation
            && radius < self.support_radius
        {
            return Err(Error::InvalidConfiguration(
                "extrapolation radius below support radius",
            ));
        }
        Ok(())
    }
}

/// A typed/versioned signal aggregation choice retained with the metric
/// definition. A verified metric-definition binding owns its artifact and
/// cannot be constructed with a divergent selection.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalAggregationSelection {
    pub algorithm_version: SignalAlgorithmVersion,
    pub method: AggregateMethod,
}
impl SignalAggregationSelection {
    pub const fn new(method: AggregateMethod) -> Self {
        Self {
            algorithm_version: kyberia_wifi_semantics::ALGORITHM_VERSION,
            method,
        }
    }

    pub fn validate(self) -> Result<(), Error> {
        if self.algorithm_version != kyberia_wifi_semantics::ALGORITHM_VERSION {
            return Err(Error::AggregationVersionMismatch);
        }
        kyberia_wifi_semantics::aggregate(&[], self.method)
            .map(|_| ())
            .map_err(map_aggregation_error)
    }

    pub fn validate_for_spatial(self) -> Result<(), Error> {
        self.validate()?;
        kyberia_wifi_semantics::aggregate_static(&[], self.method)
            .map(|_| ())
            .map_err(map_aggregation_error)
    }

    pub const fn is_temporal(self) -> bool {
        matches!(
            self.method,
            AggregateMethod::EwmaDbm { .. } | AggregateMethod::RobustStateSpaceDbm { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetricDefinitionError {
    ResourceLimit(&'static str),
    MalformedBytes,
    NonCanonicalBytes,
    ArtifactLengthMismatch,
    ArtifactHashMismatch,
    ArtifactVersionMismatch,
    ArtifactMediaTypeMismatch,
    SelectionMismatch,
    AggregationVersionMismatch,
    InvalidAggregationConfiguration(&'static str),
    TemporalAggregationRequiresMonotonicEvidence,
}
impl std::fmt::Display for MetricDefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for MetricDefinitionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum SignalMetricDefinitionSchema {
    #[serde(rename = "kyberia.signal-metric-definition/1")]
    V1,
}

/// Canonical signal metric-definition document used to create verified
/// bindings. Its fields stay private so its exact wire projection is produced
/// only by this type's canonical serializer.
#[derive(Clone, Debug, PartialEq)]
pub struct SignalMetricDefinition {
    version: Text,
    signal_aggregation: SignalAggregationSelection,
}
impl SignalMetricDefinition {
    pub fn new(
        version: Text,
        signal_aggregation: SignalAggregationSelection,
    ) -> Result<Self, MetricDefinitionError> {
        validate_signal_selection(signal_aggregation)?;
        Ok(Self {
            version,
            signal_aggregation,
        })
    }

    pub fn version(&self) -> &Text {
        &self.version
    }

    pub const fn signal_aggregation(&self) -> SignalAggregationSelection {
        self.signal_aggregation
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MetricDefinitionError> {
        let document = SignalMetricDefinitionDocument {
            schema: SignalMetricDefinitionSchema::V1,
            version: self.version.clone(),
            signal_aggregation: self.signal_aggregation,
        };
        let bytes =
            serde_json::to_vec(&document).map_err(|_| MetricDefinitionError::MalformedBytes)?;
        if bytes.len() > MAX_SIGNAL_METRIC_DEFINITION_BYTES {
            return Err(MetricDefinitionError::ResourceLimit(
                "signal metric definition bytes",
            ));
        }
        Ok(bytes)
    }

    pub fn bind(
        &self,
        artifact: VersionedArtifact,
    ) -> Result<MetricDefinitionBinding, MetricDefinitionError> {
        let bytes = self.canonical_bytes()?;
        MetricDefinitionBinding::from_artifact_bytes(artifact, &bytes, self.signal_aggregation)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignalMetricDefinitionDocument {
    schema: SignalMetricDefinitionSchema,
    version: Text,
    signal_aggregation: SignalAggregationSelection,
}

/// The metric-definition artifact and its verified typed projection used by
/// this spatial job. Private fields prevent arbitrary artifact/selection
/// pairings; construct it with `from_artifact_bytes` or `bind`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MetricDefinitionBinding {
    artifact: VersionedArtifact,
    signal_aggregation: SignalAggregationSelection,
}
impl MetricDefinitionBinding {
    /// Verify an untrusted artifact and bind only the selection requested by
    /// the caller. The selection is read from the canonical bytes and compared
    /// before the private binding is returned.
    pub fn from_artifact_bytes(
        artifact: VersionedArtifact,
        bytes: &[u8],
        expected_signal_aggregation: SignalAggregationSelection,
    ) -> Result<Self, MetricDefinitionError> {
        if bytes.len() > MAX_SIGNAL_METRIC_DEFINITION_BYTES {
            return Err(MetricDefinitionError::ResourceLimit(
                "signal metric definition bytes",
            ));
        }
        if artifact.byte_length.get() != bytes.len() as u64 {
            return Err(MetricDefinitionError::ArtifactLengthMismatch);
        }
        if !AnalysisManifest::verify_artifact(&artifact, bytes) {
            return Err(MetricDefinitionError::ArtifactHashMismatch);
        }
        if artifact.media_type.as_str() != SIGNAL_METRIC_DEFINITION_MEDIA_TYPE {
            return Err(MetricDefinitionError::ArtifactMediaTypeMismatch);
        }
        validate_json_bounds(bytes)?;
        let document: SignalMetricDefinitionDocument =
            serde_json::from_slice(bytes).map_err(|_| MetricDefinitionError::MalformedBytes)?;
        let canonical =
            serde_json::to_vec(&document).map_err(|_| MetricDefinitionError::MalformedBytes)?;
        if canonical != bytes {
            return Err(MetricDefinitionError::NonCanonicalBytes);
        }
        if document.version != artifact.version {
            return Err(MetricDefinitionError::ArtifactVersionMismatch);
        }
        validate_signal_selection(document.signal_aggregation)?;
        if document.signal_aggregation != expected_signal_aggregation {
            return Err(MetricDefinitionError::SelectionMismatch);
        }
        Ok(Self {
            artifact,
            signal_aggregation: document.signal_aggregation,
        })
    }

    pub fn artifact(&self) -> &VersionedArtifact {
        &self.artifact
    }

    pub const fn signal_aggregation(&self) -> SignalAggregationSelection {
        self.signal_aggregation
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MetricDefinitionError> {
        SignalMetricDefinition {
            version: self.artifact.version.clone(),
            signal_aggregation: self.signal_aggregation,
        }
        .canonical_bytes()
    }
}

fn validate_signal_selection(
    selection: SignalAggregationSelection,
) -> Result<(), MetricDefinitionError> {
    selection.validate().map_err(|error| match error {
        Error::AggregationVersionMismatch => MetricDefinitionError::AggregationVersionMismatch,
        Error::TemporalAggregationRequiresMonotonicEvidence => {
            MetricDefinitionError::TemporalAggregationRequiresMonotonicEvidence
        }
        Error::InvalidAggregationConfiguration(reason) => {
            MetricDefinitionError::InvalidAggregationConfiguration(reason)
        }
        Error::ResourceLimit(reason) => MetricDefinitionError::ResourceLimit(reason),
        _ => MetricDefinitionError::InvalidAggregationConfiguration(
            "unexpected signal aggregation error",
        ),
    })
}

fn validate_json_bounds(bytes: &[u8]) -> Result<(), MetricDefinitionError> {
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
                if depth > MAX_SIGNAL_METRIC_DEFINITION_DEPTH {
                    return Err(MetricDefinitionError::ResourceLimit(
                        "signal metric definition depth",
                    ));
                }
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Err(MetricDefinitionError::MalformedBytes);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    if in_string || escaped || depth != 0 {
        return Err(MetricDefinitionError::MalformedBytes);
    }
    Ok(())
}

/// A group is one exact coordinate, not one independent statistical sample.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LocationGroup {
    pub position: Point2,
    pub observation_ids: Vec<ObservationId>,
    /// The exact aggregate, including method/version and ordered IDs, is
    /// retained alongside the convenient spatial location index.
    pub signal_aggregate: SignalAggregate,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Inputs {
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub evidence_plane: InputEvidencePlane,
    /// Pins filtering, transmitter selection, aggregation and assignment
    /// versions. Construct it with the binding's validating constructor.
    pub metric_definition: MetricDefinitionBinding,
    /// Caller must verify that this immutable artifact contains these samples.
    pub source_artifact: ArtifactReference,
    pub samples: Vec<Sample>,
}
/// Synthetic fixtures remain visibly synthetic even at exact sample coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputEvidencePlane {
    Measured,
    Synthetic,
}
#[derive(Clone, Debug)]
pub struct Model {
    pub(crate) inputs: Inputs,
    pub(crate) config: Config,
    pub(crate) groups: Vec<LocationGroup>,
}
impl Model {
    pub fn new(mut inputs: Inputs, config: Config) -> Result<Self, Error> {
        config.validate()?;
        let signal_aggregation = inputs.metric_definition.signal_aggregation();
        signal_aggregation.validate_for_spatial()?;
        if inputs.samples.len() > MAX_SAMPLES {
            return Err(Error::ResourceLimit("samples"));
        }
        inputs.samples.sort_by_key(|s| s.observation_id);
        for (i, sample) in inputs.samples.iter().enumerate() {
            if sample.floor_id != inputs.floor_id {
                return Err(Error::FloorMismatch);
            }
            if sample.frame_id != inputs.frame_id {
                return Err(Error::FrameMismatch);
            }
            if i > 0 && inputs.samples[i - 1].observation_id == sample.observation_id {
                return Err(Error::DuplicateObservation(sample.observation_id));
            }
        }
        let mut known: Vec<_> = inputs
            .samples
            .iter()
            .filter(|s| s.value.as_known().is_some())
            .collect();
        known.sort_by(|a, b| {
            a.position
                .x
                .get()
                .total_cmp(&b.position.x.get())
                .then(a.position.y.get().total_cmp(&b.position.y.get()))
                .then(a.observation_id.cmp(&b.observation_id))
        });
        let mut groups = Vec::new();
        let mut start = 0;
        while start < known.len() {
            let mut end = start + 1;
            while end < known.len() && known[end].position == known[start].position {
                end += 1;
            }
            let values: Vec<_> = known[start..end]
                .iter()
                .map(|s| kyberia_wifi_semantics::StaticSignalSample {
                    observation_id: s.observation_id,
                    rssi: *s.value.as_known().unwrap(),
                })
                .collect();
            let signal_aggregate =
                kyberia_wifi_semantics::aggregate_static(&values, signal_aggregation.method)
                    .map_err(map_aggregation_error)?;
            groups.push(LocationGroup {
                position: known[start].position,
                observation_ids: known[start..end].iter().map(|s| s.observation_id).collect(),
                signal_aggregate,
            });
            start = end;
        }
        Ok(Self {
            inputs,
            config,
            groups,
        })
    }
    pub fn groups(&self) -> &[LocationGroup] {
        &self.groups
    }
    pub fn inputs(&self) -> &Inputs {
        &self.inputs
    }

    pub fn estimate(
        &self,
        point: Point2,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Cell, Error> {
        let mut nearest = BinaryHeap::new();
        let mut minimum_distance = f64::INFINITY;
        let mut support_locations = 0;
        let mut support_observations = 0;
        let mut exact = None;
        let limit = match self.config.extrapolation {
            Extrapolation::Disabled => self.config.support_radius,
            Extrapolation::WithinRadius(radius) => radius,
        }
        .get();
        for (index, group) in self.groups.iter().enumerate() {
            if index % 64 == 0 && cancelled() {
                return Err(Error::Cancelled);
            }
            // Overflow indicates a distance larger than every finite radius.
            let distance = (point.x.get() - group.position.x.get())
                .hypot(point.y.get() - group.position.y.get());
            minimum_distance = minimum_distance.min(distance);
            if distance == 0.0 {
                exact = Some(index);
            }
            if distance <= self.config.support_radius.get() {
                support_locations += 1;
                support_observations += group.observation_ids.len();
            }
            if distance.is_finite() && distance <= limit {
                nearest.push(Neighbor { distance, index });
                if nearest.len() > self.config.maximum_neighbors {
                    nearest.pop();
                }
            }
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let nearest_distance = if minimum_distance.is_finite() {
            Evidence::Known(
                Meters::new(minimum_distance).map_err(|_| Error::NumericalFailure("distance"))?,
            )
        } else {
            Evidence::Unknown(if self.groups.is_empty() {
                UnknownReason::NotMeasured
            } else {
                UnknownReason::InvalidGeometry
            })
        };
        let mut cell = Cell {
            value: Evidence::Unknown(if self.groups.is_empty() {
                UnknownReason::NotMeasured
            } else {
                UnknownReason::OutsideEvidenceSupport
            }),
            class: CellClass::Unknown,
            support_locations,
            support_observations,
            nearest_distance,
            uncertainty_db: Evidence::Unknown(UnknownReason::NotMeasured),
            contributors: Vec::new(),
        };
        if let Some(index) = exact {
            cell.value = Evidence::Known(
                *self.groups[index]
                    .signal_aggregate
                    .estimate
                    .as_known()
                    .ok_or(Error::NumericalFailure("coincident aggregate"))?,
            );
            cell.class = CellClass::Observed;
            cell.contributors.push(Contribution {
                location_group: index,
                weight: Probability::new(1.0).unwrap(),
            });
            return Ok(cell);
        }
        let supported = support_locations >= self.config.minimum_locations;
        if !supported && matches!(self.config.extrapolation, Extrapolation::Disabled) {
            return Ok(cell);
        }
        let mut neighbors = nearest.into_sorted_vec();
        // Supported estimates use only supported neighbors even when extrapolation is enabled.
        if supported {
            neighbors.retain(|n| n.distance <= self.config.support_radius.get());
        }
        if neighbors.len() < self.config.minimum_locations {
            return Ok(cell);
        }
        if matches!(self.config.method, Method::Nearest) {
            neighbors.truncate(1);
        }
        let d0 = neighbors[0].distance;
        let weights: Vec<_> = neighbors
            .iter()
            .map(|n| match self.config.method {
                Method::Nearest => 1.0,
                Method::Idw { power } => (d0 / n.distance).powf(power),
            })
            .collect();
        let sum: f64 = weights.iter().sum();
        let value = convex_mean(neighbors.iter().zip(&weights).map(|(n, w)| {
            (
                self.groups[n.index]
                    .signal_aggregate
                    .estimate
                    .as_known()
                    .expect("validated coincident aggregate")
                    .get(),
                *w,
            )
        }))?;
        cell.value = Evidence::Known(
            kyberia_domain::units::Dbm::new(value)
                .map_err(|_| Error::NumericalFailure("IDW result"))?,
        );
        cell.class = if supported {
            CellClass::Interpolated
        } else {
            CellClass::Extrapolated
        };
        cell.contributors = neighbors
            .iter()
            .zip(weights)
            .filter(|(_, w)| *w > 0.0)
            .map(|(n, w)| Contribution {
                location_group: n.index,
                weight: Probability::new(w / sum).unwrap(),
            })
            .collect();
        Ok(cell)
    }
}

fn map_aggregation_error(error: kyberia_wifi_semantics::Error) -> Error {
    match error {
        kyberia_wifi_semantics::Error::InvalidConfiguration(reason) => {
            Error::InvalidAggregationConfiguration(reason)
        }
        kyberia_wifi_semantics::Error::DuplicateObservation(observation_id) => {
            Error::DuplicateObservation(observation_id)
        }
        kyberia_wifi_semantics::Error::ClockEpochMismatch => {
            Error::InvalidAggregationConfiguration("clock epoch mismatch in spatial aggregation")
        }
        kyberia_wifi_semantics::Error::NonMonotonicSequence => {
            Error::InvalidAggregationConfiguration("non-monotonic sequence in spatial aggregation")
        }
        kyberia_wifi_semantics::Error::TemporalMethodRequiresMonotonicTime => {
            Error::TemporalAggregationRequiresMonotonicEvidence
        }
        kyberia_wifi_semantics::Error::ResourceLimit => {
            Error::ResourceLimit("signal aggregation samples")
        }
        kyberia_wifi_semantics::Error::NumericalFailure => {
            Error::NumericalFailure("signal aggregation")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum CellClass {
    Observed,
    Interpolated,
    Extrapolated,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Contribution {
    pub location_group: usize,
    pub weight: Probability,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Cell {
    pub value: Evidence<kyberia_domain::units::Dbm>,
    pub class: CellClass,
    pub support_locations: usize,
    pub support_observations: usize,
    pub nearest_distance: Evidence<Meters>,
    /// No calibrated uncertainty is inferred from neighbor spread or density.
    pub uncertainty_db: Evidence<Db>,
    pub contributors: Vec<Contribution>,
}
#[derive(Clone, Copy, Debug)]
struct Neighbor {
    distance: f64,
    index: usize,
}
impl PartialEq for Neighbor {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Neighbor {}
impl PartialOrd for Neighbor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Neighbor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.index.cmp(&other.index))
    }
}

/// Scaled compensated weighted mean avoids overflow, including opposite extrema.
fn convex_mean(values: impl Iterator<Item = (f64, f64)> + Clone) -> Result<f64, Error> {
    let scale = values.clone().fold(0.0_f64, |a, (v, _)| a.max(v.abs()));
    if scale == 0.0 {
        return Ok(0.0);
    }
    let mut numerator = 0.0;
    let mut correction = 0.0;
    let mut denominator = 0.0;
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for (value, weight) in values {
        minimum = minimum.min(value);
        maximum = maximum.max(value);
        let adjusted = (value / scale) * weight - correction;
        let next = numerator + adjusted;
        correction = (next - numerator) - adjusted;
        numerator = next;
        denominator += weight;
    }
    let value = (numerator / denominator).clamp(-1.0, 1.0) * scale;
    if !value.is_finite() {
        return Err(Error::NumericalFailure("weighted mean"));
    }
    Ok(value.clamp(minimum, maximum))
}
