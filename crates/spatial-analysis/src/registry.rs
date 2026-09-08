//! Canonical, bounded metric definitions.
//!
//! A metric definition is the semantic contract shared by computation,
//! reports, and presentation.  It is deliberately a closed Rust model rather
//! than an unbounded JSON value.  The wire document is authenticated by its
//! canonical bytes and content hash before it can be used by a numerical job.

use kyberia_domain::{
    analysis::{AnalysisManifest, VersionedArtifact},
    capability::Capability,
    evidence::EvidenceClass,
    identity::{ContentHash, Text},
};
use kyberia_wifi_semantics::{AggregateMethod, SignalAlgorithmVersion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const METRIC_DEFINITION_SCHEMA: &str = "kyberia.metric-definition/1";
pub const METRIC_DEFINITION_MEDIA_TYPE: &str = "application/kyberia-metric-definition+json";
pub const MAX_METRIC_DEFINITION_BYTES: usize = 16 * 1024;
pub const MAX_METRIC_DEFINITION_DEPTH: usize = 16;
pub const MAX_METRIC_DEFINITIONS: usize = 128;
pub const MAX_SELECTION_DIMENSIONS: usize = 16;
pub const MAX_CAPABILITY_ALTERNATIVES: usize = 16;

/// Legacy signal artifact constants retained for existing project bundles.
/// New definitions use the current metric registry constants above.
pub const SIGNAL_METRIC_DEFINITION_SCHEMA: &str = "kyberia.signal-metric-definition/1";
pub const SIGNAL_METRIC_DEFINITION_MEDIA_TYPE: &str =
    "application/kyberia-signal-metric-definition+json";
pub const MAX_SIGNAL_METRIC_DEFINITION_BYTES: usize = 16 * 1024;
pub const MAX_SIGNAL_METRIC_DEFINITION_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetricId(Text);
impl MetricId {
    pub fn new(value: impl Into<String>) -> Result<Self, MetricDefinitionError> {
        let text = Text::new(value).map_err(|_| MetricDefinitionError::InvalidIdentifier)?;
        let bytes = text.as_str().as_bytes();
        if bytes.len() > 128
            || bytes.first().is_none_or(|b| !b.is_ascii_alphanumeric())
            || bytes.last().is_none_or(|b| !b.is_ascii_alphanumeric())
            || text.as_str().split('/').any(str::is_empty)
            || !bytes.iter().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'.' | b'_' | b'-' | b'/')
            })
        {
            return Err(MetricDefinitionError::InvalidIdentifier);
        }
        Ok(Self(text))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl Serialize for MetricId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for MetricId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MetricVersion(u16);
