//! Immutable, versioned project operations for offline collaboration.
//!
//! This crate is deliberately inward: it contains typed project commands and
//! deterministic merge rules, but no storage, clock, UI, packet, or platform
//! imports. A wall clock is not part of an operation's ordering contract. The
//! application supplies a Lamport-style [`LogicalTimestamp`] and a causal
//! parent list when it creates a command.
//!
//! High-volume observations never enter this model. A command can carry an
//! [`ImmutableReference`] to a content-addressed observation chunk, which
//! keeps acquisition data in its own append-only plane.

use kyberia_domain::{
    ValidationError,
    evidence::{Evidence, UnknownReason},
    identity::{
        ActorDeviceId, ActorId, CalibrationId, ContentHash, FloorId, MapAssetId, OperationId,
        ProjectId, SiteId, Text,
    },
};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    fmt,
};

/// `ActorDeviceId` is the canonical domain device identity. This alias keeps
/// the operation API readable without introducing a second incompatible ID.
pub type DeviceId = ActorDeviceId;

/// Operation schema versions admitted by this crate.
///
/// V1 remains byte-for-byte compatible with the original operation contract.
/// V2 changes only inverse representation: it can carry an explicit typed
/// prior calibration state and the non-reversible floor-evidence binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OperationSchemaVersion {
    #[serde(rename = "1")]
    V1,
    #[serde(rename = "2")]
    V2,
}

/// A positive Lamport-style logical time. It is never derived from UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct LogicalTimestamp(u64);

