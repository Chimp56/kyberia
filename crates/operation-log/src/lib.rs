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
use kyberia_resource_budget::{
    BudgetKind, CancellationHook, ResourceBudget, ResourceBudgetError, ResourceLimits,
    ResourceUsage,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    fmt,
    io::{self, Write},
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
const ESTIMATED_STRUCTURAL_ENTRY_BYTES: usize = 128;
// Per-node ordering policy: two map entries including tree-node overhead,
// one sort-key heap slot and one result pointer, with capacity slack. Each
// edge reserves a 16-byte child ID plus vector growth/allocator slack.
const ORDERING_NODE_BYTES: usize = 512;
const ORDERING_EDGE_BYTES: usize = 64;
pub const DEFAULT_MERGE_OPERATION_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_MERGE_WORKING_BYTES: usize = 512 * 1024 * 1024;

const fn estimated_structural_bytes(entries: usize) -> usize {
    entries.saturating_mul(ESTIMATED_STRUCTURAL_ENTRY_BYTES)
}

fn default_operation_budget() -> ResourceBudget {
    default_operation_budget_with_ancestry(MAX_ANCESTRY_WORK)
}

fn default_operation_budget_with_ancestry(ancestry_work: usize) -> ResourceBudget {
    ResourceBudget::new(ResourceLimits::new(
        ancestry_work,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    ))
}

fn operation_budget_error(error: ResourceBudgetError) -> OperationError {
    match error {
        ResourceBudgetError::Cancelled => OperationError::Cancelled,
        ResourceBudgetError::LimitExceeded(limit) => {
            let label = match limit.kind() {
                BudgetKind::OperationAncestryWork => "ancestry_work",
                kind => kind.label(),
            };
            OperationError::ResourceLimit(label)
        }
    }
}

fn merge_budget_error(error: ResourceBudgetError) -> MergeError {
    match error {
        ResourceBudgetError::Cancelled => MergeError::Cancelled,
        ResourceBudgetError::LimitExceeded(limit) => {
            let label = match limit.kind() {
                BudgetKind::OperationAncestryWork => "ancestry_work",
                kind => kind.label(),
            };
            MergeError::ResourceLimit(label)
        }
    }
}

fn resolution_effect_error(error: MergeError) -> OperationError {
    match error {
        MergeError::Operation(operation_error) => operation_error,
        MergeError::InvalidToggle => OperationError::InvalidResolution,
        MergeError::ResourceLimit(label) => OperationError::ResourceLimit(label),
        MergeError::Cancelled => OperationError::Cancelled,
        MergeError::WrongProject | MergeError::TamperedDuplicate | MergeError::Conflicts(_) => {
            OperationError::InvalidResolution
        }
    }
}

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

    fn validate_for_resolution(&self, value: &ResolutionValue) -> Result<(), OperationError> {
        match value {
            ResolutionValue::Mutation(mutation) => self.validate_for(mutation),
            ResolutionValue::Calibration {
                map_id,
                calibration: selected_calibration,
            } => {
                if !matches!(
                    selected_calibration,
                    Evidence::Known(_) | Evidence::Unknown(UnknownReason::NotMeasured)
                ) {
                    return Err(OperationError::InvalidInverse);
                }
                match self {
                    Self::MapCalibration {
                        map_id: prior_map_id,
                        calibration,
                    } if map_id == prior_map_id
                        && matches!(
                            calibration,
                            Evidence::Known(_) | Evidence::Unknown(UnknownReason::NotMeasured)
                        ) =>
                    {
                        Ok(())
                    }
                    _ => Err(OperationError::InvalidInverse),
                }
            }
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

/// A V2 resolution value. Unlike the V1 `Resolve` payload, this closed value
/// can select an explicitly unknown calibration state without inventing a
/// calibration identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ResolutionValue {
    Mutation(Mutation),
    Calibration {
        map_id: MapAssetId,
        calibration: Evidence<CalibrationId>,
    },
}

impl ResolutionValue {
    pub fn activate_calibration(map_id: MapAssetId, calibration: Evidence<CalibrationId>) -> Self {
        Self::Calibration {
            map_id,
            calibration,
        }
    }

    pub const fn field_key(&self) -> FieldKey {
        match self {
            Self::Mutation(mutation) => mutation.field_key(),
            Self::Calibration { map_id, .. } => FieldKey::MapCalibration(*map_id),
        }
    }
}

impl From<Mutation> for ResolutionValue {
    fn from(value: Mutation) -> Self {
        Self::Mutation(value)
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
    /// V2 resolution payload retaining the selected typed value.
    ResolveV2 {
        left: OperationReference,
        right: OperationReference,
        value: ResolutionValue,
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

#[derive(Serialize)]
struct OperationWireRef<'a> {
    schema_version: OperationSchemaVersion,
    operation_id: OperationId,
    project_id: ProjectId,
    actor_id: ActorId,
    device_id: DeviceId,
    logical_time: LogicalTimestamp,
    causal_depth: CausalDepth,
    parents: &'a [OperationId],
    payload: &'a OperationPayload,
    inverse: &'a InverseMetadata,
    content_hash: ContentHash,
}

struct CountingWriter {
    length: usize,
    limit_exceeded: bool,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .length
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("operation wire length overflow"))?;
        if next > MAX_OPERATION_WIRE_BYTES {
            self.limit_exceeded = true;
            return Err(io::Error::other("operation wire length limit"));
        }
        self.length = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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

    /// Construct a V2 resolution with a typed selected value and inverse
    /// prior. Passing a [`Mutation`] remains supported for known V2 values;
    /// callers that need to select an explicit unknown calibration use
    /// [`ResolutionValue::Calibration`] (or
    /// [`ResolutionValue::activate_calibration`]).
    #[allow(clippy::too_many_arguments)]
    pub fn try_resolve_v2<V: Into<ResolutionValue>>(
        operation_id: OperationId,
        project_id: ProjectId,
        actor_id: ActorId,
        device_id: DeviceId,
        logical_time: LogicalTimestamp,
        causal_depth: CausalDepth,
        parents: Vec<OperationId>,
        left: OperationReference,
        right: OperationReference,
        value: V,
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
            OperationPayload::ResolveV2 {
                left,
                right,
                value: value.into(),
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
        let wire = OperationWireRef {
            schema_version: self.schema_version,
            operation_id: self.operation_id,
            project_id: self.project_id,
            actor_id: self.actor_id,
            device_id: self.device_id,
            logical_time: self.logical_time,
            causal_depth: self.causal_depth,
            parents: &self.parents,
            payload: &self.payload,
            inverse: &self.inverse,
            content_hash: self.content_hash,
        };
        let bytes = serde_json::to_vec(&wire).map_err(|_| OperationError::CanonicalEncoding)?;
        if bytes.len() > MAX_OPERATION_WIRE_BYTES {
            return Err(OperationError::ResourceLimit("operation_wire_bytes"));
        }
        Ok(bytes)
    }

    /// Return the exact canonical wire length without allocating the wire
    /// buffer. This is used to precharge an outer resource budget before
    /// identity encoding retains each operation's bytes.
    pub fn wire_bytes_len(&self) -> Result<usize, OperationError> {
        let mut writer = CountingWriter {
            length: 0,
            limit_exceeded: false,
        };
        let wire = OperationWireRef {
            schema_version: self.schema_version,
            operation_id: self.operation_id,
            project_id: self.project_id,
            actor_id: self.actor_id,
            device_id: self.device_id,
            logical_time: self.logical_time,
            causal_depth: self.causal_depth,
            parents: &self.parents,
            payload: &self.payload,
            inverse: &self.inverse,
            content_hash: self.content_hash,
        };
        if serde_json::to_writer(&mut writer, &wire).is_err() {
            return Err(if writer.limit_exceeded {
                OperationError::ResourceLimit("operation_wire_bytes")
            } else {
                OperationError::CanonicalEncoding
            });
        }
        Ok(writer.length)
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
            OperationPayload::Resolve { .. } | OperationPayload::ResolveV2 { .. } => None,
        }
    }

    pub const fn resolution_references(&self) -> Option<(OperationReference, OperationReference)> {
        match self.payload {
            OperationPayload::Resolve { left, right, .. }
            | OperationPayload::ResolveV2 { left, right, .. } => Some((left, right)),
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
            (OperationPayload::ResolveV2 { value, .. }, InverseMetadata::ApplyV2 { prior }) => {
                prior.validate_for_resolution(value)?;
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
            OperationPayload::Resolve { .. } | OperationPayload::ResolveV2 { .. } => {}
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
        if matches!(
            operation.payload(),
            OperationPayload::Resolve { .. } | OperationPayload::ResolveV2 { .. }
        ) {
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
            OperationPayload::Apply { .. }
            | OperationPayload::Resolve { .. }
            | OperationPayload::ResolveV2 { .. } => {}
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

impl ResolutionValue {
    fn applied_effect(&self, operation_id: OperationId) -> AppliedEffect {
        match self {
            Self::Mutation(mutation) => AppliedEffect::Mutation(AppliedMutation {
                operation_id,
                mutation: mutation.clone(),
            }),
            Self::Calibration {
                map_id,
                calibration,
            } => AppliedEffect::Calibration {
                operation_id,
                map_id: *map_id,
                calibration: calibration.clone(),
            },
        }
    }
}

impl FrontierIdentity {
    fn value(&self) -> &EffectValue {
        match self {
            Self::Value(value) | Self::Toggle { effect: value, .. } => value,
        }
    }

    fn is_value(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    fn same_toggle_intent(&self, other: &Self) -> bool {
        match (self, other) {
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
            _ => false,
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
        OperationPayload::Apply { .. }
        | OperationPayload::Resolve { .. }
        | OperationPayload::ResolveV2 { .. } => (
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
        let mut budget = default_operation_budget();
        Self::from_operations_with_budget(operations, &mut budget)
    }

    /// Construct and validate a set while charging graph work and owned
    /// operation payload bytes to a caller-owned cumulative budget.
    pub fn from_operations_with_budget<I, H>(
        operations: I,
        budget: &mut ResourceBudget<H>,
    ) -> Result<Self, OperationError>
    where
        I: IntoIterator<Item = Operation>,
        H: CancellationHook,
    {
        budget.check_cancelled().map_err(operation_budget_error)?;
        let mut local_usage = ResourceUsage::default();
        let mut project_id = None;
        let mut map: BTreeMap<OperationId, Operation> = BTreeMap::new();
        let mut input_count = 0usize;
        for operation in operations {
            budget.check_cancelled().map_err(operation_budget_error)?;
            input_count = input_count
                .checked_add(1)
                .ok_or(OperationError::ResourceLimit("operation_count"))?;
            if input_count > MAX_OPERATION_COUNT {
                return Err(OperationError::ResourceLimit("operation_count"));
            }
            budget
                .charge(
                    BudgetKind::WorkingSetBytes,
                    ESTIMATED_STRUCTURAL_ENTRY_BYTES,
                )
                .map_err(operation_budget_error)?;
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
            budget
                .charge(
                    BudgetKind::OperationBytes,
                    operation.canonical_bytes().len(),
                )
                .map_err(operation_budget_error)?;
            map.insert(operation.operation_id(), operation);
        }
        let project_id = project_id.ok_or(OperationError::EmptyOperationSet)?;
        let result = Self {
            project_id,
            operations: map,
        };
        result.validate_graph_with_budget(budget, &mut local_usage)?;
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
        let mut budget = default_operation_budget();
        self.ordered_with_budget(&mut budget)
    }

    /// Topological ordering with cumulative allocation admission and
    /// cooperative cancellation. Node accounting reserves both maps, the
    /// ready heap, result pointers and collection overhead; edge accounting
    /// reserves child vectors including capacity growth. These are proxies,
    /// not an allocator-specific resident-memory bound.
    pub fn ordered_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<Vec<&Operation>, OperationError> {
        budget.check_cancelled().map_err(operation_budget_error)?;
        for operation in self.operations.values() {
            let bytes = operation
                .parents()
                .len()
                .checked_mul(ORDERING_EDGE_BYTES)
                .and_then(|edges| edges.checked_add(ORDERING_NODE_BYTES))
                .ok_or(OperationError::ResourceLimit("working_set_bytes"))?;
            budget
                .charge(BudgetKind::WorkingSetBytes, bytes)
                .map_err(operation_budget_error)?;
        }
        let mut indegree = BTreeMap::new();
        let mut children: BTreeMap<OperationId, Vec<OperationId>> = BTreeMap::new();
        for operation in self.operations.values() {
            budget.check_cancelled().map_err(operation_budget_error)?;
            indegree.insert(operation.operation_id(), operation.parents().len());
            for parent in operation.parents() {
                children
                    .entry(*parent)
                    .or_default()
                    .push(operation.operation_id());
            }
        }
        let mut ready = BinaryHeap::with_capacity(self.operations.len());
        for operation in self.operations.values() {
            budget.check_cancelled().map_err(operation_budget_error)?;
            if indegree[&operation.operation_id()] == 0 {
                ready.push(Reverse(operation.sort_key()));
            }
        }
        let mut result = Vec::with_capacity(self.operations.len());
        while let Some(Reverse((_, _, _, id))) = ready.pop() {
            budget.check_cancelled().map_err(operation_budget_error)?;
            let operation = self
                .operations
                .get(&id)
                .ok_or(OperationError::MissingParent)?;
            result.push(operation);
            if let Some(descendants) = children.get(&id) {
                for child in descendants {
                    budget.check_cancelled().map_err(operation_budget_error)?;
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
        let mut budget = ResourceBudget::new(ResourceLimits::new(
            MAX_ANCESTRY_WORK,
            usize::MAX,
            usize::MAX,
            DEFAULT_MERGE_OPERATION_BYTES,
            usize::MAX,
            DEFAULT_MERGE_WORKING_BYTES,
        ));
        self.merge_with_budget(other, &mut budget)
    }

    /// Merge using a caller-owned cumulative budget.
    ///
    /// The operation sets are admitted into a fresh map one operation at a
    /// time. Each retained operation is charged for its canonical payload and
    /// structural map entry before cloning it, so a failed quota check never
    /// requires cloning the whole left set first. Equal duplicates are
    /// checked for tampering and do not consume retained-operation bytes.
    /// Graph validation and conflict inspection share the same budget and
    /// ancestry scope, allowing a caller to bound the complete merge rather
    /// than each phase independently.
    pub fn merge_with_budget<H: CancellationHook>(
        &self,
        other: &Self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<MergeOutcome, MergeError> {
        if self.project_id != other.project_id {
            return Err(MergeError::WrongProject);
        }

        budget.check_cancelled().map_err(merge_budget_error)?;
        let mut map = BTreeMap::new();
        for operation in self.operations.values() {
            budget.check_cancelled().map_err(merge_budget_error)?;
            if map.len() >= MAX_OPERATION_COUNT {
                return Err(MergeError::ResourceLimit("operation_count"));
            }
            charge_operation_copy(operation, budget)?;
            map.insert(operation.operation_id(), operation.clone());
        }
        for operation in other.operations.values() {
            budget.check_cancelled().map_err(merge_budget_error)?;
            if let Some(existing) = map.get(&operation.operation_id()) {
                if existing.content_hash() != operation.content_hash() {
                    return Err(MergeError::TamperedDuplicate);
                }
                continue;
            }
            if map.len() >= MAX_OPERATION_COUNT {
                return Err(MergeError::ResourceLimit("operation_count"));
            }
            charge_operation_copy(operation, budget)?;
            map.insert(operation.operation_id(), operation.clone());
        }
        let merged = Self {
            project_id: self.project_id,
            operations: map,
        };
        let mut local_usage = ResourceUsage::default();
        merged
            .validate_graph_with_budget(budget, &mut local_usage)
            .map_err(MergeError::Operation)?;
        let conflicts = merged.conflicts_with_budget(budget, &mut local_usage)?;
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
        let mut budget = default_operation_budget();
        self.replay_effects_with_budget(&mut budget)
    }

    /// Replay with a caller-owned cumulative budget. The budget may be shared
    /// by validation, conflict inspection, and several historical replays.
    pub fn replay_effects_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<Vec<AppliedEffect>, MergeError> {
        budget.check_cancelled().map_err(merge_budget_error)?;
        let mut local_usage = ResourceUsage::default();
        let conflicts = self.conflicts_with_budget(budget, &mut local_usage)?;
        if !conflicts.is_empty() {
            return Err(MergeError::Conflicts(conflicts));
        }
        Ok(self
            .replay_effects_without_conflict_check_with_budget(budget, &mut local_usage)?
            .0)
    }

    /// Validate replayable toggle state without requiring semantic field
    /// conflicts to be resolved. Admission uses this before persistence so an
    /// unrelated unresolved edit conflict cannot mask a repeated sequential
    /// undo or redo. The returned state is intentionally discarded: callers
    /// that need mutations must use [`Self::replay`], which still refuses to
    /// apply a conflicting set.
    pub fn validate_replay_semantics(&self) -> Result<(), MergeError> {
        let mut budget = default_operation_budget();
        self.validate_replay_semantics_with_budget(&mut budget)
    }

    /// Validate replay semantics using a cumulative caller-owned budget.
    pub fn validate_replay_semantics_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<(), MergeError> {
        let mut local_usage = ResourceUsage::default();
        self.replay_effects_without_conflict_check_with_budget(budget, &mut local_usage)
            .map(|_| ())
    }

    fn replay_without_conflict_check(
        &self,
    ) -> Result<(Vec<AppliedMutation>, BTreeMap<OperationId, bool>), MergeError> {
        let mut budget = default_operation_budget();
        self.replay_without_conflict_check_with_budget(&mut budget)
    }

    fn replay_without_conflict_check_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
    ) -> Result<(Vec<AppliedMutation>, BTreeMap<OperationId, bool>), MergeError> {
        let mut local_usage = ResourceUsage::default();
        let (effects, active) =
            self.replay_effects_without_conflict_check_with_budget(budget, &mut local_usage)?;
        let mutations = effects
            .into_iter()
            .map(AppliedEffect::into_mutation)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((mutations, active))
    }

    fn replay_effects_without_conflict_check_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
        local_usage: &mut ResourceUsage,
    ) -> Result<(Vec<AppliedEffect>, BTreeMap<OperationId, bool>), MergeError> {
        budget.check_cancelled().map_err(merge_budget_error)?;
        budget
            .charge(
                BudgetKind::WorkingSetBytes,
                estimated_structural_bytes(self.operations.len()),
            )
            .map_err(merge_budget_error)?;
        let mut active = BTreeMap::new();
        let mut toggle_history: BTreeMap<(OperationReference, ToggleDirection), Vec<OperationId>> =
            BTreeMap::new();
        let mut result = Vec::with_capacity(self.operations.len());
        for operation in self
            .ordered_with_budget(budget)
            .map_err(MergeError::Operation)?
        {
            budget.check_cancelled().map_err(merge_budget_error)?;
            budget
                .charge(
                    BudgetKind::WorkingSetBytes,
                    operation.canonical_bytes().len(),
                )
                .map_err(merge_budget_error)?;
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
                        let prior_toggles_len = toggle_history.get(&intent).map_or(0, Vec::len);
                        budget
                            .charge(
                                BudgetKind::WorkingSetBytes,
                                estimated_structural_bytes(prior_toggles_len),
                            )
                            .map_err(merge_budget_error)?;
                        let prior_toggles =
                            toggle_history.get(&intent).cloned().unwrap_or_default();
                        if !prior_toggles.is_empty() {
                            let mut sequential = false;
                            for prior in prior_toggles {
                                if self
                                    .ancestor_with_budget(
                                        prior,
                                        operation.operation_id(),
                                        budget,
                                        local_usage,
                                    )
                                    .map_err(merge_budget_error)?
                                {
                                    sequential = true;
                                    break;
                                }
                            }
                            if sequential {
                                return Err(MergeError::InvalidToggle);
                            }
                            budget
                                .charge(
                                    BudgetKind::WorkingSetBytes,
                                    ESTIMATED_STRUCTURAL_ENTRY_BYTES,
                                )
                                .map_err(merge_budget_error)?;
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
                    // `as_applied_effect` clones the target's inverse payload;
                    // charge that referenced payload before constructing the
                    // effect, in addition to the current toggle's wire size.
                    budget
                        .charge(
                            BudgetKind::WorkingSetBytes,
                            target_operation.canonical_bytes().len(),
                        )
                        .map_err(merge_budget_error)?;
                    let effect = target_operation
                        .inverse()
                        .as_applied_effect(operation.operation_id())
                        .map_err(MergeError::Operation)?;
                    active.insert(target.operation_id(), false);
                    budget
                        .charge(
                            BudgetKind::WorkingSetBytes,
                            ESTIMATED_STRUCTURAL_ENTRY_BYTES,
                        )
                        .map_err(merge_budget_error)?;
                    toggle_history
                        .entry((*target, ToggleDirection::Undo))
                        .or_default()
                        .push(operation.operation_id());
                    result.push(effect);
                }
                OperationPayload::Redo { target } => {
                    if active.get(&target.operation_id()) != Some(&false) {
                        let intent = (*target, ToggleDirection::Redo);
                        let prior_toggles_len = toggle_history.get(&intent).map_or(0, Vec::len);
                        budget
                            .charge(
                                BudgetKind::WorkingSetBytes,
                                estimated_structural_bytes(prior_toggles_len),
                            )
                            .map_err(merge_budget_error)?;
                        let prior_toggles =
                            toggle_history.get(&intent).cloned().unwrap_or_default();
                        if !prior_toggles.is_empty() {
                            let mut sequential = false;
                            for prior in prior_toggles {
                                if self
                                    .ancestor_with_budget(
                                        prior,
                                        operation.operation_id(),
                                        budget,
                                        local_usage,
                                    )
                                    .map_err(merge_budget_error)?
                                {
                                    sequential = true;
                                    break;
                                }
                            }
                            if sequential {
                                return Err(MergeError::InvalidToggle);
                            }
                            budget
                                .charge(
                                    BudgetKind::WorkingSetBytes,
                                    ESTIMATED_STRUCTURAL_ENTRY_BYTES,
                                )
                                .map_err(merge_budget_error)?;
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
                    // Redo clones the referenced apply mutation, whose size
                    // is independent of the redo operation's own wire size.
                    budget
                        .charge(
                            BudgetKind::WorkingSetBytes,
                            target_operation.canonical_bytes().len(),
                        )
                        .map_err(merge_budget_error)?;
                    active.insert(target.operation_id(), true);
                    budget
                        .charge(
                            BudgetKind::WorkingSetBytes,
                            ESTIMATED_STRUCTURAL_ENTRY_BYTES,
                        )
                        .map_err(merge_budget_error)?;
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
                OperationPayload::ResolveV2 { value, .. } => {
                    result.push(value.applied_effect(operation.operation_id()));
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

    fn validate_graph_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
        local_usage: &mut ResourceUsage,
    ) -> Result<(), OperationError> {
        budget.check_cancelled().map_err(operation_budget_error)?;
        for operation in self.operations.values() {
            budget.check_cancelled().map_err(operation_budget_error)?;
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
                    if self
                        .ancestor_with_budget(*left, *right, budget, local_usage)
                        .map_err(operation_budget_error)?
                        || self
                            .ancestor_with_budget(*right, *left, budget, local_usage)
                            .map_err(operation_budget_error)?
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
                    || !self
                        .ancestor_with_budget(
                            target.operation_id(),
                            operation.operation_id(),
                            budget,
                            local_usage,
                        )
                        .map_err(operation_budget_error)?
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
                self.validate_resolution(operation, left, right, budget, local_usage)?;
            }
        }
        budget
            .charge(
                BudgetKind::WorkingSetBytes,
                estimated_structural_bytes(self.operations.len()),
            )
            .map_err(operation_budget_error)?;
        self.ordered_with_budget(budget).map(|_| ())
    }

    fn validate_resolution<H: CancellationHook>(
        &self,
        operation: &Operation,
        left: OperationReference,
        right: OperationReference,
        budget: &mut ResourceBudget<H>,
        local_usage: &mut ResourceUsage,
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
        if !self
            .ancestor_with_budget(
                left.operation_id(),
                operation.operation_id(),
                budget,
                local_usage,
            )
            .map_err(operation_budget_error)?
            || !self
                .ancestor_with_budget(
                    right.operation_id(),
                    operation.operation_id(),
                    budget,
                    local_usage,
                )
                .map_err(operation_budget_error)?
            || self
                .ancestor_with_budget(
                    left.operation_id(),
                    right.operation_id(),
                    budget,
                    local_usage,
                )
                .map_err(operation_budget_error)?
            || self
                .ancestor_with_budget(
                    right.operation_id(),
                    left.operation_id(),
                    budget,
                    local_usage,
                )
                .map_err(operation_budget_error)?
        {
            return Err(OperationError::InvalidResolution);
        }
        // Validate the semantic values, rather than converting a typed V2
        // effect back into the mutation-only V1 representation. In
        // particular, an unknown calibration prior is a valid conflict arm
        // and must remain inspectable and resolvable.
        let (left_effect, left_effect_bytes) = self
            .effect_with_budget(left_operation, budget)
            .map_err(resolution_effect_error)?;
        let (right_effect, right_effect_bytes) = self
            .effect_with_budget(right_operation, budget)
            .map_err(resolution_effect_error)?;
        let resolution_value = match operation.payload() {
            OperationPayload::Resolve { mutation, .. } => {
                budget
                    .charge(
                        BudgetKind::WorkingSetBytes,
                        operation.canonical_bytes().len(),
                    )
                    .map_err(operation_budget_error)?;
                ResolutionValue::Mutation(mutation.clone())
            }
            OperationPayload::ResolveV2 { value, .. } => {
                budget
                    .charge(
                        BudgetKind::WorkingSetBytes,
                        operation.canonical_bytes().len(),
                    )
                    .map_err(operation_budget_error)?;
                value.clone()
            }
            OperationPayload::Apply { .. }
            | OperationPayload::Undo { .. }
            | OperationPayload::Redo { .. } => return Err(OperationError::InvalidResolution),
        };
        let distinct_toggle_intents = match (left_operation.payload(), right_operation.payload()) {
            (
                OperationPayload::Undo {
                    target: left_target,
                },
                OperationPayload::Undo {
                    target: right_target,
                },
            )
            | (
                OperationPayload::Redo {
                    target: left_target,
                },
                OperationPayload::Redo {
                    target: right_target,
                },
            ) => left_target != right_target,
            (
                OperationPayload::Undo { .. } | OperationPayload::Redo { .. },
                OperationPayload::Undo { .. } | OperationPayload::Redo { .. },
            ) => true,
            _ => false,
        };
        if left_effect.field_key() != right_effect.field_key()
            || left_effect.field_key() != resolution_value.field_key()
        {
            return Err(OperationError::InvalidResolution);
        }
        budget
            .charge(BudgetKind::WorkingSetBytes, left_effect_bytes)
            .map_err(operation_budget_error)?;
        let left_identity = left_effect.canonical_identity();
        budget
            .charge(BudgetKind::WorkingSetBytes, right_effect_bytes)
            .map_err(operation_budget_error)?;
        let right_identity = right_effect.canonical_identity();
        if left_identity == right_identity && !distinct_toggle_intents {
            return Err(OperationError::InvalidResolution);
        }
        Ok(())
    }

    fn ancestor_with_budget<H: CancellationHook>(
        &self,
        ancestor: OperationId,
        descendant: OperationId,
        budget: &mut ResourceBudget<H>,
        local_usage: &mut ResourceUsage,
    ) -> Result<bool, ResourceBudgetError> {
        if ancestor == descendant {
            return Ok(false);
        }
        budget.charge(
            BudgetKind::WorkingSetBytes,
            ESTIMATED_STRUCTURAL_ENTRY_BYTES,
        )?;
        let mut pending = vec![descendant];
        let mut visited = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if visited.contains(&current) {
                continue;
            }
            budget.charge_with_limit(
                BudgetKind::OperationAncestryWork,
                1,
                local_usage,
                MAX_ANCESTRY_WORK,
            )?;
            budget.charge(
                BudgetKind::WorkingSetBytes,
                ESTIMATED_STRUCTURAL_ENTRY_BYTES,
            )?;
            visited.insert(current);
            let Some(operation) = self.operations.get(&current) else {
                continue;
            };
            for parent in operation.parents() {
                if *parent == ancestor {
                    return Ok(true);
                }
                budget.charge(
                    BudgetKind::WorkingSetBytes,
                    ESTIMATED_STRUCTURAL_ENTRY_BYTES,
                )?;
                pending.push(*parent);
            }
        }
        Ok(false)
    }

    fn conflicts(&self) -> Result<Vec<MergeConflict>, MergeError> {
        let mut budget = default_operation_budget();
        let mut local_usage = ResourceUsage::default();
        self.conflicts_with_budget(&mut budget, &mut local_usage)
    }

    fn conflicts_with_budget<H: CancellationHook>(
        &self,
        budget: &mut ResourceBudget<H>,
        local_usage: &mut ResourceUsage,
    ) -> Result<Vec<MergeConflict>, MergeError> {
        budget.check_cancelled().map_err(merge_budget_error)?;
        budget
            .charge(
                BudgetKind::WorkingSetBytes,
                estimated_structural_bytes(self.operations.len()),
            )
            .map_err(merge_budget_error)?;
        #[derive(Clone)]
        struct FrontierEntry {
            operation_id: OperationId,
            reference: OperationReference,
            identity: FrontierIdentity,
            intent: ConflictIntent,
            event: AppliedEffect,
            /// Conservative canonical-byte proxy for one clone of `event`.
            /// Undo/redo events own a target payload, so this is the target's
            /// operation size rather than the toggle operation's size.
            event_bytes: usize,
        }

        type ConflictKey = (OperationId, OperationId);
        let mut frontier: BTreeMap<FieldKey, Vec<FrontierEntry>> = BTreeMap::new();
        let mut conflicts: BTreeMap<ConflictKey, MergeConflict> = BTreeMap::new();
        for operation in self
            .ordered_with_budget(budget)
            .map_err(MergeError::Operation)?
        {
            if let Some((left, right)) = operation.resolution_references() {
                conflicts.remove(&(left.operation_id(), right.operation_id()));
            }
            let (event, event_bytes) = self.effect_event_with_budget(operation, budget)?;
            // Both conversions below own payload-bearing values. Charge each
            // copy before invoking the conversion so a rejected budget cannot
            // leave an allocation behind.
            budget
                .charge(BudgetKind::WorkingSetBytes, event_bytes)
                .map_err(merge_budget_error)?;
            let effect = event.as_effect_value();
            budget
                .charge(BudgetKind::WorkingSetBytes, event_bytes)
                .map_err(merge_budget_error)?;
            let (identity, intent) = frontier_identity(operation, &effect);
            let key = effect.field_key();
            let previous = frontier.remove(&key).unwrap_or_default();
            let mut next = Vec::with_capacity(previous.len().saturating_add(1));
            for entry in previous {
                let same_value = entry.identity.value() == identity.value();
                if same_value
                    && self
                        .ancestor_with_budget(
                            entry.operation_id,
                            operation.operation_id(),
                            budget,
                            local_usage,
                        )
                        .map_err(merge_budget_error)?
                {
                    self.remove_superseded_conflicts(
                        &mut conflicts,
                        entry.operation_id,
                        operation.operation_id(),
                        budget,
                        local_usage,
                    )?;
                    continue;
                }
                if same_value
                    && (entry.identity.is_value() && identity.is_value()
                        || entry.identity.same_toggle_intent(&identity))
                {
                    // Equal value effects and duplicate same-target toggles
                    // share one representative. The latter is a canonical
                    // replay no-op after the first toggle.
                    self.remove_superseded_conflicts(
                        &mut conflicts,
                        entry.operation_id,
                        operation.operation_id(),
                        budget,
                        local_usage,
                    )?;
                    continue;
                }
                if same_value && !entry.identity.is_value() && !identity.is_value() {
                    // Different concurrent toggle intents remain independent
                    // even when they happen to restore the same value. Keep
                    // both frontier entries and report their intent conflict.
                    let (left, right, left_intent, right_intent, left_effect, right_effect) =
                        if entry.operation_id < operation.operation_id() {
                            (
                                entry.reference,
                                OperationReference::from(operation),
                                entry.intent,
                                intent,
                                clone_effect_with_budget(&entry.event, entry.event_bytes, budget)?,
                                clone_effect_with_budget(&event, event_bytes, budget)?,
                            )
                        } else {
                            (
                                OperationReference::from(operation),
                                entry.reference,
                                intent,
                                entry.intent,
                                clone_effect_with_budget(&event, event_bytes, budget)?,
                                clone_effect_with_budget(&entry.event, entry.event_bytes, budget)?,
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
                    continue;
                }
                if same_value {
                    // A value operation and a toggle with the same effective
                    // value are equivalent for state selection, but the
                    // toggle entry remains in the frontier so distinct
                    // toggle intents can still be audited independently.
                    next.push(entry);
                    continue;
                }
                if self
                    .ancestor_with_budget(
                        entry.operation_id,
                        operation.operation_id(),
                        budget,
                        local_usage,
                    )
                    .map_err(merge_budget_error)?
                {
                    self.remove_superseded_conflicts(
                        &mut conflicts,
                        entry.operation_id,
                        operation.operation_id(),
                        budget,
                        local_usage,
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
                            clone_effect_with_budget(&entry.event, entry.event_bytes, budget)?,
                            clone_effect_with_budget(&event, event_bytes, budget)?,
                        )
                    } else {
                        (
                            OperationReference::from(operation),
                            entry.reference,
                            intent,
                            entry.intent,
                            clone_effect_with_budget(&event, event_bytes, budget)?,
                            clone_effect_with_budget(&entry.event, entry.event_bytes, budget)?,
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
                event_bytes,
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
        budget: &mut ResourceBudget<impl CancellationHook>,
        local_usage: &mut ResourceUsage,
    ) -> Result<(), MergeError> {
        let mut keys = Vec::new();
        for key in conflicts.keys().copied() {
            budget
                .charge(
                    BudgetKind::WorkingSetBytes,
                    ESTIMATED_STRUCTURAL_ENTRY_BYTES,
                )
                .map_err(merge_budget_error)?;
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
                .ancestor_with_budget(other, current, budget, local_usage)
                .map_err(merge_budget_error)?
            {
                continue;
            }
            conflicts.remove(&key);
        }
        Ok(())
    }

    fn effect_with_budget<H: CancellationHook>(
        &self,
        operation: &Operation,
        budget: &mut ResourceBudget<H>,
    ) -> Result<(EffectValue, usize), MergeError> {
        let (event, event_bytes) = self.effect_event_with_budget(operation, budget)?;
        budget
            .charge(BudgetKind::WorkingSetBytes, event_bytes)
            .map_err(merge_budget_error)?;
        Ok((event.as_effect_value(), event_bytes))
    }

    fn effect_event_with_budget<H: CancellationHook>(
        &self,
        operation: &Operation,
        budget: &mut ResourceBudget<H>,
    ) -> Result<(AppliedEffect, usize), MergeError> {
        self.effect_event_with_charge(operation, |source| {
            budget
                .charge(BudgetKind::WorkingSetBytes, source.canonical_bytes().len())
                .map_err(merge_budget_error)
        })
    }

    fn effect_event_with_charge<F>(
        &self,
        operation: &Operation,
        mut charge: F,
    ) -> Result<(AppliedEffect, usize), MergeError>
    where
        F: FnMut(&Operation) -> Result<(), MergeError>,
    {
        match operation.payload() {
            OperationPayload::Apply { mutation } => {
                charge(operation)?;
                Ok((
                    AppliedEffect::Mutation(AppliedMutation {
                        operation_id: operation.operation_id(),
                        mutation: mutation.clone(),
                    }),
                    operation.canonical_bytes().len(),
                ))
            }
            OperationPayload::Undo { target } => {
                let target_operation = self
                    .operations
                    .get(&target.operation_id())
                    .ok_or(MergeError::Operation(OperationError::MissingTarget))?;
                charge(target_operation)?;
                Ok((
                    target_operation
                        .inverse()
                        .as_applied_effect(operation.operation_id())
                        .map_err(MergeError::Operation)?,
                    target_operation.canonical_bytes().len(),
                ))
            }
            OperationPayload::Redo { target } => {
                let target_operation = self
                    .operations
                    .get(&target.operation_id())
                    .ok_or(MergeError::Operation(OperationError::MissingTarget))?;
                let OperationPayload::Apply { mutation } = target_operation.payload() else {
                    return Err(MergeError::InvalidToggle);
                };
                charge(target_operation)?;
                Ok((
                    AppliedEffect::Mutation(AppliedMutation {
                        operation_id: operation.operation_id(),
                        mutation: mutation.clone(),
                    }),
                    target_operation.canonical_bytes().len(),
                ))
            }
            OperationPayload::Resolve { mutation, .. } => {
                charge(operation)?;
                Ok((
                    AppliedEffect::Mutation(AppliedMutation {
                        operation_id: operation.operation_id(),
                        mutation: mutation.clone(),
                    }),
                    operation.canonical_bytes().len(),
                ))
            }
            OperationPayload::ResolveV2 { value, .. } => {
                charge(operation)?;
                Ok((
                    value.applied_effect(operation.operation_id()),
                    operation.canonical_bytes().len(),
                ))
            }
        }
    }
}

fn clone_effect_with_budget<H: CancellationHook>(
    effect: &AppliedEffect,
    effect_bytes: usize,
    budget: &mut ResourceBudget<H>,
) -> Result<AppliedEffect, MergeError> {
    budget
        .charge(BudgetKind::WorkingSetBytes, effect_bytes)
        .map_err(merge_budget_error)?;
    Ok(effect.clone())
}

fn charge_operation_copy<H: CancellationHook>(
    operation: &Operation,
    budget: &mut ResourceBudget<H>,
) -> Result<(), MergeError> {
    // OperationBytes accounts for the retained canonical Vec. The decoded
    // payload is a separate owned copy, conservatively represented by its
    // canonical length in WorkingSetBytes alongside the map entry.
    let working_bytes = operation
        .canonical_bytes()
        .len()
        .checked_add(ESTIMATED_STRUCTURAL_ENTRY_BYTES)
        .ok_or(MergeError::ResourceLimit("working_set_bytes"))?;
    budget
        .charge(
            BudgetKind::OperationBytes,
            operation.canonical_bytes().len(),
        )
        .map_err(merge_budget_error)?;
    budget
        .charge(BudgetKind::WorkingSetBytes, working_bytes)
        .map_err(merge_budget_error)
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
    Cancelled,
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
    Cancelled,
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