impl MetricVersion {
    pub const fn new(value: u16) -> Result<Self, MetricDefinitionError> {
        if value == 0 {
            Err(MetricDefinitionError::InvalidVersion)
        } else {
            Ok(Self(value))
        }
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}
impl Serialize for MetricVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u16(self.0)
    }
}
impl<'de> Deserialize<'de> for MetricVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(u16::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalUnit {
    Dbm,
    Db,
    Mbps,
    Hertz,
    Meters,
    Seconds,
    Probability,
    Dimensionless,
    Categorical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum ValidRange {
    Numeric { minimum: f64, maximum: f64 },
    Categorical { values: Vec<Text> },
}
impl ValidRange {
    fn validate(&self) -> Result<(), MetricDefinitionError> {
        match self {
            Self::Numeric { minimum, maximum }
                if minimum.is_finite() && maximum.is_finite() && minimum <= maximum =>
            {
                Ok(())
            }
            Self::Numeric { .. } => Err(MetricDefinitionError::InvalidRange),
            Self::Categorical { values } if !values.is_empty() && values.len() <= 64 => {
                if values.windows(2).any(|pair| pair[0] >= pair[1]) {
                    Err(MetricDefinitionError::InvalidRange)
                } else {
                    Ok(())
                }
            }
            Self::Categorical { .. } => Err(MetricDefinitionError::InvalidRange),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum CapabilityRequirement {
    AnyOf { capabilities: Vec<Capability> },
    AllOf { capabilities: Vec<Capability> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirements {
    pub evidence: Vec<EvidenceClass>,
    pub capabilities: Vec<CapabilityRequirement>,
}
impl EvidenceRequirements {
    fn validate(&self) -> Result<(), MetricDefinitionError> {
        if self.evidence.is_empty() || self.evidence.len() > 8 {
            return Err(MetricDefinitionError::ResourceLimit(
                "evidence requirements",
            ));
        }
        if self
            .evidence
            .iter()
            .enumerate()
            .any(|(index, class)| self.evidence[..index].contains(class))
        {
            return Err(MetricDefinitionError::InvalidConfiguration(
                "duplicate evidence class",
            ));
        }
        if self.capabilities.len() > MAX_CAPABILITY_ALTERNATIVES {
            return Err(MetricDefinitionError::ResourceLimit(
                "capability requirements",
            ));
        }
        for requirement in &self.capabilities {
            let values = match requirement {
                CapabilityRequirement::AnyOf { capabilities }
                | CapabilityRequirement::AllOf { capabilities } => capabilities,
            };
            if values.is_empty() || values.len() > 16 {
                return Err(MetricDefinitionError::ResourceLimit(
                    "capability alternatives",
                ));
            }
            let mut sorted = values.clone();
            sorted.sort();
            sorted.dedup();
            if sorted.len() != values.len() {
                return Err(MetricDefinitionError::InvalidConfiguration(
                    "duplicate capability",
                ));
            }
            if sorted != *values {
                return Err(MetricDefinitionError::InvalidConfiguration(
                    "capabilities must be sorted",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialMethod {
    PointValue,
    Nearest,
    InverseDistanceWeighted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionDimension {
    Survey,
    TimeWindow,
    Sensor,
    Adapter,
    Network,
    AccessPoint,
    Radio,
    Band,
    Channel,
    Client,
    EvidenceClass,
    Floor,
    Zone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionPolicy {
    pub filters: Vec<SelectionDimension>,
    pub grouping: Vec<SelectionDimension>,
}
impl SelectionPolicy {
    fn validate(&self) -> Result<(), MetricDefinitionError> {
        if self.filters.len() > MAX_SELECTION_DIMENSIONS
            || self.grouping.len() > MAX_SELECTION_DIMENSIONS
        {
            return Err(MetricDefinitionError::ResourceLimit("selection dimensions"));
        }
        for values in [&self.filters, &self.grouping] {
            let mut sorted = values.clone();
            sorted.sort();
            sorted.dedup();
            if sorted.len() != values.len() {
                return Err(MetricDefinitionError::InvalidConfiguration(
                    "duplicate selection dimension",
                ));
            }
            if sorted != *values {
                return Err(MetricDefinitionError::InvalidConfiguration(
                    "selection dimensions must be sorted",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UncertaintyMethod {
    NotReported,
    EmpiricalSpread,
    CalibratedModel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum Compatibility {
    StableWithinVersion,
    ExactEvidenceContract,
    Unknown { reason: Text },
}

/// Display hints are deliberately a separate nonsemantic structure. Changing
/// them must not alter computed values; they remain in the artifact so reports
/// can reproduce the complete presentation contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualizationDefaults {
    pub palette: Palette,
    pub display_precision: u8,
}
impl VisualizationDefaults {
    fn validate(self) -> Result<(), MetricDefinitionError> {
        if self.display_precision > 8 {
            Err(MetricDefinitionError::InvalidConfiguration(
                "visualization precision",
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    Sequential,
    Diverging,
    Categorical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceDirection {
    HigherIsBetter,
    LowerIsBetter,
    Categorical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum AggregationDefinition {
    SignalDbm {
        selection: SignalAggregationSelection,
    },
}
impl AggregationDefinition {
    fn validate(&self) -> Result<(), MetricDefinitionError> {
        match self {
            Self::SignalDbm { selection } => validate_signal_selection(*selection),
        }
    }
    fn signal_selection(&self) -> SignalAggregationSelection {
        match self {
            Self::SignalDbm { selection } => *selection,
        }
    }
}

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
    pub fn validate(self) -> Result<(), crate::Error> {
        if self.algorithm_version != kyberia_wifi_semantics::ALGORITHM_VERSION {
            return Err(crate::Error::AggregationVersionMismatch);
        }
        kyberia_wifi_semantics::aggregate(&[], self.method)
            .map(|_| ())
            .map_err(map_signal_error)
    }
    pub fn validate_for_spatial(self) -> Result<(), crate::Error> {
        self.validate()?;
        kyberia_wifi_semantics::aggregate_static(&[], self.method)
            .map(|_| ())
            .map_err(map_signal_error)
    }
    pub const fn is_temporal(self) -> bool {
        matches!(
            self.method,
            AggregateMethod::EwmaDbm { .. } | AggregateMethod::RobustStateSpaceDbm { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricDefinitionSpec {
    pub id: MetricId,
    pub version: MetricVersion,
    pub semantic_description: Text,
    pub unit: PhysicalUnit,
    pub valid_range: ValidRange,
    pub evidence_requirements: EvidenceRequirements,
    pub aggregation: AggregationDefinition,
    pub spatial_method: SpatialMethod,
    pub selection: SelectionPolicy,
    pub uncertainty: UncertaintyMethod,
    pub unknown_compatibility: UnknownCompatibility,
    pub compatibility: Compatibility,
    pub visualization: VisualizationDefaults,
    pub compliance: ComplianceDirection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricDefinition {
    spec: MetricDefinitionSpec,
    artifact_version: Text,
    canonical_override: Option<Vec<u8>>,
}

/// The original signal-only artifact API and wire contract.
///
/// This type remains separate from the complete metric registry model. The
/// binding layer projects it into the current complete model for computation.
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
        let document = LegacySignalMetricDefinitionDocument {
            schema: LegacySignalMetricDefinitionSchema::V1,
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

    fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, MetricDefinitionError> {
        if bytes.len() > MAX_SIGNAL_METRIC_DEFINITION_BYTES {
            return Err(MetricDefinitionError::ResourceLimit(
                "signal metric definition bytes",
            ));
        }
        if let Err(MetricDefinitionError::ResourceLimit("metric definition depth")) =
            validate_json_bounds(bytes)
        {
            return Err(MetricDefinitionError::ResourceLimit(
                "signal metric definition depth",
            ));
        }
        let document: LegacySignalMetricDefinitionDocument =
            serde_json::from_slice(bytes).map_err(|_| MetricDefinitionError::MalformedBytes)?;
        let canonical =
            serde_json::to_vec(&document).map_err(|_| MetricDefinitionError::MalformedBytes)?;
        if canonical != bytes {
            return Err(MetricDefinitionError::NonCanonicalBytes);
        }
        validate_signal_selection(document.signal_aggregation)?;
        Ok(Self {
            version: document.version,
            signal_aggregation: document.signal_aggregation,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum LegacySignalMetricDefinitionSchema {
    #[serde(rename = "kyberia.signal-metric-definition/1")]
    V1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySignalMetricDefinitionDocument {
    schema: LegacySignalMetricDefinitionSchema,
    version: Text,
    signal_aggregation: SignalAggregationSelection,
}

impl Serialize for MetricDefinition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        MetricDefinitionDocument::from_spec(&self.spec).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for MetricDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        MetricDefinitionDocument::deserialize(deserializer)
            .and_then(|document| document.into_spec().map_err(serde::de::Error::custom))
            .and_then(|spec| Self::from_spec(spec).map_err(serde::de::Error::custom))
    }
}

impl MetricDefinition {
    pub fn from_spec(spec: MetricDefinitionSpec) -> Result<Self, MetricDefinitionError> {
        validate_spec(&spec)?;
        let artifact_version = artifact_version(&spec.id, spec.version)?;
        Ok(Self {
            spec,
            artifact_version,
            canonical_override: None,
        })
    }

    /// Construct a point-value RSSI definition.
    ///
    /// This is the original public constructor and remains pinned to
    /// `wifi.rssi/1`.  Its canonical bytes are an existing project artifact;
    /// changing this definition would silently change the meaning of stored
    /// point-value analyses.
    pub fn new(
        version_label: Text,
        signal_aggregation: SignalAggregationSelection,
    ) -> Result<Self, MetricDefinitionError> {
        let (id, version) = parse_artifact_version(&version_label)?;
        Self::from_spec(signal_metric_spec(id, version, signal_aggregation)?)
    }

    fn from_legacy(
        version_label: Text,
        signal_aggregation: SignalAggregationSelection,
        canonical_bytes: Vec<u8>,
    ) -> Result<Self, MetricDefinitionError> {
        let (id, version) = parse_artifact_version(&version_label)
            .unwrap_or((MetricId::new("legacy.signal-rssi")?, MetricVersion::new(1)?));
        let mut definition = Self::from_spec(signal_metric_spec(id, version, signal_aggregation)?)?;
        definition.artifact_version = version_label;
        definition.canonical_override = Some(canonical_bytes);
        Ok(definition)
    }

    pub fn signal_rssi() -> Result<Self, MetricDefinitionError> {
        Self::observed_rssi(SpatialMethod::PointValue)
    }

    /// Construct the canonical observed-RSSI definition for one spatial
    /// method.  The spatial method is part of the metric identity because it
    /// changes the meaning of every non-exact cell in a derived layer.
    pub fn observed_rssi(method: SpatialMethod) -> Result<Self, MetricDefinitionError> {
        let selection = SignalAggregationSelection::new(AggregateMethod::MedianDbm);
        let (version_label, description) = match method {
            SpatialMethod::PointValue => (
                "wifi.rssi/1",
                "Received Wi-Fi signal power at the observation point",
            ),
            SpatialMethod::Nearest => (
                "wifi.rssi.nearest/1",
                "Nearest supported observed Wi-Fi signal power",
            ),
            SpatialMethod::InverseDistanceWeighted => (
                "wifi.rssi.idw/1",
                "Inverse-distance weighted observed Wi-Fi signal power",
            ),
        };
        let (id, version) = parse_artifact_version(
            &Text::new(version_label).expect("valid builtin metric version"),
        )?;
        Self::from_spec(signal_metric_spec_with_method(
            id,
            version,
            selection,
            method,
            Text::new(description).expect("valid builtin metric description"),
        )?)
    }

    pub fn signal_rssi_nearest() -> Result<Self, MetricDefinitionError> {
        Self::observed_rssi(SpatialMethod::Nearest)
    }

    pub fn signal_rssi_idw() -> Result<Self, MetricDefinitionError> {
        Self::observed_rssi(SpatialMethod::InverseDistanceWeighted)
    }

    pub fn id(&self) -> &MetricId {
        &self.spec.id
    }
    pub const fn revision(&self) -> MetricVersion {
        self.spec.version
    }
    /// Combined `id/version` label retained for VersionedArtifact compatibility.
    pub fn version(&self) -> &Text {
        &self.artifact_version
    }
    pub fn semantic_description(&self) -> &Text {
        &self.spec.semantic_description
    }
    pub const fn unit(&self) -> PhysicalUnit {
        self.spec.unit
    }
    pub fn valid_range(&self) -> &ValidRange {
        &self.spec.valid_range
    }
    pub fn evidence_requirements(&self) -> &EvidenceRequirements {
        &self.spec.evidence_requirements
    }
    pub fn aggregation(&self) -> &AggregationDefinition {
        &self.spec.aggregation
    }
    pub const fn spatial_method(&self) -> SpatialMethod {
        self.spec.spatial_method
    }
    pub fn selection(&self) -> &SelectionPolicy {
        &self.spec.selection
    }
    pub const fn uncertainty(&self) -> UncertaintyMethod {
        self.spec.uncertainty
    }
    pub const fn unknown_compatibility(&self) -> UnknownCompatibility {
        self.spec.unknown_compatibility
    }
    pub fn compatibility(&self) -> &Compatibility {
        &self.spec.compatibility
    }
    pub const fn visualization(&self) -> VisualizationDefaults {
        self.spec.visualization
    }
    pub const fn compliance(&self) -> ComplianceDirection {
        self.spec.compliance
    }
    pub fn signal_aggregation(&self) -> SignalAggregationSelection {
        self.spec.aggregation.signal_selection()
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MetricDefinitionError> {
        if let Some(bytes) = &self.canonical_override {
            return Ok(bytes.clone());
        }
        let document = MetricDefinitionDocument::from_spec(&self.spec);
        let bytes =
            serde_json::to_vec(&document).map_err(|_| MetricDefinitionError::MalformedBytes)?;
        if bytes.len() > MAX_METRIC_DEFINITION_BYTES {
            return Err(MetricDefinitionError::ResourceLimit(
                "metric definition bytes",
            ));
        }
        Ok(bytes)
    }
    pub fn content_hash(&self) -> Result<ContentHash, MetricDefinitionError> {
        Ok(ContentHash::from_sha256(
            Sha256::digest(&self.canonical_bytes()?).into(),
        ))
    }
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, MetricDefinitionError> {
        if bytes.len() > MAX_METRIC_DEFINITION_BYTES {
            return Err(MetricDefinitionError::ResourceLimit(
                "metric definition bytes",
            ));
        }
        validate_json_bounds(bytes)?;
        let document: MetricDefinitionDocument =
            serde_json::from_slice(bytes).map_err(|_| MetricDefinitionError::MalformedBytes)?;
        let spec = document.into_spec()?;
        let definition = Self::from_spec(spec)?;
        if definition.canonical_bytes()? != bytes {
            return Err(MetricDefinitionError::NonCanonicalBytes);
        }
        Ok(definition)
    }

    /// Presentation material is tied to exactly the same hash used by compute.
    pub fn ui_help(&self) -> Result<MetricHelp, MetricDefinitionError> {
        Ok(MetricHelp {
            definition_hash: self.content_hash()?,
            id: self.id().clone(),
            description: self.semantic_description().clone(),
            unit: self.unit(),
            visualization: self.visualization(),
        })
    }
    /// Compute consumers receive the canonical identity with the typed contract.
    pub fn compute_contract(&self) -> Result<ComputeContract, MetricDefinitionError> {
        Ok(ComputeContract {
            definition_hash: self.content_hash()?,
            unit: self.unit(),
            valid_range: self.valid_range().clone(),
            aggregation: self.aggregation().clone(),
            spatial_method: self.spatial_method(),
            unknown: self.unknown_compatibility(),
        })
    }
    pub fn with_visualization(
        &self,
        visualization: VisualizationDefaults,
    ) -> Result<Self, MetricDefinitionError> {
        let mut spec = self.spec.clone();
        spec.visualization = visualization;
        Self::from_spec(spec)
    }
}

fn signal_metric_spec(
    id: MetricId,
    version: MetricVersion,
    signal_aggregation: SignalAggregationSelection,
) -> Result<MetricDefinitionSpec, MetricDefinitionError> {
    signal_metric_spec_with_method(
        id,
        version,
        signal_aggregation,
        SpatialMethod::PointValue,
        Text::new("Received Wi-Fi signal power at the observation point")
            .map_err(|_| MetricDefinitionError::InvalidConfiguration("description"))?,
    )
}

fn signal_metric_spec_with_method(
    id: MetricId,
    version: MetricVersion,
    signal_aggregation: SignalAggregationSelection,
    spatial_method: SpatialMethod,
    semantic_description: Text,
) -> Result<MetricDefinitionSpec, MetricDefinitionError> {
    Ok(MetricDefinitionSpec {
        id,
        version,
        semantic_description,
        unit: PhysicalUnit::Dbm,
        valid_range: ValidRange::Numeric {
            minimum: -200.0,
            maximum: 100.0,
        },
        evidence_requirements: EvidenceRequirements {
            evidence: vec![EvidenceClass::Observed],
            capabilities: vec![CapabilityRequirement::AnyOf {
                capabilities: vec![Capability::NearbyScan, Capability::MonitorFrames],
            }],
        },
        aggregation: AggregationDefinition::SignalDbm {
            selection: signal_aggregation,
        },
        spatial_method,
        selection: SelectionPolicy {
            filters: vec![SelectionDimension::Band, SelectionDimension::Channel],
            grouping: vec![SelectionDimension::AccessPoint],
        },
        uncertainty: UncertaintyMethod::NotReported,
        unknown_compatibility: UnknownCompatibility::PropagateReason,
        compatibility: Compatibility::ExactEvidenceContract,
        visualization: VisualizationDefaults {
            palette: Palette::Sequential,
            display_precision: 1,
        },
        compliance: ComplianceDirection::HigherIsBetter,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricHelp {
    pub definition_hash: ContentHash,
    pub id: MetricId,
    pub description: Text,
    pub unit: PhysicalUnit,
    pub visualization: VisualizationDefaults,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputeContract {
    pub definition_hash: ContentHash,
    pub unit: PhysicalUnit,
    pub valid_range: ValidRange,
    pub aggregation: AggregationDefinition,
    pub spatial_method: SpatialMethod,
    pub unknown: UnknownCompatibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownCompatibility {
    PropagateReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerOperation {
    Minimum,
    Difference,
    Rank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DimensionError {
    IncompatibleUnits,
    UnsupportedOperation(&'static str),
}
impl std::fmt::Display for DimensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DimensionError {}

/// Bounded dimensional primitives for the planned layer algebra. This is a
/// checker, not an expression evaluator or a generic user-supplied DSL.
pub fn check_layer_operation(
    operation: LayerOperation,
    left: PhysicalUnit,
    right: Option<PhysicalUnit>,
) -> Result<PhysicalUnit, DimensionError> {
    match operation {
        LayerOperation::Rank => Ok(PhysicalUnit::Dimensionless),
        LayerOperation::Minimum | LayerOperation::Difference => {
            let right = right.ok_or(DimensionError::UnsupportedOperation(
                "binary operation requires two metrics",
            ))?;
            if left != right {
                return Err(DimensionError::IncompatibleUnits);
            }
            if left == PhysicalUnit::Categorical {
                return Err(DimensionError::UnsupportedOperation(
                    "categorical arithmetic",
                ));
            }
            Ok(
                if operation == LayerOperation::Difference && left == PhysicalUnit::Dbm {
                    PhysicalUnit::Db
                } else {
                    left
                },
            )
        }
    }
}

/// Bounded registry with deterministic duplicate/version behavior.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricRegistry {
    definitions: BTreeMap<(MetricId, MetricVersion), MetricDefinition>,
}
impl MetricRegistry {
    pub fn new(definitions: Vec<MetricDefinition>) -> Result<Self, RegistryError> {
        if definitions.len() > MAX_METRIC_DEFINITIONS {
            return Err(RegistryError::ResourceLimit("metric definitions"));
        }
        let mut indexed = BTreeMap::new();
        for definition in definitions {
            let key = (definition.id().clone(), definition.revision());
            if indexed.insert(key, definition).is_some() {
                return Err(RegistryError::DuplicateVersion);
            }
        }
        Ok(Self {
            definitions: indexed,
        })
    }
    pub fn builtins() -> Result<Self, RegistryError> {
        Self::new(vec![
            MetricDefinition::signal_rssi()?,
            MetricDefinition::signal_rssi_nearest()?,
            MetricDefinition::signal_rssi_idw()?,
        ])
    }
    pub fn lookup(&self, id: &MetricId, version: MetricVersion) -> Option<&MetricDefinition> {
        self.definitions.get(&(id.clone(), version))
    }
    pub fn list(&self) -> impl Iterator<Item = &MetricDefinition> {
        self.definitions.values()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    ResourceLimit(&'static str),
    DuplicateVersion,
    InvalidDefinition(MetricDefinitionError),
}
impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RegistryError {}
impl From<MetricDefinitionError> for RegistryError {
    fn from(error: MetricDefinitionError) -> Self {
        Self::InvalidDefinition(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetricDefinitionError {
    ResourceLimit(&'static str),
    InvalidIdentifier,
    InvalidVersion,
    InvalidRange,
    MalformedBytes,
    NonCanonicalBytes,
    ArtifactLengthMismatch,
    ArtifactHashMismatch,
    ArtifactVersionMismatch,
    ArtifactMediaTypeMismatch,
    SelectionMismatch,
    AggregationVersionMismatch,
    InvalidAggregationConfiguration(&'static str),
    InvalidConfiguration(&'static str),
    TemporalAggregationRequiresMonotonicEvidence,
}
impl std::fmt::Display for MetricDefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for MetricDefinitionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum MetricDefinitionSchema {
    #[serde(rename = "kyberia.metric-definition/1")]
    V1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricDefinitionDocument {
    schema: MetricDefinitionSchema,
    id: MetricId,
    version: MetricVersion,
    semantic_description: Text,
    unit: PhysicalUnit,
    valid_range: ValidRange,
    evidence_requirements: EvidenceRequirements,
    aggregation: AggregationDefinition,
    spatial_method: SpatialMethod,
    selection: SelectionPolicy,
    uncertainty: UncertaintyMethod,
    unknown_compatibility: UnknownCompatibility,
    compatibility: Compatibility,
    visualization: VisualizationDefaults,
    compliance: ComplianceDirection,
}
impl MetricDefinitionDocument {
    fn from_spec(spec: &MetricDefinitionSpec) -> Self {
        Self {
            schema: MetricDefinitionSchema::V1,
            id: spec.id.clone(),
            version: spec.version,
            semantic_description: spec.semantic_description.clone(),
            unit: spec.unit,
            valid_range: spec.valid_range.clone(),
            evidence_requirements: spec.evidence_requirements.clone(),
            aggregation: spec.aggregation.clone(),
            spatial_method: spec.spatial_method,
            selection: spec.selection.clone(),
            uncertainty: spec.uncertainty,
            unknown_compatibility: spec.unknown_compatibility,
            compatibility: spec.compatibility.clone(),
            visualization: spec.visualization,
            compliance: spec.compliance,
        }
    }
    fn into_spec(self) -> Result<MetricDefinitionSpec, MetricDefinitionError> {
        Ok(MetricDefinitionSpec {
            id: self.id,
            version: self.version,
            semantic_description: self.semantic_description,
            unit: self.unit,
            valid_range: self.valid_range,
            evidence_requirements: self.evidence_requirements,
            aggregation: self.aggregation,
            spatial_method: self.spatial_method,
            selection: self.selection,
            uncertainty: self.uncertainty,
            unknown_compatibility: self.unknown_compatibility,
            compatibility: self.compatibility,
            visualization: self.visualization,
            compliance: self.compliance,
        })
    }
}

fn validate_spec(spec: &MetricDefinitionSpec) -> Result<(), MetricDefinitionError> {
    match (spec.unit, &spec.valid_range) {
        (PhysicalUnit::Categorical, ValidRange::Categorical { .. })
        | (PhysicalUnit::Dbm, ValidRange::Numeric { .. })
        | (PhysicalUnit::Db, ValidRange::Numeric { .. })
        | (PhysicalUnit::Mbps, ValidRange::Numeric { .. })
        | (PhysicalUnit::Hertz, ValidRange::Numeric { .. })
        | (PhysicalUnit::Meters, ValidRange::Numeric { .. })
        | (PhysicalUnit::Seconds, ValidRange::Numeric { .. })
        | (PhysicalUnit::Probability, ValidRange::Numeric { .. })
        | (PhysicalUnit::Dimensionless, ValidRange::Numeric { .. }) => {}
        _ => {
            return Err(MetricDefinitionError::InvalidConfiguration(
                "unit and valid range mismatch",
            ));
        }
    }
    if !matches!(spec.aggregation, AggregationDefinition::SignalDbm { .. })
        || spec.unit != PhysicalUnit::Dbm
    {
        return Err(MetricDefinitionError::InvalidConfiguration(
            "signal aggregation requires dBm",
        ));
    }
    spec.valid_range.validate()?;
    spec.evidence_requirements.validate()?;
    spec.aggregation.validate()?;
    spec.selection.validate()?;
    spec.visualization.validate()?;
    Ok(())
}

fn artifact_version(id: &MetricId, version: MetricVersion) -> Result<Text, MetricDefinitionError> {
    Text::new(format!("{}/{}", id.as_str(), version.get()))
        .map_err(|_| MetricDefinitionError::InvalidIdentifier)
}

fn parse_artifact_version(
    label: &Text,
) -> Result<(MetricId, MetricVersion), MetricDefinitionError> {
    let (id, version) = label
        .as_str()
        .rsplit_once('/')
        .ok_or(MetricDefinitionError::InvalidIdentifier)?;
    let id = MetricId::new(id)?;
    let version = MetricVersion::new(
        version
            .parse::<u16>()
            .map_err(|_| MetricDefinitionError::InvalidVersion)?,
    )?;
    Ok((id, version))
}

fn validate_signal_selection(
    selection: SignalAggregationSelection,
) -> Result<(), MetricDefinitionError> {
    selection.validate().map_err(|error| match error {
        crate::Error::AggregationVersionMismatch => {
            MetricDefinitionError::AggregationVersionMismatch
        }
        crate::Error::TemporalAggregationRequiresMonotonicEvidence => {
            MetricDefinitionError::TemporalAggregationRequiresMonotonicEvidence
        }
        crate::Error::InvalidAggregationConfiguration(reason) => {
            MetricDefinitionError::InvalidAggregationConfiguration(reason)
        }
        crate::Error::ResourceLimit(reason) => MetricDefinitionError::ResourceLimit(reason),
        _ => MetricDefinitionError::InvalidAggregationConfiguration("unexpected aggregation error"),
    })
}

fn map_signal_error(error: kyberia_wifi_semantics::Error) -> crate::Error {
    match error {
        kyberia_wifi_semantics::Error::InvalidConfiguration(reason) => {
            crate::Error::InvalidAggregationConfiguration(reason)
        }
        kyberia_wifi_semantics::Error::DuplicateObservation(observation_id) => {
            crate::Error::DuplicateObservation(observation_id)
        }
        kyberia_wifi_semantics::Error::ClockEpochMismatch => {
            crate::Error::InvalidAggregationConfiguration("clock epoch mismatch")
        }
        kyberia_wifi_semantics::Error::NonMonotonicSequence => {
            crate::Error::InvalidAggregationConfiguration("non-monotonic sequence")
        }
        kyberia_wifi_semantics::Error::TemporalMethodRequiresMonotonicTime => {
            crate::Error::TemporalAggregationRequiresMonotonicEvidence
        }
        kyberia_wifi_semantics::Error::ResourceLimit => {
            crate::Error::ResourceLimit("signal aggregation samples")
        }
        kyberia_wifi_semantics::Error::NumericalFailure => {
            crate::Error::NumericalFailure("signal aggregation")
        }
    }
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
                if depth > MAX_METRIC_DEFINITION_DEPTH {
                    return Err(MetricDefinitionError::ResourceLimit(
                        "metric definition depth",
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

impl MetricDefinition {
    pub fn bind(
        &self,
        artifact: VersionedArtifact,
    ) -> Result<MetricDefinitionBinding, MetricDefinitionError> {
        let bytes = self.canonical_bytes()?;
        MetricDefinitionBinding::from_artifact_bytes(artifact, &bytes, self.signal_aggregation())
    }
}

/// Verified metric-definition artifact used by spatial computation.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricDefinitionBinding {
    artifact: VersionedArtifact,
    definition: MetricDefinition,
    signal_aggregation: SignalAggregationSelection,
    spatial_method: Option<SpatialMethod>,
}

impl Serialize for MetricDefinitionBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Wire<'a> {
            artifact: &'a VersionedArtifact,
            signal_aggregation: SignalAggregationSelection,
        }
        Wire {
            artifact: &self.artifact,
            signal_aggregation: self.signal_aggregation,
        }
        .serialize(serializer)
    }
}
impl MetricDefinitionBinding {
    pub fn from_artifact_bytes(
        artifact: VersionedArtifact,
        bytes: &[u8],
        expected_signal_aggregation: SignalAggregationSelection,
    ) -> Result<Self, MetricDefinitionError> {
        if bytes.len() > MAX_METRIC_DEFINITION_BYTES {
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
        let (definition, spatial_method) =
            if artifact.media_type.as_str() == METRIC_DEFINITION_MEDIA_TYPE {
                let definition = MetricDefinition::from_canonical_bytes(bytes)?;
                if definition.version() != &artifact.version {
                    return Err(MetricDefinitionError::ArtifactVersionMismatch);
                }
                let spatial_method = definition.spatial_method();
                (definition, Some(spatial_method))
            } else if artifact.media_type.as_str() == SIGNAL_METRIC_DEFINITION_MEDIA_TYPE {
                let legacy = SignalMetricDefinition::from_canonical_bytes(bytes)?;
                if legacy.version() != &artifact.version {
                    return Err(MetricDefinitionError::ArtifactVersionMismatch);
                }
                let definition = MetricDefinition::from_legacy(
                    legacy.version().clone(),
                    legacy.signal_aggregation(),
                    bytes.to_vec(),
                )?;
                (definition, None)
            } else {
                return Err(MetricDefinitionError::ArtifactMediaTypeMismatch);
            };
        if definition.signal_aggregation() != expected_signal_aggregation {
            return Err(MetricDefinitionError::SelectionMismatch);
        }
        Ok(Self {
            artifact,
            definition,
            signal_aggregation: expected_signal_aggregation,
            spatial_method,
        })
    }
    pub fn artifact(&self) -> &VersionedArtifact {
        &self.artifact
    }
    pub const fn signal_aggregation(&self) -> SignalAggregationSelection {
        self.signal_aggregation
    }
    pub fn definition(&self) -> &MetricDefinition {
        &self.definition
    }
    pub const fn spatial_method(&self) -> Option<SpatialMethod> {
        self.spatial_method
    }
    pub fn definition_hash(&self) -> Result<ContentHash, MetricDefinitionError> {
        self.definition.content_hash()
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, MetricDefinitionError> {
        self.definition.canonical_bytes()
    }
}
