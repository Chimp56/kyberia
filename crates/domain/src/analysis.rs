//! Immutable, content-bound spatial analysis requests. No filesystem or clocks.
//!
//! The V1 canonical encoding is deliberately versioned, with golden fixtures.
//! It is not RFC 8785: full-width integers use decimal strings, and the pinned
//! serde_json encoder defines finite binary64 formatting. See the format doc.
use crate::{
    ValidationError,
    identity::{ContentHash, FloorId, FrameId, SessionId, SnapshotId, Text},
    units::{
        CoordinateMeters, Db, Dbm, Dimensionless, Hertz, Mbps, Meters, Probability, Radians,
        Seconds,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_MANIFEST_BYTES: usize = 1_048_576;
pub const MAX_INPUTS: usize = 4096;
pub const MAX_PARAMETERS: usize = 128;

/// Exact decimal integer on the wire, safe through JSON consumers using doubles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ExactU64(u64);
impl ExactU64 {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl From<ExactU64> for String {
    fn from(value: ExactU64) -> Self {
        value.0.to_string()
    }
}
impl TryFrom<String> for ExactU64 {
    type Error = ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed = value
            .parse::<u64>()
            .map_err(|_| ValidationError::InvalidText)?;
        if value != parsed.to_string() {
            return Err(ValidationError::InvalidText);
        }
        Ok(Self(parsed))
    }
}

/// A version label is never sufficient: the immutable bytes are also pinned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedArtifact {
    pub version: Text,
    pub sha256: ContentHash,
    pub byte_length: ExactU64,
    pub media_type: Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurveyInput {
    pub session_id: SessionId,
    pub snapshot_id: SnapshotId,
    /// Pins the immutable snapshot/chunk index, including its observation hashes.
    pub snapshot: VersionedArtifact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum ClientProfile {
    NotApplicable {},
    Pinned { artifact: VersionedArtifact },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub enum Randomness {
    Deterministic {},
    Seeded { seed: ExactU64 },
}

/// Typed parameter values; no undocumented arbitrary JSON blob or implicit units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "unit",
    content = "value",
    deny_unknown_fields,
    rename_all = "snake_case"
)]
pub enum ParameterValue {
    Boolean(bool),
    Text(Text),
    Count(ExactU64),
    Dimensionless(Dimensionless),
    Db(Db),
    Dbm(Dbm),
    Hertz(Hertz),
    Meters(Meters),
    Seconds(Seconds),
    Mbps(Mbps),
    Probability(Probability),
    Radians(Radians),
    Artifact(VersionedArtifact),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: Text,
    pub value: ParameterValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Algorithm {
    pub name: Text,
    pub version: Text,
    /// Includes executable code and dependency/runtime build identity.
    pub implementation: VersionedArtifact,
    /// Pins platform/backend, tolerances and deterministic execution assumptions.
    pub execution_profile: VersionedArtifact,
    pub parameters: Vec<Parameter>,
    pub randomness: Randomness,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputGrid {
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    /// Lower-left outer corner; cell (column,row) center is origin+(index+0.5)*resolution.
    pub origin_x_m: CoordinateMeters,
    pub origin_y_m: CoordinateMeters,
    pub elevation_m: CoordinateMeters,
    pub resolution_m: Meters,
    pub columns: u32,
    pub rows: u32,
    /// Explicit numerical support/occupied-area mask, never an implicit all-valid mask.
    pub area_mask: VersionedArtifact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestVersion {
    #[serde(rename = "kyberia-spatial-analysis/1")]
    V1,
}

/// Mutable construction/wire record. It becomes immutable only after validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSpec {
    pub schema: ManifestVersion,
    pub surveys: Vec<SurveyInput>,
    pub identity_graph: VersionedArtifact,
    pub geometry: VersionedArtifact,
    pub metric_definition: VersionedArtifact,
    pub client_profile: ClientProfile,
    /// Versioned filters, grouping and requested evidence/uncertainty policies.
    pub selection_policy: VersionedArtifact,
    pub algorithm: Algorithm,
    pub output_grid: OutputGrid,
}

/// Validated immutable spec plus its exact canonical bytes and content address.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisManifest {
    spec: ManifestSpec,
    canonical: Vec<u8>,
    sha256: ContentHash,
}
impl AnalysisManifest {
    pub fn new(mut spec: ManifestSpec) -> Result<Self, ValidationError> {
        if spec.surveys.is_empty() || spec.surveys.len() > MAX_INPUTS {
            return Err(ValidationError::ResourceLimit("analysis survey inputs"));
        }
        // These collections are sets. Execution-sensitive sequences belong in a
        // pinned policy artifact and must never be sorted implicitly here.
        spec.surveys.sort_by_key(|s| (s.session_id, s.snapshot_id));
        if spec
            .surveys
            .windows(2)
            .any(|s| s[0].session_id == s[1].session_id)
        {
            return Err(ValidationError::Inconsistent(
                "multiple snapshots for one session",
            ));
        }
        if spec.algorithm.parameters.len() > MAX_PARAMETERS {
            return Err(ValidationError::ResourceLimit("analysis parameters"));
        }
        spec.algorithm
            .parameters
            .sort_by(|a, b| a.name.cmp(&b.name));
        if spec
            .algorithm
            .parameters
            .windows(2)
            .any(|p| p[0].name == p[1].name)
        {
            return Err(ValidationError::Inconsistent(
                "duplicate analysis parameter",
            ));
        }
        let grid = &spec.output_grid;
        if grid.columns == 0 || grid.rows == 0 || grid.resolution_m.get() <= 0.0 {
            return Err(ValidationError::OutOfRange("analysis grid dimensions"));
        }
        if u64::from(grid.columns) * u64::from(grid.rows) > 1_000_000_000 {
            return Err(ValidationError::ResourceLimit("analysis grid cells"));
        }
        for (origin, cells) in [
            (grid.origin_x_m.get(), grid.columns),
            (grid.origin_y_m.get(), grid.rows),
        ] {
            let extent = grid.resolution_m.get() * f64::from(cells);
            let end = origin + extent;
            let first_center = origin + 0.5 * grid.resolution_m.get();
            let last_center = origin + (f64::from(cells) - 0.5) * grid.resolution_m.get();
            // Conservative roundoff budget for index multiplication and origin
            // addition, independent of cell count. Checking endpoints alone
            // does not rule out collapsed interior cell centers.
            let scale = origin.abs().max(end.abs()).max(extent);
            let roundoff_budget = (8.0 * f64::EPSILON) * scale;
            if !extent.is_finite()
                || !end.is_finite()
                || end <= origin
                || grid.resolution_m.get() <= roundoff_budget
                || first_center <= origin
                || last_center >= end
                || (cells > 1
                    && (first_center >= last_center || end - grid.resolution_m.get() >= end))
            {
                return Err(ValidationError::OutOfRange(
                    "analysis grid precision/extent",
                ));
            }
        }
        let canonical = serde_json::to_vec(&spec)
            .map_err(|_| ValidationError::Inconsistent("manifest serialization"))?;
        if canonical.len() > MAX_MANIFEST_BYTES {
            return Err(ValidationError::ResourceLimit("analysis manifest bytes"));
        }
        let sha256 = ContentHash::from_sha256(Sha256::digest(&canonical).into());
        Ok(Self {
            spec,
            canonical,
            sha256,
        })
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, ValidationError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ValidationError::ResourceLimit("analysis manifest bytes"));
        }
        let spec = serde_json::from_slice(bytes)
            .map_err(|_| ValidationError::Inconsistent("invalid analysis manifest JSON"))?;
        Self::new(spec)
    }
    pub fn spec(&self) -> &ManifestSpec {
        &self.spec
    }
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }
    pub const fn sha256(&self) -> ContentHash {
        self.sha256
    }

    /// Pure verification. The caller loads bytes through its bounded artifact port.
    pub fn verify_artifact(reference: &VersionedArtifact, bytes: &[u8]) -> bool {
        reference.byte_length.get() == bytes.len() as u64
            && reference.sha256.bytes() == <[u8; 32]>::from(Sha256::digest(bytes))
    }
}
