use serde::{Deserialize, Serialize};

pub const MANIFEST_SCHEMA: &str = "rfatlas.plugin-manifest";
pub const MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const CANONICAL_MANIFEST_FORMAT: &str = "rfatlas.plugin-manifest-json-v1";

/// Stable release version tuple used by the wire contract (no pre-release or
/// build metadata). A future SemVer extension requires a new manifest schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SemanticVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityRange {
    pub minimum_inclusive: SemanticVersion,
    pub maximum_exclusive: SemanticVersion,
}

impl CompatibilityRange {
    pub fn contains(&self, version: SemanticVersion) -> bool {
        self.minimum_inclusive <= version && version < self.maximum_exclusive
    }

    pub fn is_valid(&self) -> bool {
        self.minimum_inclusive < self.maximum_exclusive
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Collector,
    Metric,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntime {
    WasmComponent,
}

/// Closed v1 capability vocabulary. New authority requires a reviewed schema
/// and host-contract change; plugins cannot introduce arbitrary capability IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    #[serde(rename = "rfatlas.capture_events.read")]
    CaptureEventsRead,
    #[serde(rename = "rfatlas.derived_layers.emit")]
    DerivedLayersEmit,
    #[serde(rename = "rfatlas.exports.create")]
    ExportsCreate,
    #[serde(rename = "rfatlas.geometry.read")]
    GeometryRead,
    #[serde(rename = "rfatlas.observations.emit")]
    ObservationsEmit,
    #[serde(rename = "rfatlas.observations.read")]
    ObservationsRead,
    #[serde(rename = "rfatlas.project.read")]
    ProjectRead,
}

impl Capability {
    pub const fn id(self) -> &'static str {
        match self {
            Self::CaptureEventsRead => "rfatlas.capture_events.read",
            Self::DerivedLayersEmit => "rfatlas.derived_layers.emit",
            Self::ExportsCreate => "rfatlas.exports.create",
            Self::GeometryRead => "rfatlas.geometry.read",
            Self::ObservationsEmit => "rfatlas.observations.emit",
            Self::ObservationsRead => "rfatlas.observations.read",
            Self::ProjectRead => "rfatlas.project.read",
        }
    }
}

/// Versioned payload contracts shared by the three supported plugin roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DataContract {
    #[serde(rename = "rfatlas.capture-batch")]
    CaptureBatch,
    #[serde(rename = "rfatlas.derived-layer")]
    DerivedLayer,
    #[serde(rename = "rfatlas.export-artifact")]
    ExportArtifact,
    #[serde(rename = "rfatlas.export-view")]
    ExportView,
    #[serde(rename = "rfatlas.metric-input")]
    MetricInput,
    #[serde(rename = "rfatlas.observation-batch")]
    ObservationBatch,
}

impl DataContract {
    pub const fn id(self) -> &'static str {
        match self {
            Self::CaptureBatch => "rfatlas.capture-batch",
            Self::DerivedLayer => "rfatlas.derived-layer",
            Self::ExportArtifact => "rfatlas.export-artifact",
            Self::ExportView => "rfatlas.export-view",
            Self::MetricInput => "rfatlas.metric-input",
            Self::ObservationBatch => "rfatlas.observation-batch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractRequirement {
    pub contract: DataContract,
    pub versions: CompatibilityRange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    pub capability: Capability,
    pub versions: CompatibilityRange,
}

/// Requested ceilings are declarations for a future host to enforce. This
/// crate only checks that requests fit within the host's advertised policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_records: u64,
    pub max_memory_bytes: u64,
    pub max_fuel: u64,
    pub max_wall_time_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema: String,
    pub schema_version: u16,
    pub plugin_id: String,
    pub plugin_version: SemanticVersion,
    pub kind: PluginKind,
    pub runtime: PluginRuntime,
    /// SHA-256 and byte length of the exact WebAssembly component blob.
    pub component_sha256: String,
    pub component_size_bytes: u64,
    pub host_api: CompatibilityRange,
    pub input: ContractRequirement,
    pub output: ContractRequirement,
    /// Must contain exactly the capabilities associated with this plugin kind.
    pub capabilities: Vec<CapabilityRequirement>,
    pub requested_resources: ResourceLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityOffer {
    pub capability: Capability,
    pub version: SemanticVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractOffer {
    pub contract: DataContract,
    pub version: SemanticVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostDescriptor {
    pub api_version: SemanticVersion,
    pub capabilities: Vec<CapabilityOffer>,
    pub contracts: Vec<ContractOffer>,
    pub resource_limits: ResourceLimits,
    pub max_manifest_bytes: u64,
    pub max_plugins: usize,
}

/// Stable project reference. It binds the plugin bytes and every canonical
/// manifest field so a project cannot silently resolve changed plugin authority.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginReference {
    pub plugin_id: String,
    pub plugin_version: SemanticVersion,
    pub component_sha256: String,
    pub component_size_bytes: u64,
    pub manifest_schema_version: u16,
    pub canonical_manifest_format: String,
    pub canonical_manifest_sha256: String,
}

/// The result is a validated declaration, not a user authorization decision
/// and not an enforcement token for a runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegotiatedPlugin {
    pub project_reference: PluginReference,
    pub kind: PluginKind,
    pub runtime: PluginRuntime,
    pub capabilities: Vec<CapabilityOffer>,
    pub input: DataContract,
    pub input_version: SemanticVersion,
    pub output: DataContract,
    pub output_version: SemanticVersion,
    pub requested_resources: ResourceLimits,
}