impl LogicalTimestamp {
    pub fn new(value: u64) -> Result<Self, OperationError> {
        if value == 0 {
            return Err(OperationError::InvalidLogicalTimestamp);
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for LogicalTimestamp {
    type Error = OperationError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<LogicalTimestamp> for u64 {
    fn from(value: LogicalTimestamp) -> Self {
        value.0
    }
}

/// A linear project revision owned by [`OperationLog`]. It is not the causal
/// depth encoded in an immutable DAG operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectVersion(u64);

impl ProjectVersion {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for ProjectVersion {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl From<ProjectVersion> for u64 {
    fn from(value: ProjectVersion) -> Self {
        value.0
    }
}

/// The causal depth of an operation in the DAG. A root has depth zero; a
/// child has one plus the maximum depth of its parents. This value is not a
/// persisted materialized-project revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CausalDepth(u64);

impl CausalDepth {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for CausalDepth {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl From<CausalDepth> for u64 {
    fn from(value: CausalDepth) -> Self {
        value.0
    }
}

/// Limits are part of the format contract, rather than storage policy.
pub const MAX_PARENTS: usize = 8;
pub const MAX_OPERATION_COUNT: usize = 100_000;
pub const MAX_OPERATION_CANONICAL_BYTES: usize = 32 * 1024;
pub const MAX_OPERATION_WIRE_BYTES: usize = 48 * 1024;
pub const MAX_REFERENCED_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_CONFLICTS: usize = 8_192;
pub const MAX_ANCESTRY_WORK: usize = 2_000_000;
const MAX_MERGE_FRONTIER: usize = MAX_CONFLICTS;

/// A content-addressed reference to an immutable artifact or observation
/// chunk. The bytes themselves are intentionally not representable here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ImmutableReferenceWire", into = "ImmutableReferenceWire")]
pub struct ImmutableReference {
    hash: ContentHash,
    media_type: Text,
    byte_length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImmutableReferenceWire {
    hash: ContentHash,
    media_type: Text,
    byte_length: u64,
}

impl ImmutableReference {
    pub fn new(
        hash: ContentHash,
        media_type: Text,
        byte_length: u64,
    ) -> Result<Self, OperationError> {
        if byte_length == 0 || byte_length > MAX_REFERENCED_ARTIFACT_BYTES {
            return Err(OperationError::ResourceLimit("referenced_artifact"));
        }
        Ok(Self {
            hash,
            media_type,
            byte_length,
        })
    }

    pub const fn hash(&self) -> ContentHash {
        self.hash
    }

    pub fn media_type(&self) -> &Text {
        &self.media_type
    }

    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }
}

impl TryFrom<ImmutableReferenceWire> for ImmutableReference {
    type Error = OperationError;

    fn try_from(value: ImmutableReferenceWire) -> Result<Self, Self::Error> {
        Self::new(value.hash, value.media_type, value.byte_length)
    }
}

impl From<ImmutableReference> for ImmutableReferenceWire {
    fn from(value: ImmutableReference) -> Self {
        Self {
            hash: value.hash,
            media_type: value.media_type,
            byte_length: value.byte_length,
        }
    }
}

/// A stable identity for a mutable project field. Conflict detection is
/// performed on this key, never on rendered text or a generic JSON path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FieldKey {
    ProjectName,
    SiteName(SiteId),
    MapCalibration(MapAssetId),
    FloorEvidence(FloorId),
}

/// Closed, typed project mutations admitted by operation schema version 1.
/// Observation payloads, packet bytes, and arbitrary JSON values have no
/// variant in this enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Mutation {
    SetProjectName {
        name: Text,
    },
    SetSiteName {
        site_id: SiteId,
        name: Text,
    },
    ActivateCalibration {
        map_id: MapAssetId,
        calibration_id: CalibrationId,
    },
    BindFloorEvidence {
        floor_id: FloorId,
        reference: ImmutableReference,
    },
}

impl Mutation {
    pub fn set_project_name(name: Text) -> Self {
        Self::SetProjectName { name }
    }

    pub fn set_site_name(site_id: SiteId, name: Text) -> Self {
        Self::SetSiteName { site_id, name }
    }

    pub fn activate_calibration(map_id: MapAssetId, calibration_id: CalibrationId) -> Self {
        Self::ActivateCalibration {
            map_id,
            calibration_id,
        }
    }

    pub fn bind_floor_evidence(floor_id: FloorId, reference: ImmutableReference) -> Self {
        Self::BindFloorEvidence {
            floor_id,
            reference,
        }
    }

    pub const fn field_key(&self) -> FieldKey {
        match self {
            Self::SetProjectName { .. } => FieldKey::ProjectName,
            Self::SetSiteName { site_id, .. } => FieldKey::SiteName(*site_id),
            Self::ActivateCalibration { map_id, .. } => FieldKey::MapCalibration(*map_id),
            Self::BindFloorEvidence { floor_id, .. } => FieldKey::FloorEvidence(*floor_id),
        }
    }
}

/// The typed state that existed immediately before a V2 apply. This is a
/// semantic prior, rather than a second generic command envelope. In
/// particular, calibration may explicitly be `Unknown(NotMeasured)`, which
/// cannot be represented by V1's `Mutation::ActivateCalibration` inverse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InversePrior {
    ProjectName {
        name: Text,
    },
    SiteName {
        site_id: SiteId,
        name: Text,
    },
    MapCalibration {
        map_id: MapAssetId,
        calibration: Evidence<CalibrationId>,
    },
}

impl InversePrior {
    pub const fn field_key(&self) -> FieldKey {
        match self {
            Self::ProjectName { .. } => FieldKey::ProjectName,
            Self::SiteName { site_id, .. } => FieldKey::SiteName(*site_id),
            Self::MapCalibration { map_id, .. } => FieldKey::MapCalibration(*map_id),
        }
    }

    fn validate_for(&self, mutation: &Mutation) -> Result<(), OperationError> {
        let valid = match (mutation, self) {
            (Mutation::SetProjectName { .. }, Self::ProjectName { .. }) => true,
            (
                Mutation::SetSiteName { site_id, .. },
                Self::SiteName {
                    site_id: prior_site_id,
                    ..
                },
            ) => site_id == prior_site_id,
            (
                Mutation::ActivateCalibration { map_id, .. },
                Self::MapCalibration {
                    map_id: prior_map_id,
                    calibration,
                },
            ) => {
                map_id == prior_map_id
                    && matches!(
                        calibration,
                        Evidence::Known(_) | Evidence::Unknown(UnknownReason::NotMeasured)
                    )
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(OperationError::InvalidInverse)
        }
    }

    /// Convert a representable prior to the legacy mutation form. Unknown
    /// calibration is intentionally not converted: doing so would invent an
    /// identifier and lose the domain's explicit unknown state.
    pub fn as_mutation(&self) -> Result<Mutation, OperationError> {
        match self {
            Self::ProjectName { name } => Ok(Mutation::SetProjectName { name: name.clone() }),
            Self::SiteName { site_id, name } => Ok(Mutation::SetSiteName {
                site_id: *site_id,
                name: name.clone(),
            }),
            Self::MapCalibration {
                map_id,
                calibration: Evidence::Known(calibration_id),
            } => Ok(Mutation::ActivateCalibration {
                map_id: *map_id,
                calibration_id: *calibration_id,
            }),
            Self::MapCalibration {
                calibration: Evidence::Unknown(_),
                ..
            } => Err(OperationError::TypedPriorRequired),
        }
    }
}

/// Why an operation has no executable inverse. This is intentionally closed
/// and tied to the existing domain's irreversible evidence binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NonReversibleReason {
    FloorEvidenceBinding,
}

/// A stable reference to one immutable operation, including its digest. A
/// matching ID with different bytes is a tamper signal, never a replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationReference {
    operation_id: OperationId,
    content_hash: ContentHash,
}

impl OperationReference {
    pub const fn new(operation_id: OperationId, content_hash: ContentHash) -> Self {
        Self {
            operation_id,
            content_hash,
        }
    }

    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    pub const fn content_hash(self) -> ContentHash {
        self.content_hash
    }
}

/// The inverse of an apply is typed data. Undo and redo inverses are immutable
/// references, so history remains intact and can be audited after toggling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InverseMetadata {
    Apply {
        mutation: Mutation,
    },
    /// V2 apply metadata records a typed prior state. Unlike V1 this can
    /// preserve the domain's explicit unknown calibration state.
    ApplyV2 {
        prior: InversePrior,
    },
    /// V2 binds an irreversible operation to a closed, auditable reason. It
    /// is not an executable command and therefore cannot be an undo target.
    NonReversible {
        reason: NonReversibleReason,
    },
    Toggle {
        target: OperationReference,
        direction: ToggleDirection,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleDirection {
    Undo,
    Redo,
}

/// A closed operation command. Its inverse metadata is validated against the
/// payload before the operation receives a content hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OperationPayload {
    Apply {
        mutation: Mutation,
    },
    Undo {
        target: OperationReference,
    },
    Redo {
        target: OperationReference,
    },
    Resolve {
        left: OperationReference,
        right: OperationReference,
        mutation: Mutation,
    },
}

/// A validated immutable operation. Fields are private so callers must use a
/// checked constructor or a strict, hash-verifying deserializer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "OperationWire", into = "OperationWire")]
pub struct Operation {
    schema_version: OperationSchemaVersion,
    operation_id: OperationId,
    project_id: ProjectId,
    actor_id: ActorId,
    device_id: DeviceId,
    logical_time: LogicalTimestamp,
    causal_depth: CausalDepth,
    parents: Vec<OperationId>,
    payload: OperationPayload,
    inverse: InverseMetadata,
    content_hash: ContentHash,
    canonical: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationWire {
    schema_version: OperationSchemaVersion,
    operation_id: OperationId,
    project_id: ProjectId,
    actor_id: ActorId,
    device_id: DeviceId,
    logical_time: LogicalTimestamp,
    causal_depth: CausalDepth,
    #[serde(deserialize_with = "bounded_parents")]
    parents: Vec<OperationId>,
    payload: OperationPayload,
    inverse: InverseMetadata,
    content_hash: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct UnsignedOperationWire {
    schema_version: OperationSchemaVersion,
    operation_id: OperationId,
    project_id: ProjectId,
    actor_id: ActorId,
    device_id: DeviceId,
    logical_time: LogicalTimestamp,
    causal_depth: CausalDepth,
    parents: Vec<OperationId>,
    payload: OperationPayload,
    inverse: InverseMetadata,
}

fn bounded_parents<'de, D>(deserializer: D) -> Result<Vec<OperationId>, D::Error>
where
    D: Deserializer<'de>,
{
    struct ParentVisitor;

    impl<'de> de::Visitor<'de> for ParentVisitor {
        type Value = Vec<OperationId>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an ordered list of at most eight operation IDs")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut parents = Vec::with_capacity(MAX_PARENTS.min(2));
            while let Some(parent) = sequence.next_element()? {
                if parents.len() >= MAX_PARENTS {
                    return Err(de::Error::custom("parent limit exceeded"));
                }
                parents.push(parent);
            }
            Ok(parents)
        }
    }

    deserializer.deserialize_seq(ParentVisitor)
}

impl Operation {
    #[allow(clippy::too_many_arguments)]
    pub fn try_apply(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        mutation: Mutation,
        inverse: Mutation,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V1,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Apply { mutation },
            InverseMetadata::Apply { mutation: inverse },
            None,
        )
    }

    /// Construct a V2 apply with a typed causal prior. V1 constructors remain
    /// unchanged so their canonical bytes and content hashes remain stable.
    #[allow(clippy::too_many_arguments)]
    pub fn try_apply_v2(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        mutation: Mutation,
        prior: InversePrior,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V2,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Apply { mutation },
            InverseMetadata::ApplyV2 { prior },
            None,
        )
    }

    /// Construct a V2 apply whose domain effect has no executable inverse.
    /// The closed reason is persisted so consumers can distinguish an
    /// intentionally irreversible operation from malformed inverse metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn try_apply_v2_non_reversible(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        mutation: Mutation,
        reason: NonReversibleReason,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V2,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Apply { mutation },
            InverseMetadata::NonReversible { reason },
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_undo(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        target: OperationReference,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V1,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Undo { target },
            InverseMetadata::Toggle {
                target,
                direction: ToggleDirection::Redo,
            },
            None,
        )
    }

    /// Construct a V2 undo. A V2 toggle must remain in the V2 envelope so a
    /// migration cannot silently downgrade its inverse contract.
    #[allow(clippy::too_many_arguments)]
    pub fn try_undo_v2(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        target: OperationReference,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V2,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Undo { target },
            InverseMetadata::Toggle {
                target,
                direction: ToggleDirection::Redo,
            },
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_redo(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        target: OperationReference,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V1,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Redo { target },
            InverseMetadata::Toggle {
                target,
                direction: ToggleDirection::Undo,
            },
            None,
        )
    }

    /// Construct a V2 redo while retaining the V2 operation envelope.
    #[allow(clippy::too_many_arguments)]
    pub fn try_redo_v2(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        target: OperationReference,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V2,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Redo { target },
            InverseMetadata::Toggle {
                target,
                direction: ToggleDirection::Undo,
            },
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_resolve(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        left: OperationReference,
        right: OperationReference,
        mutation: Mutation,
        inverse: Mutation,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V1,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Resolve {
                left,
                right,
                mutation,
            },
            InverseMetadata::Apply { mutation: inverse },
            None,
        )
    }

    /// Construct a V2 resolution with a typed inverse prior.
    #[allow(clippy::too_many_arguments)]
    pub fn try_resolve_v2(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        left: OperationReference,
        right: OperationReference,
        mutation: Mutation,
        prior: InversePrior,
    ) -> Result<Self, OperationError> {
        Self::try_parts(
            OperationSchemaVersion::V2,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            OperationPayload::Resolve {
                left,
                right,
                mutation,
            },
            InverseMetadata::ApplyV2 { prior },
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_parts(
        schema_version: OperationSchemaVersion,
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        payload: OperationPayload,
        inverse: InverseMetadata,
        expected_hash: Option<ContentHash>,
    ) -> Result<Self, OperationError> {
        validate_parent_list(&parents)?;
        validate_payload_inverse(schema_version, &payload, &inverse)?;
        if parents.is_empty() {
            if causal_depth.value() != 0 {
                return Err(OperationError::InvalidCausality);
            }
        } else if causal_depth.value() == 0 {
            return Err(OperationError::InvalidCausality);
        }

        let unsigned = UnsignedOperationWire {
            schema_version,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents: parents.clone(),
            payload: payload.clone(),
            inverse: inverse.clone(),
        };
        let canonical =
            serde_json::to_vec(&unsigned).map_err(|_| OperationError::CanonicalEncoding)?;
        if canonical.len() > MAX_OPERATION_CANONICAL_BYTES {
            return Err(OperationError::ResourceLimit("operation_canonical_bytes"));
        }
        let content_hash = digest(&canonical);
        if let Some(expected) = expected_hash
            && expected != content_hash
        {
            return Err(OperationError::HashMismatch);
        }
        let result = Self {
            schema_version,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            payload,
            inverse,
            content_hash,
            canonical,
        };
        let wire_size = serde_json::to_vec(&OperationWire::from(result.clone()))
            .map_err(|_| OperationError::CanonicalEncoding)?
            .len();
        if wire_size > MAX_OPERATION_WIRE_BYTES {
            return Err(OperationError::ResourceLimit("operation_wire_bytes"));
        }
        Ok(result)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OperationError> {
        if bytes.len() > MAX_OPERATION_WIRE_BYTES {
            return Err(OperationError::ResourceLimit("operation_wire_bytes"));
        }
        let wire: OperationWire =
            serde_json::from_slice(bytes).map_err(|_| OperationError::MalformedEncoding)?;
        let operation = Self::try_from(wire)?;
        if operation.to_bytes()? != bytes {
            return Err(OperationError::NonCanonicalEncoding);
        }
        Ok(operation)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, OperationError> {
        let bytes = serde_json::to_vec(&OperationWire::from(self.clone()))
            .map_err(|_| OperationError::CanonicalEncoding)?;
        if bytes.len() > MAX_OPERATION_WIRE_BYTES {
            return Err(OperationError::ResourceLimit("operation_wire_bytes"));
        }
        Ok(bytes)
    }

    /// Bytes are canonical JSON generated from the typed unsigned operation.
    /// The content hash is SHA-256 over exactly these bytes.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    pub const fn content_hash(&self) -> ContentHash {
        self.content_hash
    }

    pub const fn schema_version(&self) -> OperationSchemaVersion {
        self.schema_version
    }

    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn actor_id(&self) -> ActorId {
        self.actor_id
    }

    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }

    pub const fn logical_time(&self) -> LogicalTimestamp {
        self.logical_time
    }

    pub const fn causal_depth(&self) -> CausalDepth {
        self.causal_depth
    }

    pub fn parents(&self) -> &[OperationId] {
        &self.parents
    }

    pub const fn payload(&self) -> &OperationPayload {
        &self.payload
    }

    pub const fn inverse(&self) -> &InverseMetadata {
        &self.inverse
    }

    pub const fn sort_key(&self) -> (LogicalTimestamp, ActorId, DeviceId, OperationId) {
        (
            self.logical_time,
            self.actor_id,
            self.device_id,
            self.operation_id,
        )
    }

    pub const fn target_reference(&self) -> Option<OperationReference> {
        match self.payload {
            OperationPayload::Apply { .. } => None,
            OperationPayload::Undo { target } | OperationPayload::Redo { target } => Some(target),
            OperationPayload::Resolve { .. } => None,
        }
    }

    pub const fn resolution_references(&self) -> Option<(OperationReference, OperationReference)> {
        match self.payload {
            OperationPayload::Resolve { left, right, .. } => Some((left, right)),
            _ => None,
        }
    }
}

impl TryFrom<OperationWire> for Operation {
    type Error = OperationError;

    fn try_from(value: OperationWire) -> Result<Self, Self::Error> {
        let OperationWire {
            schema_version,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            payload,
            inverse,
            content_hash,
        } = value;
        let result = Self::try_parts(
            schema_version,
            operation_id,
            project_id,
            actor_id,
            device_id,
            logical_time,
            causal_depth,
            parents,
            payload,
            inverse,
            Some(content_hash),
        )?;
        Ok(result)
    }
}

impl From<Operation> for OperationWire {
    fn from(value: Operation) -> Self {
        Self {
            schema_version: value.schema_version,
            operation_id: value.operation_id,
            project_id: value.project_id,
            actor_id: value.actor_id,
            device_id: value.device_id,
            logical_time: value.logical_time,
            causal_depth: value.causal_depth,
            parents: value.parents,
            payload: value.payload,
            inverse: value.inverse,
            content_hash: value.content_hash,
        }
    }
}

impl From<&Operation> for OperationReference {
    fn from(value: &Operation) -> Self {
        Self::new(value.operation_id, value.content_hash)
    }
}

fn validate_parent_list(parents: &[OperationId]) -> Result<(), OperationError> {
    if parents.len() > MAX_PARENTS {
        return Err(OperationError::ResourceLimit("operation_parents"));
    }
    if parents.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(OperationError::InvalidCausality);
    }
    Ok(())
}

fn validate_payload_inverse(
    schema_version: OperationSchemaVersion,
    payload: &OperationPayload,
    inverse: &InverseMetadata,
) -> Result<(), OperationError> {
    match schema_version {
        OperationSchemaVersion::V1 => match (payload, inverse) {
            (OperationPayload::Apply { mutation }, InverseMetadata::Apply { mutation: undo })
                if mutation.field_key() == undo.field_key() => {}
            (
                OperationPayload::Undo { target },
                InverseMetadata::Toggle {
                    target: inverse_target,
                    direction: ToggleDirection::Redo,
                },
            )
            | (
                OperationPayload::Redo { target },
                InverseMetadata::Toggle {
                    target: inverse_target,
                    direction: ToggleDirection::Undo,
                },
            ) if target == inverse_target => {}
            (
                OperationPayload::Resolve { mutation, .. },
                InverseMetadata::Apply { mutation: inverse },
            ) if mutation.field_key() == inverse.field_key() => {}
            _ => return Err(OperationError::InvalidInverse),
        },
        OperationSchemaVersion::V2 => match (payload, inverse) {
            (OperationPayload::Apply { mutation }, InverseMetadata::ApplyV2 { prior })
            | (OperationPayload::Resolve { mutation, .. }, InverseMetadata::ApplyV2 { prior }) => {
                prior.validate_for(mutation)?;
            }
            (
                OperationPayload::Apply {
                    mutation: Mutation::BindFloorEvidence { .. },
                },
                InverseMetadata::NonReversible {
                    reason: NonReversibleReason::FloorEvidenceBinding,
                },
            ) => {}
            (
                OperationPayload::Undo { target },
                InverseMetadata::Toggle {
                    target: inverse_target,
                    direction: ToggleDirection::Redo,
                },
            )
            | (
                OperationPayload::Redo { target },
                InverseMetadata::Toggle {
                    target: inverse_target,
                    direction: ToggleDirection::Undo,
                },
            ) if target == inverse_target => {}
            _ => return Err(OperationError::InvalidInverse),
        },
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> ContentHash {
    ContentHash::from_sha256(Sha256::digest(bytes).into())
}

/// Result of appending to a single linear local log. A duplicate with the
/// same hash is a successful idempotent replay; it does not advance revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppendOutcome {
    Appended { revision: ProjectVersion },
    Duplicate { revision: ProjectVersion },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppendError {
    Operation(OperationError),
    WrongProject,
    CausalDepthConflict {
        expected: CausalDepth,
        actual: CausalDepth,
    },
    ParentConflict,
    LogicalTimeConflict,
    TamperedDuplicate,
    MissingTarget,
    TargetHashMismatch,
    InvalidTarget,
    InvalidToggle,
    InvalidResolution,
    ResourceLimit(&'static str),
}

impl fmt::Display for AppendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AppendError {}

impl From<OperationError> for AppendError {
    fn from(value: OperationError) -> Self {
        Self::Operation(value)
    }
}

/// A command-side linear append validator. Its [`ProjectVersion`] revision is
/// local materializer state; each operation carries an independent
/// [`CausalDepth`] for DAG validation. Query methods expose immutable
/// operations; no storage or project materializer is coupled to this type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationLog {
    project_id: ProjectId,
    revision: ProjectVersion,
    logical_time: LogicalTimestamp,
    tip: Option<OperationId>,
    operations: BTreeMap<OperationId, Operation>,
    active: BTreeMap<OperationId, bool>,
}

impl OperationLog {
    pub fn new(project_id: ProjectId) -> Self {
        Self {
            project_id,
            revision: ProjectVersion::new(0),
            logical_time: LogicalTimestamp(0),
            tip: None,
            operations: BTreeMap::new(),
            active: BTreeMap::new(),
        }
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn revision(&self) -> ProjectVersion {
        self.revision
    }

    pub const fn logical_time(&self) -> Option<LogicalTimestamp> {
        match self.logical_time.0 {
            0 => None,
            value => Some(LogicalTimestamp(value)),
        }
    }

    pub const fn tip(&self) -> Option<OperationId> {
        self.tip
    }

    pub fn operation(&self, id: OperationId) -> Option<&Operation> {
        self.operations.get(&id)
    }

    pub fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.operations.values()
    }

    pub fn append(&mut self, operation: Operation) -> Result<AppendOutcome, AppendError> {
        if operation.project_id() != self.project_id {
            return Err(AppendError::WrongProject);
        }
        if let Some(existing) = self.operations.get(&operation.operation_id()) {
            if existing.content_hash() == operation.content_hash() {
                return Ok(AppendOutcome::Duplicate {
                    revision: self.revision,
                });
            }
            return Err(AppendError::TamperedDuplicate);
        }
        if self.operations.len() >= MAX_OPERATION_COUNT {
            return Err(AppendError::ResourceLimit("operation_count"));
        }
        let expected_parents: Vec<_> = self.tip.into_iter().collect();
        if operation.parents() != expected_parents {
            return Err(AppendError::ParentConflict);
        }
        let expected_depth = match self.tip {
            None => CausalDepth::new(0),
            Some(tip) => CausalDepth::new(
                self.operations
                    .get(&tip)
                    .ok_or(AppendError::ParentConflict)?
                    .causal_depth()
                    .value()
                    .checked_add(1)
                    .ok_or(AppendError::ResourceLimit("causal_depth"))?,
            ),
        };
        if operation.causal_depth() != expected_depth {
            return Err(AppendError::CausalDepthConflict {
                expected: expected_depth,
                actual: operation.causal_depth(),
            });
        }
        if operation.logical_time().value() <= self.logical_time.0 {
            return Err(AppendError::LogicalTimeConflict);
        }
        self.validate_target(&operation)?;
        self.revision = ProjectVersion::new(
            self.revision
                .value()
                .checked_add(1)
                .ok_or(AppendError::ResourceLimit("project_revision"))?,
        );
        self.logical_time = operation.logical_time();
        self.tip = Some(operation.operation_id());
        match operation.payload() {
            OperationPayload::Apply { .. } => {
                self.active.insert(operation.operation_id(), true);
            }
            OperationPayload::Undo { target } => {
                self.active.insert(target.operation_id(), false);
            }
            OperationPayload::Redo { target } => {
                self.active.insert(target.operation_id(), true);
            }
            OperationPayload::Resolve { .. } => {}
        }
        self.operations.insert(operation.operation_id(), operation);
        Ok(AppendOutcome::Appended {
            revision: self.revision,
        })
    }

    pub fn as_set(&self) -> Result<OperationSet, MergeError> {
        if self.operations.is_empty() {
            return Ok(OperationSet::empty(self.project_id));
        }
        OperationSet::from_operations(self.operations.values().cloned())
            .map_err(MergeError::Operation)
    }

    fn validate_target(&self, operation: &Operation) -> Result<(), AppendError> {
        if matches!(operation.payload(), OperationPayload::Resolve { .. }) {
            // A linear local log has no concurrent heads to resolve. Resolution
            // commands are admitted through a validated OperationSet join.
            return Err(AppendError::InvalidResolution);
        }
        let Some(target) = operation.target_reference() else {
            return Ok(());
        };
        let Some(existing) = self.operations.get(&target.operation_id()) else {
            return Err(AppendError::MissingTarget);
        };
        if existing.content_hash() != target.content_hash() {
            return Err(AppendError::TargetHashMismatch);
        }
        if existing.schema_version() != operation.schema_version() {
            return Err(AppendError::Operation(OperationError::VersionMismatch));
        }
        if !matches!(existing.payload(), OperationPayload::Apply { .. }) {
            return Err(AppendError::InvalidTarget);
        }
        if matches!(existing.inverse(), InverseMetadata::NonReversible { .. }) {
            return Err(AppendError::Operation(OperationError::NonReversibleTarget));
        }
        match operation.payload() {
            OperationPayload::Undo { target }
                if self.active.get(&target.operation_id()) == Some(&true) => {}
            OperationPayload::Redo { target }
                if self.active.get(&target.operation_id()) == Some(&false) => {}
            OperationPayload::Undo { .. } | OperationPayload::Redo { .. } => {
                return Err(AppendError::InvalidToggle);
            }
            OperationPayload::Apply { .. } | OperationPayload::Resolve { .. } => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FrontierIdentity {
    Value(EffectValue),
    Toggle {
        target: OperationReference,
        direction: ToggleDirection,
        effect: EffectValue,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EffectValue {
    Mutation(Mutation),
    Calibration {
        map_id: MapAssetId,
        calibration: Evidence<CalibrationId>,
    },
}

impl EffectValue {
    fn field_key(&self) -> FieldKey {
        match self {
            Self::Mutation(mutation) => mutation.field_key(),
            Self::Calibration { map_id, .. } => FieldKey::MapCalibration(*map_id),
        }
    }

    /// Normalize equivalent representations before comparing frontier values.
    ///
    /// A known calibration prior is emitted as a typed `Calibration` effect so
    /// replay can retain its evidence class, while a forward activation is a
    /// mutation effect.  Both describe the same known field value for merge
    /// identity and must not manufacture a conflict merely because the replay
    /// event carries different type metadata.
    fn canonical_identity(&self) -> Self {
        match self {
            Self::Calibration {
                map_id,
                calibration: Evidence::Known(calibration_id),
            } => Self::Mutation(Mutation::ActivateCalibration {
                map_id: *map_id,
                calibration_id: *calibration_id,
            }),
            Self::Mutation(mutation) => Self::Mutation(mutation.clone()),
            Self::Calibration {
                map_id,
                calibration: Evidence::Unknown(reason),
            } => Self::Calibration {
                map_id: *map_id,
                calibration: Evidence::Unknown(reason.clone()),
            },
        }
    }
}

impl FrontierIdentity {
    fn semantically_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Value(left), Self::Value(right)) => left == right,
            (
                Self::Toggle {
                    target: left_target,
                    direction: left_direction,
                    ..
                },
                Self::Toggle {
                    target: right_target,
                    direction: right_direction,
                    ..
                },
            ) => left_target == right_target && left_direction == right_direction,
            (Self::Toggle { effect: toggle, .. }, Self::Value(value))
            | (Self::Value(value), Self::Toggle { effect: toggle, .. }) => toggle == value,
        }
    }
}

fn frontier_identity(
    operation: &Operation,
    effect: &EffectValue,
) -> (FrontierIdentity, ConflictIntent) {
    match operation.payload() {
        OperationPayload::Undo { target } => (
            FrontierIdentity::Toggle {
                target: *target,
                direction: ToggleDirection::Undo,
                effect: effect.canonical_identity(),
            },
            ConflictIntent::Toggle {
                target: *target,
                direction: ToggleDirection::Undo,
            },
        ),
        OperationPayload::Redo { target } => (
            FrontierIdentity::Toggle {
                target: *target,
                direction: ToggleDirection::Redo,
                effect: effect.canonical_identity(),
            },
            ConflictIntent::Toggle {
                target: *target,
                direction: ToggleDirection::Redo,
            },
        ),
        OperationPayload::Apply { .. } | OperationPayload::Resolve { .. } => (
            FrontierIdentity::Value(effect.canonical_identity()),
            ConflictIntent::Value,
        ),
    }
}

/// A validated DAG of immutable operations suitable for offline merge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationSet {
    project_id: ProjectId,
    operations: BTreeMap<OperationId, Operation>,
}

impl OperationSet {
    pub fn empty(project_id: ProjectId) -> Self {
        Self {
            project_id,
            operations: BTreeMap::new(),
        }
    }

    pub fn from_operations<I>(operations: I) -> Result<Self, OperationError>
    where
        I: IntoIterator<Item = Operation>,
    {
        let mut project_id = None;
        let mut map: BTreeMap<OperationId, Operation> = BTreeMap::new();
        for operation in operations {
            if let Some(expected) = project_id {
                if operation.project_id() != expected {
                    return Err(OperationError::WrongProject);
                }
            } else {
                project_id = Some(operation.project_id());
            }
            if let Some(existing) = map.get(&operation.operation_id()) {
                if existing.content_hash() != operation.content_hash() {
                    return Err(OperationError::TamperedDuplicate);
                }
                continue;
            }
            if map.len() >= MAX_OPERATION_COUNT {
                return Err(OperationError::ResourceLimit("operation_count"));
            }
            map.insert(operation.operation_id(), operation);
        }
        let project_id = project_id.ok_or(OperationError::EmptyOperationSet)?;
        let result = Self {
            project_id,
            operations: map,
        };
        result.validate_graph()?;
        Ok(result)
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub fn operation(&self, id: OperationId) -> Option<&Operation> {
        self.operations.get(&id)
    }

    pub fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.operations.values()
    }

    /// Topological order is deterministic among ready operations by logical
    /// time, actor, device, and operation ID. Causal parents always precede
    /// their descendants even when their logical timestamps tie.
    pub fn ordered(&self) -> Result<Vec<&Operation>, OperationError> {
        let mut indegree = BTreeMap::new();
        let mut children: BTreeMap<OperationId, Vec<OperationId>> = BTreeMap::new();
        for operation in self.operations.values() {
            indegree.insert(operation.operation_id(), operation.parents().len());
            for parent in operation.parents() {
                children
                    .entry(*parent)
                    .or_default()
                    .push(operation.operation_id());
            }
        }
        let mut ready = BinaryHeap::new();
        for operation in self.operations.values() {
            if indegree[&operation.operation_id()] == 0 {
                ready.push(Reverse(operation.sort_key()));
            }
        }
        let mut result = Vec::with_capacity(self.operations.len());
        while let Some(Reverse((_, _, _, id))) = ready.pop() {
            let operation = self
                .operations
                .get(&id)
                .ok_or(OperationError::MissingParent)?;
            result.push(operation);
            if let Some(descendants) = children.get(&id) {
                for child in descendants {
                    let degree = indegree
                        .get_mut(child)
                        .ok_or(OperationError::MissingParent)?;
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(Reverse(
                            self.operations
                                .get(child)
                                .ok_or(OperationError::MissingParent)?
                                .sort_key(),
                        ));
                    }
                }
            }
        }
        if result.len() != self.operations.len() {
            return Err(OperationError::CyclicCausality);
        }
        Ok(result)
    }

    /// Merge is set union plus explicit semantic conflict records. The
    /// ordered set is available for inspection, but `into_applyable` refuses
    /// to treat conflicting values as resolved.
    pub fn merge(&self, other: &Self) -> Result<MergeOutcome, MergeError> {
        if self.project_id != other.project_id {
            return Err(MergeError::WrongProject);
        }
        let mut map = self.operations.clone();
        for operation in other.operations.values() {
            if let Some(existing) = map.get(&operation.operation_id()) {
                if existing.content_hash() != operation.content_hash() {
                    return Err(MergeError::TamperedDuplicate);
                }
                continue;
            }
            if map.len() >= MAX_OPERATION_COUNT {
                return Err(MergeError::ResourceLimit("operation_count"));
            }
            map.insert(operation.operation_id(), operation.clone());
        }
        let merged = Self {
            project_id: self.project_id,
            operations: map,
        };
        merged.validate_graph().map_err(MergeError::Operation)?;
        let conflicts = merged.conflicts()?;
        Ok(MergeOutcome { merged, conflicts })
    }

    /// Replay using the original mutation-only V1 effect boundary. Callers
    /// that need V2 unknown calibration state must use [`Self::replay_effects`].
    pub fn replay(&self) -> Result<Vec<AppliedMutation>, MergeError> {
        let conflicts = self.conflicts()?;
        if !conflicts.is_empty() {
            return Err(MergeError::Conflicts(conflicts));
        }
        Ok(self.replay_without_conflict_check()?.0)
    }

    /// Replay with the typed effect boundary required by V2. An unknown
    /// calibration prior is returned as `AppliedEffect::Calibration` instead
    /// of being coerced into a fake identifier or omitted from the replay.
    pub fn replay_effects(&self) -> Result<Vec<AppliedEffect>, MergeError> {
        let conflicts = self.conflicts()?;
        if !conflicts.is_empty() {
            return Err(MergeError::Conflicts(conflicts));
        }
        Ok(self.replay_effects_without_conflict_check()?.0)
    }

    /// Validate replayable toggle state without requiring semantic field
    /// conflicts to be resolved. Admission uses this before persistence so an
    /// unrelated unresolved edit conflict cannot mask a repeated sequential
    /// undo or redo. The returned state is intentionally discarded: callers
    /// that need mutations must use [`Self::replay`], which still refuses to
    /// apply a conflicting set.
    pub fn validate_replay_semantics(&self) -> Result<(), MergeError> {
        self.replay_effects_without_conflict_check().map(|_| ())
    }

    fn replay_without_conflict_check(
        &self,
    ) -> Result<(Vec<AppliedMutation>, BTreeMap<OperationId, bool>), MergeError> {
        let (effects, active) = self.replay_effects_without_conflict_check()?;
        let mutations = effects
            .into_iter()
            .map(AppliedEffect::into_mutation)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((mutations, active))
    }

    fn replay_effects_without_conflict_check(
        &self,
    ) -> Result<(Vec<AppliedEffect>, BTreeMap<OperationId, bool>), MergeError> {
        let mut active = BTreeMap::new();
        let mut toggle_history: BTreeMap<(OperationReference, ToggleDirection), Vec<OperationId>> =
            BTreeMap::new();
        let mut ancestry_work = MAX_ANCESTRY_WORK;
        let mut result = Vec::with_capacity(self.operations.len());
        for operation in self.ordered().map_err(MergeError::Operation)? {
            match operation.payload() {
                OperationPayload::Apply { mutation } => {
                    active.insert(operation.operation_id(), true);
                    result.push(AppliedEffect::Mutation(AppliedMutation {
                        operation_id: operation.operation_id(),
                        mutation: mutation.clone(),
                    }));
                }
                OperationPayload::Undo { target } => {
                    if active.get(&target.operation_id()) != Some(&true) {
                        let intent = (*target, ToggleDirection::Undo);
                        let prior_toggles =
                            toggle_history.get(&intent).cloned().unwrap_or_default();
                        if !prior_toggles.is_empty() {
                            let mut sequential = false;
                            for prior in prior_toggles {
                                if self
                                    .ancestor_with_budget(
                                        prior,
                                        operation.operation_id(),
                                        &mut ancestry_work,
                                    )
                                    .map_err(MergeError::Operation)?
                                {
                                    sequential = true;
                                    break;
                                }
                            }
                            if sequential {
                                return Err(MergeError::InvalidToggle);
                            }
                            toggle_history
                                .entry(intent)
                                .or_default()
                                .push(operation.operation_id());
                            continue;
                        }
                        return Err(MergeError::InvalidToggle);
                    }
                    let target_operation = self
                        .operations
                        .get(&target.operation_id())
                        .ok_or(MergeError::Operation(OperationError::MissingTarget))?;
                    let effect = target_operation
                        .inverse()
                        .as_applied_effect(operation.operation_id())
                        .map_err(MergeError::Operation)?;
                    active.insert(target.operation_id(), false);
                    toggle_history
                        .entry((*target, ToggleDirection::Undo))
                        .or_default()
                        .push(operation.operation_id());
                    result.push(effect);
                }
                OperationPayload::Redo { target } => {
                    if active.get(&target.operation_id()) != Some(&false) {
                        let intent = (*target, ToggleDirection::Redo);
                        let prior_toggles =
                            toggle_history.get(&intent).cloned().unwrap_or_default();
                        if !prior_toggles.is_empty() {
                            let mut sequential = false;
                            for prior in prior_toggles {
                                if self
                                    .ancestor_with_budget(
                                        prior,
                                        operation.operation_id(),
                                        &mut ancestry_work,
                                    )
                                    .map_err(MergeError::Operation)?
                                {
                                    sequential = true;
                                    break;
                                }
                            }
                            if sequential {
                                return Err(MergeError::InvalidToggle);
                            }
                            toggle_history
                                .entry(intent)
                                .or_default()
                                .push(operation.operation_id());
                            continue;
                        }
                        return Err(MergeError::InvalidToggle);
                    }
                    let target_operation = self
                        .operations
                        .get(&target.operation_id())
                        .ok_or(MergeError::Operation(OperationError::MissingTarget))?;
                    let OperationPayload::Apply { mutation } = target_operation.payload() else {
                        return Err(MergeError::InvalidToggle);
                    };
                    active.insert(target.operation_id(), true);
                    toggle_history
                        .entry((*target, ToggleDirection::Redo))
                        .or_default()
                        .push(operation.operation_id());
                    result.push(AppliedEffect::Mutation(AppliedMutation {
                        operation_id: operation.operation_id(),
                        mutation: mutation.clone(),
                    }));
                }
                OperationPayload::Resolve { mutation, .. } => {
                    result.push(AppliedEffect::Mutation(AppliedMutation {
                        operation_id: operation.operation_id(),
                        mutation: mutation.clone(),
                    }));
                }
            }
        }
        Ok((result, active))
    }

    pub fn replay_state(&self) -> Result<BTreeMap<FieldKey, Mutation>, MergeError> {
        let mut state = BTreeMap::new();
        for applied in self.replay()? {
            state.insert(applied.mutation.field_key(), applied.mutation);
        }
        Ok(state)
    }

    fn validate_graph(&self) -> Result<(), OperationError> {
        let mut ancestry_work = MAX_ANCESTRY_WORK;
        for operation in self.operations.values() {
            let mut max_parent_depth = None;
            for parent_id in operation.parents() {
                if *parent_id == operation.operation_id() {
                    return Err(OperationError::CyclicCausality);
                }
                let parent = self
                    .operations
                    .get(parent_id)
                    .ok_or(OperationError::MissingParent)?;
                if parent.logical_time() >= operation.logical_time() {
                    return Err(OperationError::InvalidCausality);
                }
                max_parent_depth = Some(
                    max_parent_depth.map_or(parent.causal_depth(), |depth: CausalDepth| {
                        depth.max(parent.causal_depth())
                    }),
                );
            }
            for (index, left) in operation.parents().iter().enumerate() {
                for right in operation.parents().iter().skip(index + 1) {
                    if self.ancestor_with_budget(*left, *right, &mut ancestry_work)?
                        || self.ancestor_with_budget(*right, *left, &mut ancestry_work)?
                    {
                        return Err(OperationError::InvalidCausality);
                    }
                }
            }
            match max_parent_depth {
                None if operation.causal_depth().value() != 0 => {
                    return Err(OperationError::InvalidCausality);
                }
                None => {}
                Some(version) => {
                    let expected = version
                        .value()
                        .checked_add(1)
                        .ok_or(OperationError::ResourceLimit("causal_depth"))?;
                    if operation.causal_depth().value() != expected {
                        return Err(OperationError::InvalidCausality);
                    }
                }
            }
            if let Some(target) = operation.target_reference() {
                let target_operation = self
                    .operations
                    .get(&target.operation_id())
                    .ok_or(OperationError::MissingTarget)?;
                if target_operation.content_hash() != target.content_hash() {
                    return Err(OperationError::HashMismatch);
                }
                if target_operation.schema_version() != operation.schema_version() {
                    return Err(OperationError::VersionMismatch);
                }
                if !matches!(target_operation.payload(), OperationPayload::Apply { .. })
                    || !self.ancestor_with_budget(
                        target.operation_id(),
                        operation.operation_id(),
                        &mut ancestry_work,
                    )?
                {
                    return Err(OperationError::InvalidCausality);
                }
                if matches!(
                    target_operation.inverse(),
                    InverseMetadata::NonReversible { .. }
                ) {
                    return Err(OperationError::NonReversibleTarget);
                }
            }
            if let Some((left, right)) = operation.resolution_references() {
                self.validate_resolution(operation, left, right, &mut ancestry_work)?;
            }
        }
        self.ordered().map(|_| ())
    }

    fn validate_resolution(
        &self,
        operation: &Operation,
        left: OperationReference,
        right: OperationReference,
        ancestry_work: &mut usize,
    ) -> Result<(), OperationError> {
        if left.operation_id() == right.operation_id() {
            return Err(OperationError::InvalidResolution);
        }
        if left.operation_id() >= right.operation_id()
            || !operation.parents().contains(&left.operation_id())
            || !operation.parents().contains(&right.operation_id())
        {
            return Err(OperationError::InvalidResolution);
        }
        let left_operation = self
            .operations
            .get(&left.operation_id())
            .ok_or(OperationError::MissingTarget)?;
        let right_operation = self
            .operations
            .get(&right.operation_id())
            .ok_or(OperationError::MissingTarget)?;
        if left_operation.schema_version() != operation.schema_version()
            || right_operation.schema_version() != operation.schema_version()
        {
            return Err(OperationError::VersionMismatch);
        }
        if left_operation.content_hash() != left.content_hash()
            || right_operation.content_hash() != right.content_hash()
        {
            return Err(OperationError::HashMismatch);
        }
        if !self.ancestor_with_budget(
            left.operation_id(),
            operation.operation_id(),
            ancestry_work,
        )? || !self.ancestor_with_budget(
            right.operation_id(),
            operation.operation_id(),
            ancestry_work,
        )? || self.ancestor_with_budget(
            left.operation_id(),
            right.operation_id(),
            ancestry_work,
        )? || self.ancestor_with_budget(
            right.operation_id(),
            left.operation_id(),
            ancestry_work,
        )? {
            return Err(OperationError::InvalidResolution);
        }
        // Validate the semantic values, rather than converting a typed V2
        // effect back into the mutation-only V1 representation. In
        // particular, an unknown calibration prior is a valid conflict arm
        // and must remain inspectable and resolvable.
        let left_effect = self.effect(left_operation).map_err(|error| match error {
            MergeError::Operation(operation_error) => operation_error,
            MergeError::InvalidToggle => OperationError::InvalidResolution,
            MergeError::WrongProject
            | MergeError::TamperedDuplicate
            | MergeError::Conflicts(_)
            | MergeError::ResourceLimit(_) => OperationError::InvalidResolution,
        })?;
        let right_effect = self.effect(right_operation).map_err(|error| match error {
            MergeError::Operation(operation_error) => operation_error,
            MergeError::InvalidToggle => OperationError::InvalidResolution,
            MergeError::WrongProject
            | MergeError::TamperedDuplicate
            | MergeError::Conflicts(_)
            | MergeError::ResourceLimit(_) => OperationError::InvalidResolution,
        })?;
        let OperationPayload::Resolve { mutation, .. } = operation.payload() else {
            return Err(OperationError::InvalidResolution);
        };
        if left_effect.field_key() != right_effect.field_key()
            || left_effect.field_key() != mutation.field_key()
            || left_effect.canonical_identity() == right_effect.canonical_identity()
        {
            return Err(OperationError::InvalidResolution);
        }
        Ok(())
    }

    fn ancestor_with_budget(
        &self,
        ancestor: OperationId,
        descendant: OperationId,
        work: &mut usize,
    ) -> Result<bool, OperationError> {
        if ancestor == descendant {
            return Ok(false);
        }
        let mut pending = vec![descendant];
        let mut visited = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            if *work == 0 {
                return Err(OperationError::ResourceLimit("ancestry_work"));
            }
            *work -= 1;
            let Some(operation) = self.operations.get(&current) else {
                continue;
            };
            for parent in operation.parents() {
                if *parent == ancestor {
                    return Ok(true);
                }
                pending.push(*parent);
            }
        }
        Ok(false)
    }

    fn conflicts(&self) -> Result<Vec<MergeConflict>, MergeError> {
        #[derive(Clone)]
        struct FrontierEntry {
            operation_id: OperationId,
            reference: OperationReference,
            identity: FrontierIdentity,
            intent: ConflictIntent,
            event: AppliedEffect,
        }

        type ConflictKey = (OperationId, OperationId);
        let mut frontier: BTreeMap<FieldKey, Vec<FrontierEntry>> = BTreeMap::new();
        let mut conflicts: BTreeMap<ConflictKey, MergeConflict> = BTreeMap::new();
        let mut ancestry_work = MAX_ANCESTRY_WORK;
        for operation in self.ordered().map_err(MergeError::Operation)? {
            if let Some((left, right)) = operation.resolution_references() {
                conflicts.remove(&(left.operation_id(), right.operation_id()));
            }
            let event = self.effect_event(operation)?;
            let effect = event.as_effect_value();
            let (identity, intent) = frontier_identity(operation, &effect);
            let key = effect.field_key();
            let previous = frontier.remove(&key).unwrap_or_default();
            let mut next = Vec::with_capacity(previous.len().saturating_add(1));
            for entry in previous {
                if entry.identity.semantically_equal(&identity) {
                    // Equal value effects and duplicate same-target toggles
                    // share one representative. The latter is a canonical
                    // replay no-op after the first toggle.
                    self.remove_superseded_conflicts(
                        &mut conflicts,
                        entry.operation_id,
                        operation.operation_id(),
                        &mut ancestry_work,
                    )?;
                    continue;
                }
                if self
                    .ancestor_with_budget(
                        entry.operation_id,
                        operation.operation_id(),
                        &mut ancestry_work,
                    )
                    .map_err(MergeError::Operation)?
                {
                    self.remove_superseded_conflicts(
                        &mut conflicts,
                        entry.operation_id,
                        operation.operation_id(),
                        &mut ancestry_work,
                    )?;
                    continue;
                }
                let (left, right, left_intent, right_intent, left_effect, right_effect) =
                    if entry.operation_id < operation.operation_id() {
                        (
                            entry.reference,
                            OperationReference::from(operation),
                            entry.intent,
                            intent,
                            entry.event.clone(),
                            event.clone(),
                        )
                    } else {
                        (
                            OperationReference::from(operation),
                            entry.reference,
                            intent,
                            entry.intent,
                            event.clone(),
                            entry.event.clone(),
                        )
                    };
                let conflict_key = (left.operation_id(), right.operation_id());
                if !conflicts.contains_key(&conflict_key) && conflicts.len() >= MAX_CONFLICTS {
                    return Err(MergeError::ResourceLimit("merge_conflicts"));
                }
                conflicts.insert(
                    conflict_key,
                    MergeConflict {
                        field: key.clone(),
                        left,
                        right,
                        left_intent,
                        right_intent,
                        left_effect,
                        right_effect,
                    },
                );
                next.push(entry);
            }
            next.push(FrontierEntry {
                operation_id: operation.operation_id(),
                reference: OperationReference::from(operation),
                identity,
                intent,
                event,
            });
            if next.len() > MAX_MERGE_FRONTIER {
                return Err(MergeError::ResourceLimit("merge_frontier"));
            }
            frontier.insert(key, next);
        }
        Ok(conflicts.into_values().collect())
    }

    fn remove_superseded_conflicts(
        &self,
        conflicts: &mut BTreeMap<(OperationId, OperationId), MergeConflict>,
        superseded: OperationId,
        current: OperationId,
        work: &mut usize,
    ) -> Result<(), MergeError> {
        let mut keys = Vec::new();
        for key in conflicts.keys().copied() {
            if *work == 0 {
                return Err(MergeError::ResourceLimit("merge_work"));
            }
            *work -= 1;
            if key.0 == superseded || key.1 == superseded {
                keys.push(key);
            }
        }
        for key in keys {
            let other = if key.0 == superseded { key.1 } else { key.0 };
            // If both old heads are ancestors of this operation, an ordinary
            // descendant has joined them without selecting a resolution. Keep
            // that unresolved audit conflict. A later Resolve can clear only
            // its exact canonical pair.
            if self
                .ancestor_with_budget(other, current, work)
                .map_err(MergeError::Operation)?
            {
                continue;
            }
            conflicts.remove(&key);
        }
        Ok(())
    }

    fn effect(&self, operation: &Operation) -> Result<EffectValue, MergeError> {
        Ok(self.effect_event(operation)?.as_effect_value())
    }

    fn effect_event(&self, operation: &Operation) -> Result<AppliedEffect, MergeError> {
        match operation.payload() {
            OperationPayload::Apply { mutation } => Ok(AppliedEffect::Mutation(AppliedMutation {
                operation_id: operation.operation_id(),
                mutation: mutation.clone(),
            })),
            OperationPayload::Undo { target } => {
                let target_operation = self
                    .operations
                    .get(&target.operation_id())
                    .ok_or(MergeError::Operation(OperationError::MissingTarget))?;
                target_operation
                    .inverse()
                    .as_applied_effect(operation.operation_id())
                    .map_err(MergeError::Operation)
            }
            OperationPayload::Redo { target } => {
                let target_operation = self
                    .operations
                    .get(&target.operation_id())
                    .ok_or(MergeError::Operation(OperationError::MissingTarget))?;
                let OperationPayload::Apply { mutation } = target_operation.payload() else {
                    return Err(MergeError::InvalidToggle);
                };
                Ok(AppliedEffect::Mutation(AppliedMutation {
                    operation_id: operation.operation_id(),
                    mutation: mutation.clone(),
                }))
            }
            OperationPayload::Resolve { mutation, .. } => {
                Ok(AppliedEffect::Mutation(AppliedMutation {
                    operation_id: operation.operation_id(),
                    mutation: mutation.clone(),
                }))
            }
        }
    }
}

/// One mutation event emitted by deterministic replay. Consumers can apply
/// these to their own aggregate through an application-layer port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedMutation {
    operation_id: OperationId,
    mutation: Mutation,
}

impl AppliedMutation {
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    pub const fn mutation(&self) -> &Mutation {
        &self.mutation
    }
}

/// One typed effect emitted by deterministic replay. V1 consumers can use
/// [`OperationSet::replay`], while V2 consumers use this boundary to retain
/// an explicitly unknown calibration prior during undo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AppliedEffect {
    Mutation(AppliedMutation),
    Calibration {
        operation_id: OperationId,
        map_id: MapAssetId,
        calibration: Evidence<CalibrationId>,
    },
}

impl AppliedEffect {
    pub const fn operation_id(&self) -> OperationId {
        match self {
            Self::Mutation(applied) => applied.operation_id,
            Self::Calibration { operation_id, .. } => *operation_id,
        }
    }

    pub fn field_key(&self) -> FieldKey {
        match self {
            Self::Mutation(applied) => applied.mutation.field_key(),
            Self::Calibration { map_id, .. } => FieldKey::MapCalibration(*map_id),
        }
    }

    pub const fn map_id(&self) -> Option<MapAssetId> {
        match self {
            Self::Mutation(_) => None,
            Self::Calibration { map_id, .. } => Some(*map_id),
        }
    }

    pub const fn mutation(&self) -> Option<&Mutation> {
        match self {
            Self::Mutation(applied) => Some(&applied.mutation),
            Self::Calibration { .. } => None,
        }
    }

    pub const fn calibration(&self) -> Option<&Evidence<CalibrationId>> {
        match self {
            Self::Mutation(_) => None,
            Self::Calibration { calibration, .. } => Some(calibration),
        }
    }

    fn as_effect_value(&self) -> EffectValue {
        match self {
            Self::Mutation(applied) => EffectValue::Mutation(applied.mutation.clone()),
            Self::Calibration {
                map_id,
                calibration,
                ..
            } => EffectValue::Calibration {
                map_id: *map_id,
                calibration: calibration.clone(),
            },
        }
    }

    fn into_mutation(self) -> Result<AppliedMutation, MergeError> {
        match self {
            Self::Mutation(applied) => Ok(applied),
            Self::Calibration {
                operation_id,
                map_id,
                calibration: Evidence::Known(calibration_id),
            } => Ok(AppliedMutation {
                operation_id,
                mutation: Mutation::ActivateCalibration {
                    map_id,
                    calibration_id,
                },
            }),
            Self::Calibration {
                calibration: Evidence::Unknown(_),
                ..
            } => Err(MergeError::Operation(OperationError::TypedPriorRequired)),
        }
    }
}

impl InverseMetadata {
    fn as_applied_effect(
        &self,
        operation_id: OperationId,
    ) -> Result<AppliedEffect, OperationError> {
        match self {
            Self::Apply { mutation } => Ok(AppliedEffect::Mutation(AppliedMutation {
                operation_id,
                mutation: mutation.clone(),
            })),
            Self::ApplyV2 { prior } => match prior {
                InversePrior::MapCalibration {
                    map_id,
                    calibration,
                    ..
                } => Ok(AppliedEffect::Calibration {
                    operation_id,
                    map_id: *map_id,
                    calibration: calibration.clone(),
                }),
                InversePrior::ProjectName { .. } | InversePrior::SiteName { .. } => {
                    Ok(AppliedEffect::Mutation(AppliedMutation {
                        operation_id,
                        mutation: prior.as_mutation()?,
                    }))
                }
            },
            Self::NonReversible { .. } => Err(OperationError::NonReversibleTarget),
            Self::Toggle { .. } => Err(OperationError::InvalidInverse),
        }
    }
}

/// A semantic concurrent edit conflict. Both immutable operation references
/// and typed effects are retained so a UI/application can ask the user to
/// choose explicitly or create a new resolving operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ConflictIntent {
    Value,
    Toggle {
        target: OperationReference,
        direction: ToggleDirection,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeConflict {
    field: FieldKey,
    left: OperationReference,
    right: OperationReference,
    left_intent: ConflictIntent,
    right_intent: ConflictIntent,
    left_effect: AppliedEffect,
    right_effect: AppliedEffect,
}

impl MergeConflict {
    pub const fn field(&self) -> &FieldKey {
        &self.field
    }

    pub const fn left(&self) -> OperationReference {
        self.left
    }

    pub const fn right(&self) -> OperationReference {
        self.right
    }

    pub const fn left_intent(&self) -> &ConflictIntent {
        &self.left_intent
    }

    pub const fn right_intent(&self) -> &ConflictIntent {
        &self.right_intent
    }

    /// The complete typed effect represented by the left operation. Unknown
    /// calibration state is retained here instead of being coerced into a
    /// fake identifier.
    pub const fn left_effect(&self) -> &AppliedEffect {
        &self.left_effect
    }

    /// The complete typed effect represented by the right operation.
    pub const fn right_effect(&self) -> &AppliedEffect {
        &self.right_effect
    }

    /// Compatibility view for callers that only handle mutation effects.
    /// `None` is an honest result for a typed unknown calibration effect.
    pub const fn left_mutation(&self) -> Option<&Mutation> {
        self.left_effect.mutation()
    }

    /// Compatibility view for callers that only handle mutation effects.
    /// `None` is an honest result for a typed unknown calibration effect.
    pub const fn right_mutation(&self) -> Option<&Mutation> {
        self.right_effect.mutation()
    }
}

/// Union output; callers must inspect conflicts before application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeOutcome {
    merged: OperationSet,
    conflicts: Vec<MergeConflict>,
}

impl MergeOutcome {
    pub fn merged(&self) -> &OperationSet {
        &self.merged
    }

    pub fn conflicts(&self) -> &[MergeConflict] {
        &self.conflicts
    }

    pub fn into_applyable(self) -> Result<OperationSet, MergeError> {
        if self.conflicts.is_empty() {
            Ok(self.merged)
        } else {
            Err(MergeError::Conflicts(self.conflicts))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeError {
    Operation(OperationError),
    WrongProject,
    TamperedDuplicate,
    Conflicts(Vec<MergeConflict>),
    InvalidToggle,
    ResourceLimit(&'static str),
}

impl fmt::Display for MergeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MergeError {}

/// Errors are intentionally coarse at parser boundaries: upstream serde or
/// storage details do not become canonical domain contracts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationError {
    InvalidValue(ValidationError),
    InvalidLogicalTimestamp,
    InvalidCausality,
    InvalidInverse,
    TypedPriorRequired,
    NonReversibleTarget,
    VersionMismatch,
    UnsupportedSchema,
    MalformedEncoding,
    CanonicalEncoding,
    NonCanonicalEncoding,
    HashMismatch,
    EmptyOperationSet,
    WrongProject,
    MissingParent,
    MissingTarget,
    CyclicCausality,
    TamperedDuplicate,
    InvalidResolution,
    ResourceLimit(&'static str),
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OperationError {}

impl From<ValidationError> for OperationError {
    fn from(value: ValidationError) -> Self {
        Self::InvalidValue(value)
    }
}
