//! A bounded, renderer-neutral projection of a validated spatial tile.
//!
//! [`SceneDocument`] carries presentation-independent numerical evidence and
//! provenance. It is an adapter contract, not a second source of spatial
//! truth: the spatial-analysis tile remains the authority for computed cell
//! values. No renderer, GPU API, or UI type is used here.

use kyberia_domain::{
    analysis::ExactU64,
    evidence::{ArtifactReference, Evidence},
    identity::{ContentHash, FloorId, FrameId, ObservationId, Text},
    units::{CoordinateMeters, Db, Dbm, Meters, Probability},
};
use kyberia_spatial_analysis::MetricId;
use kyberia_spatial_analysis::{
    ALGORITHM_VERSION, Cell, CellClass, Config, InputEvidencePlane, MAX_CELLS,
    MAX_DISTANCE_EVALUATIONS, MAX_METRIC_DEFINITION_BYTES, MAX_NEIGHBORS, MAX_SAMPLES, Tile,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

/// The first stable renderer-neutral scene wire contract.
pub const SCHEMA_VERSION: &str = "kyberia.render-scene/1";
/// The input tile contract admitted by this adapter.
pub const TILE_SCHEMA_VERSION: &str = "kyberia.numeric-rssi-tile/2";
/// Prevents an artifact reference from causing an unbounded verification
/// allocation or hashing operation at this boundary.
pub const MAX_SOURCE_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;
/// A scene is intentionally smaller than an unbounded export. Producers that
/// need larger coverage must stream independently addressed tiles.
pub const MAX_SCENE_BYTES: usize = 64 * 1024 * 1024;
/// Maximum estimated temporary memory used while validating and projecting one
/// scene. Larger coverage must be split into independently addressed tiles.
/// This is a deterministic admission budget, not a claim about allocator RSS.
pub const MAX_SCENE_WORKING_BYTES: usize = 256 * 1024 * 1024;
/// The spatial-analysis work bound is checked before this adapter clones input
/// or allocates projected cells.
pub const MAX_SCENE_DISTANCE_EVALUATIONS: usize = MAX_DISTANCE_EVALUATIONS;
pub const MAX_LOCATION_GROUPS: usize = MAX_SAMPLES;
pub const MAX_TOTAL_CONTRIBUTIONS: usize = MAX_CELLS * 64;
/// Maximum nesting accepted by the streaming JSON shape pass. The typed
/// decoder remains strict about the complete schema after this pass.
const MAX_SCENE_JSON_DEPTH: usize = 64;
/// A JSON Vec can temporarily grow geometrically while Serde decodes an
/// array. Preflight charges a two-times count capacity so that this transient
/// growth is admitted before the typed Vec exists. The pass retains neither
/// the complete input model nor any counted array.
const PREDECODE_VEC_GROWTH_FACTOR: usize = 2;
const MAX_GROUP_ID_REFERENCES: usize = MAX_SAMPLES * 2;
const MAX_TEXT_BYTES: usize = 1024;

// These are the maximum simultaneously live values in either admission path.
// The caller's input, its model/replayed tile copies, and the projected wire
// are all counted so the estimate does not depend on whether the caller keeps
// its input alive while the scene is built.
const LIVE_SAMPLE_COLLECTIONS: usize = 4;
const LIVE_GROUP_COLLECTIONS: usize = 4;
const LIVE_CELL_COLLECTIONS: usize = 3;
const GROUP_ID_VECS_PER_GROUP: usize = 2;

// This is a deterministic accounting pad per Vec/BTreeSet allocation. It is
// four pointer-sized words for allocator bookkeeping and capacity rounding;
// it is deliberately documented as an admission reserve, not an RSS bound.
const ALLOCATION_ACCOUNTING_PAD_BYTES: usize = size_of::<usize>() * 4;
const AGGREGATION_SCRATCH_F64_VECS: usize = 2;
const AGGREGATION_TREE_ENTRY_PAD_BYTES: usize = ALLOCATION_ACCOUNTING_PAD_BYTES * 2;
const LIVE_CELL_NEIGHBOR_VECS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "kyberia.render-scene/1")]
enum SceneSchemaVersion {
    V1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneWire {
    schema: SceneSchemaVersion,
    identity: SceneIdentity,
    metric_artifact: kyberia_domain::analysis::VersionedArtifact,
    metric_definition_bytes: Vec<u8>,
    evidence_plane: InputEvidencePlane,
    configuration: Config,
    samples: Vec<kyberia_spatial_analysis::Sample>,
    grid: SceneGrid,
    location_groups: Vec<SceneLocationGroup>,
    cells: Vec<SceneCell>,
}

/// Copied provenance references for the computation and its inputs.
/// These references alone do not prove that cell values were computed from them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneIdentity {
    metric_id: MetricId,
    metric_version: Text,
    metric_revision: u16,
    metric_definition_hash: ContentHash,
    signal_aggregation: kyberia_spatial_analysis::SignalAggregationSelection,
    source_artifact: ArtifactReference,
    spatial_schema: Text,
    algorithm_version: Text,
}

impl SceneIdentity {
    pub fn metric_id(&self) -> &MetricId {
        &self.metric_id
    }

    pub fn metric_version(&self) -> &Text {
        &self.metric_version
    }

    pub const fn metric_revision(&self) -> u16 {
        self.metric_revision
    }

    pub const fn metric_definition_hash(&self) -> ContentHash {
        self.metric_definition_hash
    }

    pub const fn signal_aggregation(&self) -> kyberia_spatial_analysis::SignalAggregationSelection {
        self.signal_aggregation
    }

    pub fn source_artifact(&self) -> &ArtifactReference {
        &self.source_artifact
    }

    pub fn spatial_schema(&self) -> &Text {
        &self.spatial_schema
    }

    pub fn algorithm_version(&self) -> &Text {
        &self.algorithm_version
    }
}

/// A floor-local meter coordinate. The adapter preserves the
/// spatial-analysis convention: x increases by column and y by row. Any
/// screen-space y flip belongs to a renderer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePoint {
    pub x: CoordinateMeters,
    pub y: CoordinateMeters,
}

/// Exact row-major tile geometry, including the global integer offsets.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneGrid {
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub origin: ScenePoint,
    pub resolution: Meters,
    pub column_offset: u32,
    pub row_offset: u32,
    pub width: u32,
    pub height: u32,
}

impl SceneGrid {
    fn validate_with_cancellation(
        &self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<usize, SceneError> {
        let count = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or(SceneError::ResourceLimit("cell count"))?;
        if count == 0 || count > MAX_CELLS {
            return Err(SceneError::ResourceLimit("cell count"));
        }
        if self.resolution.get() <= 0.0 {
            return Err(SceneError::InvalidDocument("positive grid resolution"));
        }
        if self.column_offset.checked_add(self.width).is_none()
            || self.row_offset.checked_add(self.height).is_none()
        {
            return Err(SceneError::ResourceLimit("grid offsets"));
        }
        self.center(0, 0)?;
        self.center(self.width - 1, self.height - 1)?;
        for column in 1..self.width {
            if (column - 1).is_multiple_of(64) && cancelled() {
                return Err(SceneError::Cancelled);
            }
            if self.center(column - 1, 0)?.x >= self.center(column, 0)?.x {
                return Err(SceneError::InvalidDocument("unrepresentable x spacing"));
            }
        }
        for row in 1..self.height {
            if (row - 1).is_multiple_of(64) && cancelled() {
                return Err(SceneError::Cancelled);
            }
            if self.center(0, row - 1)?.y >= self.center(0, row)?.y {
                return Err(SceneError::InvalidDocument("unrepresentable y spacing"));
            }
        }
        Ok(count)
    }

    /// Returns a row-major center using the canonical floor-local formula.
    pub fn center(&self, column: u32, row: u32) -> Result<ScenePoint, SceneError> {
        if column >= self.width || row >= self.height {
            return Err(SceneError::InvalidDocument("cell coordinate outside grid"));
        }
        let x = (f64::from(self.column_offset) + f64::from(column) + 0.5) * self.resolution.get()
            + self.origin.x.get();
        let y = (f64::from(self.row_offset) + f64::from(row) + 0.5) * self.resolution.get()
            + self.origin.y.get();
        Ok(ScenePoint {
            x: CoordinateMeters::new(x)
                .map_err(|_| SceneError::InvalidDocument("grid x coordinate"))?,
            y: CoordinateMeters::new(y)
                .map_err(|_| SceneError::InvalidDocument("grid y coordinate"))?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneLocationGroup {
    position: ScenePoint,
    observation_ids: Vec<ObservationId>,
    signal_aggregate: kyberia_wifi_semantics::SignalAggregate,
}

impl SceneLocationGroup {
    pub fn position(&self) -> ScenePoint {
        self.position
    }

    pub fn observation_ids(&self) -> &[ObservationId] {
        &self.observation_ids
    }

    pub fn signal_aggregate(&self) -> &kyberia_wifi_semantics::SignalAggregate {
        &self.signal_aggregate
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneCellClass {
    Observed,
    Interpolated,
    Extrapolated,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneContribution {
    location_group: ExactU64,
    weight: Probability,
}

impl SceneContribution {
    pub fn location_group(&self) -> usize {
        usize::try_from(self.location_group.get()).unwrap_or(usize::MAX)
    }

    pub const fn weight(&self) -> Probability {
        self.weight
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneCell {
    value: Evidence<Dbm>,
    class: SceneCellClass,
    support_locations: ExactU64,
    support_observations: ExactU64,
    nearest_distance: Evidence<Meters>,
    uncertainty_db: Evidence<Db>,
    contributors: Vec<SceneContribution>,
}

impl SceneCell {
    pub fn value(&self) -> &Evidence<Dbm> {
        &self.value
    }

    pub const fn class(&self) -> SceneCellClass {
        self.class
    }

    pub fn support_locations(&self) -> usize {
        usize::try_from(self.support_locations.get()).unwrap_or(usize::MAX)
    }

    pub fn support_observations(&self) -> usize {
        usize::try_from(self.support_observations.get()).unwrap_or(usize::MAX)
    }

    pub fn nearest_distance(&self) -> &Evidence<Meters> {
        &self.nearest_distance
    }

    pub fn uncertainty_db(&self) -> &Evidence<Db> {
        &self.uncertainty_db
    }

    pub fn contributors(&self) -> &[SceneContribution] {
        &self.contributors
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// Presentation-only colors. This type is deliberately excluded from scene
/// bytes and hashes; palette edits cannot alter numerical identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationStyle {
    pub known: Rgba,
    pub unknown: Rgba,
}

impl PresentationStyle {
    pub fn color_for(&self, cell: &SceneCell) -> Rgba {
        match cell.value {
            Evidence::Known(_) => self.known,
            Evidence::Unknown(_) => self.unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneError {
    Cancelled,
    UnsupportedVersion,
    InvalidTile(&'static str),
    InvalidDocument(&'static str),
    ResourceLimit(&'static str),
    SourceArtifactMismatch,
    MalformedBytes,
    NonCanonicalBytes,
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SceneError {}

/// Deterministic admission accounting for an encoded scene. The working-set
/// value is a conservative allocation proxy used before typed decoding; it is
/// target-dependent because it includes `size_of` terms and is never part of
/// canonical bytes or scene identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneAdmissionEstimate {
    encoded_bytes: usize,
    working_set_bytes: usize,
}

impl SceneAdmissionEstimate {
    pub const fn encoded_bytes(self) -> usize {
        self.encoded_bytes
    }

    pub const fn working_set_bytes(self) -> usize {
        self.working_set_bytes
    }
}

/// An immutable validated scene projection. The canonical tile remains the
/// numerical authority; this object only carries a renderer-facing copy.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDocument {
    wire: SceneWire,
    canonical: Vec<u8>,
    sha256: ContentHash,
}

impl Serialize for SceneDocument {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.wire.serialize(serializer)
    }
}

impl SceneDocument {
    /// Performs structural and evidence consistency checks on the mutable
    /// public Tile representation before making a renderer projection.
    pub fn from_tile(tile: &Tile) -> Result<Self, SceneError> {
        Self::from_tile_with_cancellation(tile, || false)
    }

    /// Recompute the tile with cooperative cancellation; no partial scene is returned.
    pub fn from_tile_with_cancellation(
        tile: &Tile,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, SceneError> {
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        let encoded_bytes = admit_serialized_size(tile, MAX_SCENE_BYTES, &mut cancelled)?;
        admit_tile_resources(tile, encoded_bytes, &mut cancelled)?;
        validate_tile_with_cancellation(tile, &mut cancelled)?;
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        let model = kyberia_spatial_analysis::Model::new(tile.inputs.clone(), tile.configuration)
            .map_err(|_| SceneError::InvalidTile("canonical model inputs"))?;
        let replayed = model
            .tile(tile.grid, &mut cancelled)
            .map_err(|error| match error {
                kyberia_spatial_analysis::Error::Cancelled => SceneError::Cancelled,
                kyberia_spatial_analysis::Error::ResourceLimit(reason) => {
                    SceneError::ResourceLimit(reason)
                }
                _ => SceneError::InvalidTile("canonical tile replay"),
            })?;
        if replayed.cells != tile.cells || replayed.location_groups != tile.location_groups {
            return Err(SceneError::InvalidTile(
                "tile differs from canonical computation",
            ));
        }
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        let wire = wire_from_tile(&replayed, encoded_bytes, &mut cancelled)?;
        Self::from_wire_with_cancellation(wire, encoded_bytes, &mut cancelled)
    }

    /// Checks the bytes named by the tile's source artifact, then applies the
    /// recomputation checks as `from_tile`. This establishes consistency with the
    /// supplied canonical samples, but does not authenticate their origin or
    /// prove that arbitrary source-file bytes decode to those samples.
    pub fn from_verified_tile(tile: &Tile, source_bytes: &[u8]) -> Result<Self, SceneError> {
        Self::from_verified_tile_with_cancellation(tile, source_bytes, || false)
    }

    /// Checks source bytes in bounded chunks before the cancellable projection.
    pub fn from_verified_tile_with_cancellation(
        tile: &Tile,
        source_bytes: &[u8],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, SceneError> {
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        let reference = &tile.inputs.source_artifact;
        if reference.byte_length > MAX_SOURCE_ARTIFACT_BYTES
            || reference.byte_length != source_bytes.len() as u64
            || reference.sha256.bytes()
                != hash_bytes_with_cancellation(source_bytes, &mut cancelled)?
        {
            return Err(SceneError::SourceArtifactMismatch);
        }
        Self::from_tile_with_cancellation(tile, &mut cancelled)
    }

    /// Parses only this exact version and requires byte-for-byte canonical
    /// encoding. This makes future or reordered representations fail closed.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, SceneError> {
        Self::from_canonical_bytes_with_cancellation(bytes, || false)
    }

    /// Perform the non-retaining shape pass used by canonical import and
    /// return its deterministic working-set estimate without typed decoding.
    pub fn estimate_canonical_resources(
        bytes: &[u8],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SceneAdmissionEstimate, SceneError> {
        let shape = preflight_scene(bytes, &mut cancelled)?;
        Ok(SceneAdmissionEstimate {
            encoded_bytes: bytes.len(),
            working_set_bytes: estimated_scene_working_bytes(shape)?,
        })
    }

    /// Cooperatively cancel numerical replay and check between import stages.
    /// Decoding polls every 4 KiB; structural validation remains a bounded synchronous stage.
    pub fn from_canonical_bytes_with_cancellation(
        bytes: &[u8],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, SceneError> {
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        if bytes.len() > MAX_SCENE_BYTES {
            return Err(SceneError::ResourceLimit("scene bytes"));
        }
        preflight_scene(bytes, &mut cancelled)?;
        let wire = decode_scene(bytes, &mut cancelled)?;
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        Self::from_wire_with_input(wire, bytes, &mut cancelled)
    }

    fn from_wire_with_cancellation(
        wire: SceneWire,
        encoded_bytes: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Self, SceneError> {
        admit_wire_resources(&wire, encoded_bytes, cancelled)?;
        validate_wire_with_cancellation(&wire, cancelled)?;
        let canonical = bounded_scene_bytes_with_cancellation(&wire, MAX_SCENE_BYTES, cancelled)?;
        if canonical.len() > MAX_SCENE_BYTES {
            return Err(SceneError::ResourceLimit("scene bytes"));
        }
        let sha256 = ContentHash::from_sha256(hash_bytes_with_cancellation(&canonical, cancelled)?);
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        Ok(Self {
            wire,
            canonical,
            sha256,
        })
    }

    fn from_wire_with_input(
        wire: SceneWire,
        input: &[u8],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Self, SceneError> {
        admit_wire_resources(&wire, input.len(), cancelled)?;
        validate_wire_with_cancellation(&wire, cancelled)?;
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        validate_wire_computation(&wire, cancelled)?;
        let canonical = bounded_scene_bytes_with_cancellation(&wire, MAX_SCENE_BYTES, cancelled)?;
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        if canonical != input {
            return Err(SceneError::NonCanonicalBytes);
        }
        let sha256 = ContentHash::from_sha256(hash_bytes_with_cancellation(&canonical, cancelled)?);
        if cancelled() {
            return Err(SceneError::Cancelled);
        }
        Ok(Self {
            wire,
            canonical,
            sha256,
        })
    }

    pub fn identity(&self) -> &SceneIdentity {
        &self.wire.identity
    }

    pub const fn grid(&self) -> SceneGrid {
        self.wire.grid
    }

    pub const fn configuration(&self) -> Config {
        self.wire.configuration
    }

    pub const fn evidence_plane(&self) -> InputEvidencePlane {
        self.wire.evidence_plane
    }

    pub fn location_groups(&self) -> &[SceneLocationGroup] {
        &self.wire.location_groups
    }

    pub fn cells(&self) -> &[SceneCell] {
        &self.wire.cells
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    pub const fn sha256(&self) -> ContentHash {
        self.sha256
    }
}

fn poll_cancelled(cancelled: &mut impl FnMut() -> bool, index: usize) -> Result<(), SceneError> {
    if index.is_multiple_of(64) && cancelled() {
        return Err(SceneError::Cancelled);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SceneResourceShape {
    sample_count: usize,
    sample_capacity: usize,
    group_count: usize,
    group_capacity: usize,
    /// Sum of both observation-ID vectors in every group. A malformed wire
    /// may disagree between these vectors, so both lengths are admitted.
    group_id_count: usize,
    group_id_capacity: usize,
    largest_group_id_count: usize,
    largest_group_id_capacity: usize,
    cell_count: usize,
    cell_capacity: usize,
    contribution_count: usize,
    contribution_capacity: usize,
    encoded_bytes: usize,
}

#[derive(Default)]
struct ScenePreflightCounts {
    sample_count: usize,
    group_count: usize,
    group_id_count: usize,
    largest_group_id_count: usize,
    cell_count: usize,
    contribution_count: usize,
    metric_definition_bytes: usize,
}

fn predecode_vec_capacity(count: usize) -> Result<usize, SceneError> {
    count
        .checked_mul(PREDECODE_VEC_GROWTH_FACTOR)
        .ok_or(SceneError::ResourceLimit("scene working set"))
}

fn shape_from_preflight(
    counts: ScenePreflightCounts,
    encoded_bytes: usize,
) -> Result<SceneResourceShape, SceneError> {
    if counts.metric_definition_bytes > MAX_METRIC_DEFINITION_BYTES {
        return Err(SceneError::ResourceLimit("metric definition bytes"));
    }
    let sample_capacity = predecode_vec_capacity(counts.sample_count)?;
    let group_capacity = predecode_vec_capacity(counts.group_count)?;
    let group_id_capacity = predecode_vec_capacity(counts.group_id_count)?;
    let cell_capacity = predecode_vec_capacity(counts.cell_count)?;
    let contribution_capacity = predecode_vec_capacity(counts.contribution_count)?;
    let largest_group_id_capacity = predecode_vec_capacity(counts.largest_group_id_count)?;
    let shape = SceneResourceShape {
        sample_count: counts.sample_count,
        sample_capacity,
        group_count: counts.group_count,
        group_capacity,
        group_id_count: counts.group_id_count,
        group_id_capacity,
        largest_group_id_count: counts.largest_group_id_count,
        largest_group_id_capacity,
        cell_count: counts.cell_count,
        cell_capacity,
        contribution_count: counts.contribution_count,
        contribution_capacity,
        encoded_bytes,
    };
    admit_resource_shape(shape)?;
    Ok(shape)
}

impl SceneResourceShape {
    #[cfg(test)]
    fn from_lengths(
        sample_count: usize,
        group_count: usize,
        group_id_count: usize,
        cell_count: usize,
        contribution_count: usize,
        encoded_bytes: usize,
    ) -> Self {
        Self {
            sample_count,
            sample_capacity: sample_count,
            group_count,
            group_capacity: group_count,
            group_id_count,
            group_id_capacity: group_id_count,
            largest_group_id_count: group_id_count,
            largest_group_id_capacity: group_id_count,
            cell_count,
            cell_capacity: cell_count,
            contribution_count,
            contribution_capacity: contribution_count,
            encoded_bytes,
        }
    }
}

fn checked_scaled(count: usize, unit: usize) -> Result<usize, SceneError> {
    count
        .checked_mul(unit)
        .ok_or(SceneError::ResourceLimit("scene working set"))
}

fn checked_add_assign(total: &mut usize, value: usize) -> Result<(), SceneError> {
    *total = total
        .checked_add(value)
        .ok_or(SceneError::ResourceLimit("scene working set"))?;
    Ok(())
}

/// Reserve one or more Vec allocations, using the observed capacity as the
/// payload and a named per-allocation pad for allocator bookkeeping. The pad
/// is intentionally a portable accounting policy; it does not claim to model
/// a particular allocator's resident-set overhead.
fn reserve_vec<T>(
    total: &mut usize,
    capacity: usize,
    vector_count: usize,
    live_copies: usize,
) -> Result<(), SceneError> {
    let payload = checked_scaled(capacity, size_of::<T>())?;
    let payload = checked_scaled(payload, live_copies)?;
    let allocations = checked_scaled(vector_count, live_copies)?;
    let pad = checked_scaled(allocations, ALLOCATION_ACCOUNTING_PAD_BYTES)?;
    checked_add_assign(total, payload)?;
    checked_add_assign(total, pad)
}

fn reserve_raw(
    total: &mut usize,
    capacity: usize,
    element_bytes: usize,
    vector_count: usize,
    live_copies: usize,
) -> Result<(), SceneError> {
    let payload = checked_scaled(capacity, element_bytes)?;
    let payload = checked_scaled(payload, live_copies)?;
    let allocations = checked_scaled(vector_count, live_copies)?;
    let pad = checked_scaled(allocations, ALLOCATION_ACCOUNTING_PAD_BYTES)?;
    checked_add_assign(total, payload)?;
    checked_add_assign(total, pad)
}

fn fixed_object_reservation() -> Result<usize, SceneError> {
    // At the peak there can be the caller/replayed Tile pair, one Model, two
    // SceneWire values during serialized replay comparison, and the returned
    // SceneDocument value. Their Vec headers are included by size_of; element
    // and nested allocations are accounted separately below.
    let mut total = 0;
    for (bytes, copies) in [
        (size_of::<Tile>(), 2),
        (size_of::<kyberia_spatial_analysis::Model>(), 1),
        (size_of::<SceneWire>(), 2),
        (size_of::<SceneDocument>(), 1),
    ] {
        checked_add_assign(&mut total, checked_scaled(bytes, copies)?)?;
    }
    // Metric definitions are bounded by the spatial registry. Their decoded
    // object stores are distinct from the canonical byte Vec and may coexist
    // in the model, replayed tile and projected wire.
    checked_add_assign(
        &mut total,
        checked_scaled(MAX_METRIC_DEFINITION_BYTES, LIVE_SAMPLE_COLLECTIONS)?,
    )?;
    Ok(total)
}

fn estimated_scene_working_bytes(shape: SceneResourceShape) -> Result<usize, SceneError> {
    let mut estimate = MAX_SCENE_BYTES
        .checked_add(shape.encoded_bytes)
        .ok_or(SceneError::ResourceLimit("scene working set"))?;
    checked_add_assign(&mut estimate, fixed_object_reservation()?)?;

    // Four Sample collections can overlap: caller input, Model input clone,
    // replayed Tile input clone, and the projected wire samples.
    reserve_vec::<kyberia_spatial_analysis::Sample>(
        &mut estimate,
        shape.sample_capacity,
        1,
        LIVE_SAMPLE_COLLECTIONS,
    )?;
    // Model, replayed Tile and wire groups each carry a nested observation-ID
    // Vec and SignalAggregate::observation_order Vec; the caller's group set
    // is counted as the fourth live collection.
    reserve_raw(
        &mut estimate,
        shape.group_capacity,
        size_of::<kyberia_spatial_analysis::LocationGroup>().max(size_of::<SceneLocationGroup>()),
        1,
        LIVE_GROUP_COLLECTIONS,
    )?;
    reserve_vec::<ObservationId>(
        &mut estimate,
        shape.group_id_capacity,
        shape
            .group_count
            .checked_mul(GROUP_ID_VECS_PER_GROUP)
            .ok_or(SceneError::ResourceLimit("scene working set"))?,
        LIVE_GROUP_COLLECTIONS,
    )?;
    // The known-sample pointer Vec and one aggregate's scratch buffers are
    // live while Model::new groups samples. Percentile/median/trimmed/power
    // methods can retain two f64 Vecs alongside StaticSignalSample values.
    reserve_raw(
        &mut estimate,
        shape.sample_count,
        size_of::<&kyberia_spatial_analysis::Sample>(),
        1,
        1,
    )?;
    // Structural validation retains a BTreeMap of samples and a BTreeSet of
    // grouped IDs. Their entries are separate from Model's aggregate set and
    // can coexist with one aggregate's scratch allocations.
    reserve_raw(
        &mut estimate,
        shape.sample_count,
        size_of::<ObservationId>()
            + size_of::<&kyberia_spatial_analysis::Sample>()
            + AGGREGATION_TREE_ENTRY_PAD_BYTES,
        1,
        1,
    )?;
    reserve_raw(
        &mut estimate,
        shape.group_id_count,
        size_of::<ObservationId>() + AGGREGATION_TREE_ENTRY_PAD_BYTES,
        1,
        1,
    )?;
    reserve_raw(
        &mut estimate,
        shape.largest_group_id_capacity,
        size_of::<kyberia_wifi_semantics::StaticSignalSample>(),
        1,
        1,
    )?;
    reserve_raw(
        &mut estimate,
        shape.largest_group_id_capacity,
        size_of::<f64>(),
        AGGREGATION_SCRATCH_F64_VECS,
        1,
    )?;
    reserve_raw(
        &mut estimate,
        shape.largest_group_id_capacity,
        size_of::<ObservationId>() + AGGREGATION_TREE_ENTRY_PAD_BYTES,
        1,
        1,
    )?;
    // Each estimated cell can retain a bounded neighbor heap and weight Vec;
    // only one cell is active at a time. Validation also keeps one contributor
    // index set, bounded by MAX_NEIGHBORS.
    reserve_raw(
        &mut estimate,
        MAX_NEIGHBORS,
        size_of::<f64>() * 2,
        LIVE_CELL_NEIGHBOR_VECS,
        1,
    )?;
    reserve_raw(
        &mut estimate,
        MAX_NEIGHBORS,
        size_of::<usize>() + AGGREGATION_TREE_ENTRY_PAD_BYTES,
        1,
        1,
    )?;

    // Three Cell collections overlap: caller/parsed wire, replayed Tile and
    // projected SceneWire. Contributors have one Vec per cell in each copy.
    reserve_raw(
        &mut estimate,
        shape.cell_capacity,
        size_of::<Cell>().max(size_of::<SceneCell>()),
        1,
        LIVE_CELL_COLLECTIONS,
    )?;
    reserve_raw(
        &mut estimate,
        shape.contribution_capacity,
        size_of::<kyberia_spatial_analysis::Contribution>().max(size_of::<SceneContribution>()),
        shape.cell_count,
        LIVE_CELL_COLLECTIONS,
    )?;

    // The two counters are kept in the shape so malformed capacities cannot
    // be optimized away by a caller that reports only lengths.
    debug_assert!(shape.group_id_count <= shape.group_id_capacity);
    debug_assert!(shape.contribution_count <= shape.contribution_capacity);
    Ok(estimate)
}

fn spatial_grid_center(
    grid: &kyberia_spatial_analysis::Grid,
    column: u32,
    row: u32,
) -> Result<(CoordinateMeters, CoordinateMeters), SceneError> {
    let x = (f64::from(grid.column_offset) + f64::from(column) + 0.5) * grid.resolution.get()
        + grid.origin.x.get();
    let y = (f64::from(grid.row_offset) + f64::from(row) + 0.5) * grid.resolution.get()
        + grid.origin.y.get();
    Ok((
        CoordinateMeters::new(x).map_err(|_| SceneError::InvalidTile("grid"))?,
        CoordinateMeters::new(y).map_err(|_| SceneError::InvalidTile("grid"))?,
    ))
}

fn validate_spatial_grid(
    grid: &kyberia_spatial_analysis::Grid,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<usize, SceneError> {
    let count = (grid.width as usize)
        .checked_mul(grid.height as usize)
        .ok_or(SceneError::ResourceLimit("cell count"))?;
    if count == 0 || count > MAX_CELLS {
        return Err(SceneError::ResourceLimit("cell count"));
    }
    if grid.resolution.get() <= 0.0 {
        return Err(SceneError::InvalidTile("grid"));
    }
    if grid.column_offset.checked_add(grid.width).is_none()
        || grid.row_offset.checked_add(grid.height).is_none()
    {
        return Err(SceneError::ResourceLimit("grid offsets"));
    }
    spatial_grid_center(grid, 0, 0)?;
    spatial_grid_center(grid, grid.width - 1, grid.height - 1)?;
    for column in 1..grid.width {
        poll_cancelled(cancelled, column as usize)?;
        if spatial_grid_center(grid, column - 1, 0)?.0 >= spatial_grid_center(grid, column, 0)?.0 {
            return Err(SceneError::InvalidTile("grid"));
        }
    }
    for row in 1..grid.height {
        poll_cancelled(cancelled, row as usize)?;
        if spatial_grid_center(grid, 0, row - 1)?.1 >= spatial_grid_center(grid, 0, row)?.1 {
            return Err(SceneError::InvalidTile("grid"));
        }
    }
    Ok(count)
}

fn validate_shape_limits(shape: SceneResourceShape) -> Result<(), SceneError> {
    if shape.sample_count > MAX_SAMPLES {
        return Err(SceneError::ResourceLimit("samples"));
    }
    if shape.group_count > MAX_LOCATION_GROUPS {
        return Err(SceneError::ResourceLimit("location groups"));
    }
    if shape.cell_count > MAX_CELLS {
        return Err(SceneError::ResourceLimit("cell count"));
    }
    if shape.contribution_count > MAX_TOTAL_CONTRIBUTIONS {
        return Err(SceneError::ResourceLimit("contributions"));
    }
    if shape.group_id_count > MAX_GROUP_ID_REFERENCES || shape.largest_group_id_count > MAX_SAMPLES
    {
        return Err(SceneError::ResourceLimit("group observation ids"));
    }
    if shape
        .cell_count
        .checked_mul(shape.group_count)
        .is_none_or(|work| work > MAX_SCENE_DISTANCE_EVALUATIONS)
    {
        return Err(SceneError::ResourceLimit("scene replay work"));
    }
    Ok(())
}

fn admit_resource_shape(shape: SceneResourceShape) -> Result<(), SceneError> {
    validate_shape_limits(shape)?;
    if estimated_scene_working_bytes(shape)? > MAX_SCENE_WORKING_BYTES {
        return Err(SceneError::ResourceLimit("scene working set"));
    }
    Ok(())
}

#[cfg(test)]
fn admit_shape_resources(
    sample_count: usize,
    group_count: usize,
    group_id_count: usize,
    cell_count: usize,
    contribution_count: usize,
    encoded_bytes: usize,
) -> Result<(), SceneError> {
    admit_resource_shape(SceneResourceShape::from_lengths(
        sample_count,
        group_count,
        group_id_count,
        cell_count,
        contribution_count,
        encoded_bytes,
    ))
}

fn admit_tile_resources(
    tile: &Tile,
    encoded_bytes: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SceneError> {
    let cell_count = validate_spatial_grid(&tile.grid, cancelled)?;
    let mut group_id_count = 0usize;
    let mut group_id_capacity = 0usize;
    let mut largest_group_id_count = 0usize;
    let mut largest_group_id_capacity = 0usize;
    for (index, group) in tile.location_groups.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        group_id_count = group_id_count
            .checked_add(group.observation_ids.len())
            .and_then(|count| count.checked_add(group.signal_aggregate.observation_order.len()))
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
        group_id_capacity = group_id_capacity
            .checked_add(group.observation_ids.capacity())
            .and_then(|capacity| {
                capacity.checked_add(group.signal_aggregate.observation_order.capacity())
            })
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
        largest_group_id_count = largest_group_id_count.max(
            group
                .observation_ids
                .len()
                .max(group.signal_aggregate.observation_order.len()),
        );
        largest_group_id_capacity = largest_group_id_capacity.max(
            group
                .observation_ids
                .capacity()
                .max(group.signal_aggregate.observation_order.capacity()),
        );
    }
    let mut contribution_count = 0usize;
    let mut contribution_capacity = 0usize;
    for (index, cell) in tile.cells.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        contribution_count = contribution_count
            .checked_add(cell.contributors.len())
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
        contribution_capacity = contribution_capacity
            .checked_add(cell.contributors.capacity())
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
    }
    let shape = SceneResourceShape {
        sample_count: tile.inputs.samples.len(),
        sample_capacity: tile.inputs.samples.capacity(),
        group_count: tile.location_groups.len(),
        group_capacity: tile.location_groups.capacity(),
        group_id_count,
        group_id_capacity,
        largest_group_id_count,
        largest_group_id_capacity,
        cell_count,
        cell_capacity: tile.cells.capacity(),
        contribution_count,
        contribution_capacity,
        encoded_bytes,
    };
    admit_resource_shape(shape)
}

fn admit_wire_resources(
    wire: &SceneWire,
    encoded_bytes: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SceneError> {
    let cell_count = wire.grid.validate_with_cancellation(cancelled)?;
    let mut group_id_count = 0usize;
    let mut group_id_capacity = 0usize;
    let mut largest_group_id_count = 0usize;
    let mut largest_group_id_capacity = 0usize;
    for (index, group) in wire.location_groups.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        group_id_count = group_id_count
            .checked_add(group.observation_ids.len())
            .and_then(|count| count.checked_add(group.signal_aggregate.observation_order.len()))
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
        group_id_capacity = group_id_capacity
            .checked_add(group.observation_ids.capacity())
            .and_then(|capacity| {
                capacity.checked_add(group.signal_aggregate.observation_order.capacity())
            })
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
        largest_group_id_count = largest_group_id_count.max(
            group
                .observation_ids
                .len()
                .max(group.signal_aggregate.observation_order.len()),
        );
        largest_group_id_capacity = largest_group_id_capacity.max(
            group
                .observation_ids
                .capacity()
                .max(group.signal_aggregate.observation_order.capacity()),
        );
    }
    let mut contribution_count = 0usize;
    let mut contribution_capacity = 0usize;
    for (index, cell) in wire.cells.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        contribution_count = contribution_count
            .checked_add(cell.contributors.len())
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
        contribution_capacity = contribution_capacity
            .checked_add(cell.contributors.capacity())
            .ok_or(SceneError::ResourceLimit("scene working set"))?;
    }
    let shape = SceneResourceShape {
        sample_count: wire.samples.len(),
        sample_capacity: wire.samples.capacity(),
        group_count: wire.location_groups.len(),
        group_capacity: wire.location_groups.capacity(),
        group_id_count,
        group_id_capacity,
        largest_group_id_count,
        largest_group_id_capacity,
        cell_count,
        cell_capacity: wire.cells.capacity(),
        contribution_count,
        contribution_capacity,
        encoded_bytes,
    };
    admit_resource_shape(shape)
}

struct ScenePreflightState<'a> {
    counts: ScenePreflightCounts,
    cancelled: &'a mut dyn FnMut() -> bool,
    steps: usize,
    cancelled_error: bool,
    resource_error: Option<&'static str>,
    malformed_error: bool,
}

impl ScenePreflightState<'_> {
    fn poll<E: de::Error>(&mut self) -> Result<(), E> {
        self.steps = self.steps.saturating_add(1);
        if self.steps.is_multiple_of(64) && (self.cancelled)() {
            self.cancelled_error = true;
            return Err(E::custom("scene preflight cancelled"));
        }
        Ok(())
    }

    fn depth<E: de::Error>(&mut self, depth: usize) -> Result<(), E> {
        if depth > MAX_SCENE_JSON_DEPTH {
            self.resource_error = Some("scene json depth");
            return Err(E::custom("scene JSON nesting exceeds the bounded depth"));
        }
        Ok(())
    }

    fn text<E: de::Error>(&mut self, value: &str) -> Result<(), E> {
        if value.len() > MAX_TEXT_BYTES {
            self.resource_error = Some("scene text");
            return Err(E::custom("scene text exceeds the bounded length"));
        }
        Ok(())
    }

    fn resource<E: de::Error>(&mut self, label: &'static str) -> E {
        self.resource_error = Some(label);
        E::custom("scene preflight resource limit")
    }

    fn malformed<E: de::Error>(&mut self) -> E {
        self.malformed_error = true;
        E::custom("duplicate known scene field")
    }
}

fn known_scene_field_bit(key: &str) -> Option<u16> {
    Some(match key {
        "schema" => 1 << 0,
        "identity" => 1 << 1,
        "metric_artifact" => 1 << 2,
        "metric_definition_bytes" => 1 << 3,
        "evidence_plane" => 1 << 4,
        "configuration" => 1 << 5,
        "samples" => 1 << 6,
        "grid" => 1 << 7,
        "location_groups" => 1 << 8,
        "cells" => 1 << 9,
        _ => return None,
    })
}

fn known_group_field_bit(key: &str) -> Option<u8> {
    Some(match key {
        "position" => 1 << 0,
        "observation_ids" => 1 << 1,
        "signal_aggregate" => 1 << 2,
        _ => return None,
    })
}

fn known_aggregate_field_bit(key: &str) -> Option<u8> {
    Some(match key {
        "algorithm_version" => 1 << 0,
        "method" => 1 << 1,
        "estimate" => 1 << 2,
        "percentile_interval" => 1 << 3,
        "sample_count" => 1 << 4,
        "observation_order" => 1 << 5,
        _ => return None,
    })
}

fn known_cell_field_bit(key: &str) -> Option<u8> {
    Some(match key {
        "value" => 1 << 0,
        "class" => 1 << 1,
        "support_locations" => 1 << 2,
        "support_observations" => 1 << 3,
        "nearest_distance" => 1 << 4,
        "uncertainty_db" => 1 << 5,
        "contributors" => 1 << 6,
        _ => return None,
    })
}

struct SkipSceneValueSeed<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::DeserializeSeed<'de> for SkipSceneValueSeed<'s, 'c> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(BoundedSceneValueVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct BoundedSceneValueVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::Visitor<'de> for BoundedSceneValueVisitor<'s, 'c> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded JSON value")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.state.depth::<E>(self.depth)?;
        self.state.poll()?;
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.state.depth::<E>(self.depth)?;
        self.state.poll()?;
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.state.depth::<E>(self.depth)?;
        self.state.poll()?;
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.state.depth::<E>(self.depth)?;
        self.state.poll()?;
        Ok(())
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.state.depth::<E>(self.depth)?;
        self.state.text(value)?;
        self.state.poll()?;
        Ok(())
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(value)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_unit()
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.state.depth::<E>(self.depth)?;
        self.state.poll()?;
        Ok(())
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        while let Some(()) = sequence.next_element_seed(SkipSceneValueSeed {
            state: &mut *self.state,
            depth: self.depth.saturating_add(1),
        })? {
            self.state.poll()?;
        }
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        while let Some(key) = map.next_key::<String>()? {
            self.state.text::<A::Error>(&key)?;
            self.state.poll()?;
            map.next_value_seed(SkipSceneValueSeed {
                state: &mut *self.state,
                depth: self.depth.saturating_add(1),
            })?;
        }
        Ok(())
    }
}

struct CountSceneArraySeed<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
    maximum: usize,
    label: &'static str,
}

impl<'de, 's, 'c> de::DeserializeSeed<'de> for CountSceneArraySeed<'s, 'c> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_seq(CountSceneArrayVisitor {
            state: self.state,
            depth: self.depth,
            maximum: self.maximum,
            label: self.label,
        })
    }
}

struct CountSceneArrayVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
    maximum: usize,
    label: &'static str,
}

impl<'de, 's, 'c> de::Visitor<'de> for CountSceneArrayVisitor<'s, 'c> {
    type Value = usize;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded JSON array")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        let mut count = 0usize;
        while let Some(()) = sequence.next_element_seed(SkipSceneValueSeed {
            state: &mut *self.state,
            depth: self.depth.saturating_add(1),
        })? {
            self.state.poll()?;
            count = count
                .checked_add(1)
                .ok_or_else(|| self.state.resource::<A::Error>(self.label))?;
            if count > self.maximum {
                return Err(self.state.resource::<A::Error>(self.label));
            }
        }
        Ok(count)
    }
}

struct SceneGroupPreflightSeed<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::DeserializeSeed<'de> for SceneGroupPreflightSeed<'s, 'c> {
    type Value = (usize, usize);

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_map(SceneGroupPreflightVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct SceneGroupPreflightVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::Visitor<'de> for SceneGroupPreflightVisitor<'s, 'c> {
    type Value = (usize, usize);

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded location group")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        let mut observation_ids = 0usize;
        let mut observation_order = 0usize;
        let mut seen = 0u8;
        while let Some(key) = map.next_key::<String>()? {
            self.state.text::<A::Error>(&key)?;
            self.state.poll()?;
            if let Some(bit) = known_group_field_bit(&key) {
                if seen & bit != 0 {
                    return Err(self.state.malformed());
                }
                seen |= bit;
            }
            match key.as_str() {
                "observation_ids" => {
                    observation_ids = map.next_value_seed(CountSceneArraySeed {
                        state: &mut *self.state,
                        depth: self.depth.saturating_add(1),
                        maximum: MAX_SAMPLES,
                        label: "group observation ids",
                    })?;
                }
                "signal_aggregate" => {
                    observation_order = map.next_value_seed(SceneAggregatePreflightSeed {
                        state: &mut *self.state,
                        depth: self.depth.saturating_add(1),
                    })?;
                }
                _ => {
                    map.next_value_seed(SkipSceneValueSeed {
                        state: &mut *self.state,
                        depth: self.depth.saturating_add(1),
                    })?;
                }
            }
        }
        let total = observation_ids
            .checked_add(observation_order)
            .ok_or_else(|| self.state.resource::<A::Error>("group observation ids"))?;
        if total > MAX_GROUP_ID_REFERENCES {
            return Err(self.state.resource::<A::Error>("group observation ids"));
        }
        Ok((observation_ids, observation_order))
    }
}

struct SceneAggregatePreflightSeed<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::DeserializeSeed<'de> for SceneAggregatePreflightSeed<'s, 'c> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_map(SceneAggregatePreflightVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct SceneAggregatePreflightVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::Visitor<'de> for SceneAggregatePreflightVisitor<'s, 'c> {
    type Value = usize;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded signal aggregate")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        let mut observation_order = 0usize;
        let mut seen = 0u8;
        while let Some(key) = map.next_key::<String>()? {
            self.state.text::<A::Error>(&key)?;
            self.state.poll()?;
            if let Some(bit) = known_aggregate_field_bit(&key) {
                if seen & bit != 0 {
                    return Err(self.state.malformed());
                }
                seen |= bit;
            }
            if key == "observation_order" {
                observation_order = map.next_value_seed(CountSceneArraySeed {
                    state: &mut *self.state,
                    depth: self.depth.saturating_add(1),
                    maximum: MAX_SAMPLES,
                    label: "group observation order",
                })?;
            } else {
                map.next_value_seed(SkipSceneValueSeed {
                    state: &mut *self.state,
                    depth: self.depth.saturating_add(1),
                })?;
            }
        }
        Ok(observation_order)
    }
}

struct SceneCellPreflightSeed<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::DeserializeSeed<'de> for SceneCellPreflightSeed<'s, 'c> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_map(SceneCellPreflightVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct SceneCellPreflightVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::Visitor<'de> for SceneCellPreflightVisitor<'s, 'c> {
    type Value = usize;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded scene cell")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        let mut contributions = 0usize;
        let mut seen = 0u8;
        while let Some(key) = map.next_key::<String>()? {
            self.state.text::<A::Error>(&key)?;
            self.state.poll()?;
            if let Some(bit) = known_cell_field_bit(&key) {
                if seen & bit != 0 {
                    return Err(self.state.malformed());
                }
                seen |= bit;
            }
            if key == "contributors" {
                contributions = map.next_value_seed(CountSceneArraySeed {
                    state: &mut *self.state,
                    depth: self.depth.saturating_add(1),
                    maximum: MAX_NEIGHBORS,
                    label: "contributors",
                })?;
            } else {
                map.next_value_seed(SkipSceneValueSeed {
                    state: &mut *self.state,
                    depth: self.depth.saturating_add(1),
                })?;
            }
        }
        Ok(contributions)
    }
}

struct SceneRootPreflightVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
}

impl<'de, 's, 'c> de::Visitor<'de> for SceneRootPreflightVisitor<'s, 'c> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a renderer scene object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut seen = 0u16;
        while let Some(key) = map.next_key::<String>()? {
            self.state.text::<A::Error>(&key)?;
            self.state.poll()?;
            if let Some(bit) = known_scene_field_bit(&key) {
                if seen & bit != 0 {
                    return Err(self.state.malformed());
                }
                seen |= bit;
            }
            match key.as_str() {
                "samples" => {
                    let count = map.next_value_seed(CountSceneArraySeed {
                        state: &mut *self.state,
                        depth: 1,
                        maximum: MAX_SAMPLES,
                        label: "samples",
                    })?;
                    self.state.counts.sample_count = count;
                }
                "location_groups" => {
                    let groups = map.next_value_seed(SceneGroupsPreflightSeed {
                        state: &mut *self.state,
                        depth: 1,
                    })?;
                    self.state.counts.group_count = groups;
                }
                "cells" => {
                    let cells = map.next_value_seed(SceneCellsPreflightSeed {
                        state: &mut *self.state,
                        depth: 1,
                    })?;
                    self.state.counts.cell_count = cells;
                }
                "metric_definition_bytes" => {
                    self.state.counts.metric_definition_bytes =
                        map.next_value_seed(CountSceneArraySeed {
                            state: &mut *self.state,
                            depth: 1,
                            maximum: MAX_METRIC_DEFINITION_BYTES,
                            label: "metric definition bytes",
                        })?;
                }
                _ => {
                    map.next_value_seed(SkipSceneValueSeed {
                        state: &mut *self.state,
                        depth: 1,
                    })?;
                }
            }
        }
        Ok(())
    }
}

struct SceneGroupsPreflightSeed<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::DeserializeSeed<'de> for SceneGroupsPreflightSeed<'s, 'c> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_seq(SceneGroupsPreflightVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct SceneGroupsPreflightVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::Visitor<'de> for SceneGroupsPreflightVisitor<'s, 'c> {
    type Value = usize;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded location group array")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        let mut count = 0usize;
        while let Some((observation_ids, observation_order)) =
            sequence.next_element_seed(SceneGroupPreflightSeed {
                state: &mut *self.state,
                depth: self.depth.saturating_add(1),
            })?
        {
            self.state.poll()?;
            count = count
                .checked_add(1)
                .ok_or_else(|| self.state.resource::<A::Error>("location groups"))?;
            if count > MAX_LOCATION_GROUPS {
                return Err(self.state.resource::<A::Error>("location groups"));
            }
            let total = observation_ids
                .checked_add(observation_order)
                .ok_or_else(|| self.state.resource::<A::Error>("group observation ids"))?;
            self.state.counts.group_id_count = self
                .state
                .counts
                .group_id_count
                .checked_add(total)
                .ok_or_else(|| self.state.resource::<A::Error>("group observation ids"))?;
            if self.state.counts.group_id_count > MAX_GROUP_ID_REFERENCES {
                return Err(self.state.resource::<A::Error>("group observation ids"));
            }
            self.state.counts.largest_group_id_count = self
                .state
                .counts
                .largest_group_id_count
                .max(observation_ids.max(observation_order));
        }
        Ok(count)
    }
}

struct SceneCellsPreflightSeed<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::DeserializeSeed<'de> for SceneCellsPreflightSeed<'s, 'c> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_seq(SceneCellsPreflightVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct SceneCellsPreflightVisitor<'s, 'c> {
    state: &'s mut ScenePreflightState<'c>,
    depth: usize,
}

impl<'de, 's, 'c> de::Visitor<'de> for SceneCellsPreflightVisitor<'s, 'c> {
    type Value = usize;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded scene cell array")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        self.state.depth::<A::Error>(self.depth)?;
        let mut count = 0usize;
        while let Some(contributions) = sequence.next_element_seed(SceneCellPreflightSeed {
            state: &mut *self.state,
            depth: self.depth.saturating_add(1),
        })? {
            self.state.poll()?;
            count = count
                .checked_add(1)
                .ok_or_else(|| self.state.resource::<A::Error>("cell count"))?;
            if count > MAX_CELLS {
                return Err(self.state.resource::<A::Error>("cell count"));
            }
            self.state.counts.contribution_count = self
                .state
                .counts
                .contribution_count
                .checked_add(contributions)
                .ok_or_else(|| self.state.resource::<A::Error>("contributions"))?;
            if self.state.counts.contribution_count > MAX_TOTAL_CONTRIBUTIONS {
                return Err(self.state.resource::<A::Error>("contributions"));
            }
        }
        Ok(count)
    }
}

/// Check JSON strings before Serde is allowed to materialize owned `String`
/// values. This lexical pass is deliberately conservative: an escaped string
/// is charged by its encoded representation, so a valid but unusually verbose
/// spelling can be rejected at the same bounded text limit. It also polls
/// while traversing long strings, rather than waiting for Serde to finish an
/// allocation and hand the value to a visitor.
fn preflight_json_strings(
    bytes: &[u8],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SceneError> {
    let mut index = 0usize;
    let mut scanned = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            scanned = scanned.saturating_add(1);
            if scanned.is_multiple_of(256) && cancelled() {
                return Err(SceneError::Cancelled);
            }
            continue;
        }

        index += 1;
        let mut string_bytes = 0usize;
        let mut terminated = false;
        while index < bytes.len() {
            scanned = scanned.saturating_add(1);
            if scanned.is_multiple_of(256) && cancelled() {
                return Err(SceneError::Cancelled);
            }
            match bytes[index] {
                b'"' => {
                    index += 1;
                    terminated = true;
                    break;
                }
                b'\\' => {
                    index += 1;
                    let Some(&escaped) = bytes.get(index) else {
                        return Err(SceneError::MalformedBytes);
                    };
                    if escaped == b'u' {
                        let end = index.checked_add(4).ok_or(SceneError::MalformedBytes)?;
                        if end >= bytes.len()
                            || bytes[index + 1..=end]
                                .iter()
                                .any(|digit| !digit.is_ascii_hexdigit())
                        {
                            return Err(SceneError::MalformedBytes);
                        }
                        string_bytes = string_bytes.saturating_add(4);
                        index = end + 1;
                    } else if matches!(
                        escaped,
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't'
                    ) {
                        string_bytes = string_bytes.saturating_add(1);
                        index += 1;
                    } else {
                        return Err(SceneError::MalformedBytes);
                    }
                }
                byte if byte < 0x20 => return Err(SceneError::MalformedBytes),
                _ => {
                    string_bytes = string_bytes.saturating_add(1);
                    index += 1;
                }
            }
            if string_bytes > MAX_TEXT_BYTES {
                return Err(SceneError::ResourceLimit("scene text"));
            }
        }
        if !terminated {
            return Err(SceneError::MalformedBytes);
        }
    }
    Ok(())
}

fn preflight_scene(
    bytes: &[u8],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<SceneResourceShape, SceneError> {
    if bytes.len() > MAX_SCENE_BYTES {
        return Err(SceneError::ResourceLimit("scene bytes"));
    }
    if cancelled() {
        return Err(SceneError::Cancelled);
    }
    preflight_json_strings(bytes, cancelled)?;
    let mut state = ScenePreflightState {
        counts: ScenePreflightCounts::default(),
        cancelled,
        steps: 0,
        cancelled_error: false,
        resource_error: None,
        malformed_error: false,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let parsed = deserializer.deserialize_map(SceneRootPreflightVisitor { state: &mut state });
    if state.cancelled_error {
        return Err(SceneError::Cancelled);
    }
    if let Some(label) = state.resource_error {
        return Err(SceneError::ResourceLimit(label));
    }
    if state.malformed_error || parsed.is_err() {
        return Err(SceneError::MalformedBytes);
    }
    if deserializer.end().is_err() {
        return Err(SceneError::MalformedBytes);
    }
    shape_from_preflight(state.counts, bytes.len())
}

/// Poll decoding without conflating cancellation with malformed input.
fn decode_scene(
    bytes: &[u8],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<SceneWire, SceneError> {
    struct Reader<'a, F> {
        bytes: &'a [u8],
        offset: usize,
        next_poll: usize,
        interrupted: bool,
        cancelled: F,
    }
    impl<F: FnMut() -> bool> std::io::Read for Reader<'_, F> {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            if output.is_empty() {
                return Ok(0);
            }
            if self.interrupted {
                return Err(std::io::Error::other("scene decode cancelled"));
            }
            if self.offset >= self.next_poll {
                if (self.cancelled)() {
                    self.interrupted = true;
                    return Err(std::io::Error::other("scene decode cancelled"));
                }
                self.next_poll = self.offset.saturating_add(4096);
            }
            let count = output
                .len()
                .min(self.bytes.len() - self.offset)
                .min(self.next_poll - self.offset);
            output[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        }
    }
    let mut reader = Reader {
        bytes,
        offset: 0,
        next_poll: 0,
        interrupted: false,
        cancelled,
    };
    serde_json::from_reader(&mut reader).map_err(|_| {
        if reader.interrupted {
            SceneError::Cancelled
        } else {
            SceneError::MalformedBytes
        }
    })
}

fn wire_from_tile(
    tile: &Tile,
    encoded_bytes: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<SceneWire, SceneError> {
    admit_tile_resources(tile, encoded_bytes, cancelled)?;
    let definition = tile.inputs.metric_definition.definition();
    let identity = SceneIdentity {
        metric_id: definition.id().clone(),
        metric_version: definition.version().clone(),
        metric_revision: definition.revision().get(),
        signal_aggregation: tile.signal_aggregation(),
        metric_definition_hash: tile
            .inputs
            .metric_definition
            .definition_hash()
            .map_err(|_| SceneError::InvalidTile("metric definition hash"))?,
        source_artifact: tile.inputs.source_artifact.clone(),
        spatial_schema: Text::new(tile.schema_version)
            .map_err(|_| SceneError::InvalidTile("tile schema"))?,
        algorithm_version: Text::new(tile.algorithm_version)
            .map_err(|_| SceneError::InvalidTile("algorithm version"))?,
    };
    let grid = SceneGrid {
        floor_id: tile.grid.floor_id,
        frame_id: tile.grid.frame_id,
        origin: ScenePoint {
            x: tile.grid.origin.x,
            y: tile.grid.origin.y,
        },
        resolution: tile.grid.resolution,
        column_offset: tile.grid.column_offset,
        row_offset: tile.grid.row_offset,
        width: tile.grid.width,
        height: tile.grid.height,
    };
    let mut location_groups = Vec::with_capacity(tile.location_groups.len());
    for (index, group) in tile.location_groups.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        let mut observation_ids = Vec::with_capacity(group.observation_ids.len());
        for (id_index, observation_id) in group.observation_ids.iter().enumerate() {
            poll_cancelled(cancelled, id_index)?;
            observation_ids.push(*observation_id);
        }
        location_groups.push(SceneLocationGroup {
            position: ScenePoint {
                x: group.position.x,
                y: group.position.y,
            },
            observation_ids,
            signal_aggregate: group.signal_aggregate.clone(),
        });
    }
    let mut cells = Vec::with_capacity(tile.cells.len());
    for (index, cell) in tile.cells.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        cells.push(scene_cell_from_tile(cell, cancelled)?);
    }
    let mut samples = Vec::with_capacity(tile.inputs.samples.len());
    for (index, sample) in tile.inputs.samples.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        samples.push(sample.clone());
    }
    Ok(SceneWire {
        schema: SceneSchemaVersion::V1,
        identity,
        metric_artifact: tile.inputs.metric_definition.artifact().clone(),
        metric_definition_bytes: tile
            .inputs
            .metric_definition
            .canonical_bytes()
            .map_err(|_| SceneError::InvalidTile("metric definition bytes"))?,
        evidence_plane: tile.inputs.evidence_plane,
        configuration: tile.configuration,
        samples,
        grid,
        location_groups,
        cells,
    })
}

fn scene_cell_from_tile(
    cell: &Cell,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<SceneCell, SceneError> {
    let mut contributors = Vec::with_capacity(cell.contributors.len());
    for (index, contribution) in cell.contributors.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        contributors.push(SceneContribution {
            location_group: ExactU64::new(contribution.location_group as u64),
            weight: contribution.weight,
        });
    }
    Ok(SceneCell {
        value: cell.value.clone(),
        class: match cell.class {
            CellClass::Observed => SceneCellClass::Observed,
            CellClass::Interpolated => SceneCellClass::Interpolated,
            CellClass::Extrapolated => SceneCellClass::Extrapolated,
            CellClass::Unknown => SceneCellClass::Unknown,
        },
        support_locations: ExactU64::new(cell.support_locations as u64),
        support_observations: ExactU64::new(cell.support_observations as u64),
        nearest_distance: cell.nearest_distance.clone(),
        uncertainty_db: cell.uncertainty_db.clone(),
        contributors,
    })
}

fn validate_tile_with_cancellation(
    tile: &Tile,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SceneError> {
    if tile.schema_version != TILE_SCHEMA_VERSION {
        return Err(SceneError::UnsupportedVersion);
    }
    if tile.algorithm_version != ALGORITHM_VERSION {
        return Err(SceneError::UnsupportedVersion);
    }
    let count = validate_spatial_grid(&tile.grid, cancelled)?;
    if tile.cells.len() != count {
        return Err(SceneError::InvalidTile("cell count"));
    }
    if tile.grid.floor_id != tile.inputs.floor_id {
        return Err(SceneError::InvalidTile("floor mismatch"));
    }
    if tile.grid.frame_id != tile.inputs.frame_id {
        return Err(SceneError::InvalidTile("frame mismatch"));
    }
    tile.configuration
        .validate()
        .map_err(|_| SceneError::InvalidTile("configuration"))?;
    if tile.location_groups.len() > MAX_LOCATION_GROUPS
        || tile.inputs.samples.len() > MAX_SAMPLES
        || tile.inputs.source_artifact.byte_length > MAX_SOURCE_ARTIFACT_BYTES
    {
        return Err(SceneError::ResourceLimit("tile evidence"));
    }
    if tile.signal_aggregation() != tile.inputs.metric_definition.signal_aggregation() {
        return Err(SceneError::InvalidTile("aggregation selection mismatch"));
    }
    tile.inputs
        .metric_definition
        .signal_aggregation()
        .validate()
        .map_err(|_| SceneError::InvalidTile("aggregation selection"))?;

    let mut samples = BTreeMap::new();
    for (index, sample) in tile.inputs.samples.iter().enumerate() {
        poll_cancelled(cancelled, index)?;
        if sample.floor_id != tile.inputs.floor_id || sample.frame_id != tile.inputs.frame_id {
            return Err(SceneError::InvalidTile("sample frame"));
        }
        if samples.insert(sample.observation_id, sample).is_some() {
            return Err(SceneError::InvalidTile("duplicate sample"));
        }
    }

    let mut grouped = BTreeSet::new();
    for (group_index, group) in tile.location_groups.iter().enumerate() {
        poll_cancelled(cancelled, group_index)?;
        if group.signal_aggregate.method != tile.signal_aggregation().method {
            return Err(SceneError::InvalidTile("group aggregation method mismatch"));
        }
        if group.observation_ids.is_empty() {
            return Err(SceneError::InvalidTile("empty location group"));
        }
        let mut ids = BTreeSet::new();
        for (id_index, observation_id) in group.observation_ids.iter().enumerate() {
            poll_cancelled(cancelled, id_index)?;
            if !ids.insert(*observation_id) || !grouped.insert(*observation_id) {
                return Err(SceneError::InvalidTile("duplicate grouped observation"));
            }
            let sample = samples
                .get(observation_id)
                .ok_or(SceneError::InvalidTile("group observation missing"))?;
            if sample.value.as_known().is_none() || sample.position != group.position {
                return Err(SceneError::InvalidTile("group/sample mismatch"));
            }
        }
        let mut aggregate_samples = Vec::with_capacity(group.observation_ids.len());
        for (id_index, id) in group.observation_ids.iter().enumerate() {
            poll_cancelled(cancelled, id_index)?;
            let sample = samples[id];
            aggregate_samples.push(kyberia_wifi_semantics::StaticSignalSample {
                observation_id: sample.observation_id,
                rssi: *sample.value.as_known().expect("checked above"),
            });
        }
        let expected = kyberia_wifi_semantics::aggregate_static(
            &aggregate_samples,
            group.signal_aggregate.method,
        )
        .map_err(|_| SceneError::InvalidTile("group aggregate"))?;
        if expected != group.signal_aggregate {
            return Err(SceneError::InvalidTile("group aggregate mismatch"));
        }
    }
    let known_count = samples
        .values()
        .filter(|sample| sample.value.as_known().is_some())
        .count();
    if grouped.len() != known_count {
        return Err(SceneError::InvalidTile("known sample missing from groups"));
    }

    let mut total_contributions = 0usize;
    for (cell_index, cell) in tile.cells.iter().enumerate() {
        poll_cancelled(cancelled, cell_index)?;
        total_contributions = total_contributions
            .checked_add(cell.contributors.len())
            .ok_or(SceneError::ResourceLimit("contributions"))?;
    }
    if total_contributions > MAX_TOTAL_CONTRIBUTIONS {
        return Err(SceneError::ResourceLimit("contributions"));
    }
    for (cell_index, cell) in tile.cells.iter().enumerate() {
        poll_cancelled(cancelled, cell_index)?;
        validate_cell(cell, tile.location_groups.len(), tile.configuration)
            .map_err(SceneError::InvalidTile)?;
        if cell.class == CellClass::Observed
            && (cell.contributors.len() != 1
                || cell.value
                    != tile.location_groups[cell.contributors[0].location_group]
                        .signal_aggregate
                        .estimate)
        {
            return Err(SceneError::InvalidTile(
                "observed value differs from measurement group",
            ));
        }
    }
    Ok(())
}

fn validate_cell(cell: &Cell, group_count: usize, config: Config) -> Result<(), &'static str> {
    if cell.support_locations > group_count || cell.support_observations > MAX_SAMPLES {
        return Err("support count");
    }
    match (&cell.value, cell.class) {
        (Evidence::Known(_), CellClass::Unknown)
        | (Evidence::Unknown(_), CellClass::Observed)
        | (Evidence::Unknown(_), CellClass::Interpolated)
        | (Evidence::Unknown(_), CellClass::Extrapolated) => {
            return Err("value/class mismatch");
        }
        (Evidence::Unknown(_), CellClass::Unknown) if !cell.contributors.is_empty() => {
            return Err("unknown contributors");
        }
        (Evidence::Known(_), _) if cell.contributors.is_empty() => {
            return Err("known contributors");
        }
        _ => {}
    }
    if cell.contributors.len() > config.maximum_neighbors {
        return Err("contributor count");
    }
    let mut sum = 0.0;
    let mut contributor_ids = BTreeSet::new();
    for contribution in &cell.contributors {
        if !contributor_ids.insert(contribution.location_group) {
            return Err("duplicate contributor");
        }
        if contribution.location_group >= group_count {
            return Err("contributor index");
        }
        if contribution.weight.get() <= 0.0 {
            return Err("contributor weight");
        }
        sum += contribution.weight.get();
    }
    if !cell.contributors.is_empty() && (sum - 1.0).abs() > 1e-9 {
        return Err("contributor normalization");
    }
    Ok(())
}

/// Rebuild numerical truth from the preserved canonical samples. Artifact hashes
/// identify evidence but do not authenticate it or prove a source decoder's work.
fn validate_wire_computation(
    wire: &SceneWire,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SceneError> {
    use kyberia_spatial_analysis::{Grid, Inputs, MetricDefinitionBinding, Model, Point2};
    if wire.samples.len() > MAX_SAMPLES {
        return Err(SceneError::ResourceLimit("samples"));
    }
    let binding = MetricDefinitionBinding::from_artifact_bytes(
        wire.metric_artifact.clone(),
        &wire.metric_definition_bytes,
        wire.identity.signal_aggregation,
    )
    .map_err(|_| SceneError::InvalidDocument("metric definition binding"))?;
    let model = Model::new(
        Inputs {
            floor_id: wire.grid.floor_id,
            frame_id: wire.grid.frame_id,
            evidence_plane: wire.evidence_plane,
            metric_definition: binding,
            source_artifact: wire.identity.source_artifact.clone(),
            samples: wire.samples.clone(),
        },
        wire.configuration,
    )
    .map_err(|_| SceneError::InvalidDocument("canonical model inputs"))?;
    let tile = model
        .tile(
            Grid {
                floor_id: wire.grid.floor_id,
                frame_id: wire.grid.frame_id,
                origin: Point2 {
                    x: wire.grid.origin.x,
                    y: wire.grid.origin.y,
                },
                resolution: wire.grid.resolution,
                column_offset: wire.grid.column_offset,
                row_offset: wire.grid.row_offset,
                width: wire.grid.width,
                height: wire.grid.height,
            },
            &mut *cancelled,
        )
        .map_err(|error| match error {
            kyberia_spatial_analysis::Error::ResourceLimit(reason) => {
                SceneError::ResourceLimit(reason)
            }
            kyberia_spatial_analysis::Error::Cancelled => SceneError::Cancelled,
            _ => SceneError::InvalidDocument("canonical tile replay"),
        })?;
    if cancelled() {
        return Err(SceneError::Cancelled);
    }
    if wire_from_tile(&tile, 0, cancelled)? != *wire {
        return Err(SceneError::InvalidDocument(
            "scene differs from canonical computation",
        ));
    }
    Ok(())
}

fn validate_wire_with_cancellation(
    wire: &SceneWire,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), SceneError> {
    wire.configuration
        .validate()
        .map_err(|_| SceneError::InvalidDocument("configuration"))?;
    let binding = kyberia_spatial_analysis::MetricDefinitionBinding::from_artifact_bytes(
        wire.metric_artifact.clone(),
        &wire.metric_definition_bytes,
        wire.identity.signal_aggregation,
    )
    .map_err(|_| SceneError::InvalidDocument("metric definition binding"))?;
    let definition = binding.definition();
    if let Some(method) = binding.spatial_method() {
        use kyberia_spatial_analysis::{Method, SpatialMethod};
        if !matches!(
            (method, wire.configuration.method),
            (SpatialMethod::PointValue, Method::PointValue)
                | (SpatialMethod::Nearest, Method::Nearest)
                | (SpatialMethod::InverseDistanceWeighted, Method::Idw { .. })
        ) {
            return Err(SceneError::InvalidDocument(
                "metric spatial method mismatch",
            ));
        }
    }
    if definition.id() != &wire.identity.metric_id
        || definition.version() != &wire.identity.metric_version
        || definition.revision().get() != wire.identity.metric_revision
        || binding
            .definition_hash()
            .map_err(|_| SceneError::InvalidDocument("metric definition hash"))?
            != wire.identity.metric_definition_hash
    {
        return Err(SceneError::InvalidDocument("metric identity mismatch"));
    }
    if wire.schema != SceneSchemaVersion::V1 {
        return Err(SceneError::UnsupportedVersion);
    }
    if wire.identity.spatial_schema.as_str() != TILE_SCHEMA_VERSION
        || wire.identity.algorithm_version.as_str() != ALGORITHM_VERSION
    {
        return Err(SceneError::UnsupportedVersion);
    }
    if wire.identity.source_artifact.byte_length > MAX_SOURCE_ARTIFACT_BYTES {
        return Err(SceneError::ResourceLimit("source artifact"));
    }
    let count = wire.grid.validate_with_cancellation(cancelled)?;
    if wire.cells.len() != count {
        return Err(SceneError::InvalidDocument("cell count"));
    }
    if wire.location_groups.len() > MAX_LOCATION_GROUPS {
        return Err(SceneError::ResourceLimit("location groups"));
    }
    let mut observations = BTreeSet::new();
    wire.identity
        .signal_aggregation
        .validate_for_spatial()
        .map_err(|_| SceneError::InvalidDocument("aggregation selection"))?;
    for (group_index, group) in wire.location_groups.iter().enumerate() {
        poll_cancelled(cancelled, group_index)?;
        if group.signal_aggregate.method != wire.identity.signal_aggregation.method {
            return Err(SceneError::InvalidDocument(
                "group aggregation method mismatch",
            ));
        }
        if group.observation_ids.is_empty() {
            return Err(SceneError::InvalidDocument("empty location group"));
        }
        if group.signal_aggregate.sample_count as usize != group.observation_ids.len()
            || group.signal_aggregate.observation_order != group.observation_ids
        {
            return Err(SceneError::InvalidDocument("aggregate provenance"));
        }
        for (id_index, observation_id) in group.observation_ids.iter().enumerate() {
            poll_cancelled(cancelled, id_index)?;
            if !observations.insert(*observation_id) {
                return Err(SceneError::InvalidDocument("duplicate observation"));
            }
        }
    }
    if wire.cells.iter().try_fold(0usize, |sum, cell| {
        sum.checked_add(cell.contributors.len())
            .ok_or(SceneError::ResourceLimit("contributions"))
    })? > MAX_TOTAL_CONTRIBUTIONS
    {
        return Err(SceneError::ResourceLimit("contributions"));
    }
    for (cell_index, cell) in wire.cells.iter().enumerate() {
        poll_cancelled(cancelled, cell_index)?;
        if cell.class == SceneCellClass::Extrapolated
            && wire.configuration.extrapolation == kyberia_spatial_analysis::Extrapolation::Disabled
        {
            return Err(SceneError::InvalidDocument("extrapolation is disabled"));
        }
        if cell.support_locations.get() > wire.location_groups.len() as u64
            || cell.support_observations.get() > MAX_SAMPLES as u64
        {
            return Err(SceneError::InvalidDocument("support count"));
        }
        match (&cell.value, cell.class) {
            (Evidence::Known(_), SceneCellClass::Unknown)
            | (Evidence::Unknown(_), SceneCellClass::Observed)
            | (Evidence::Unknown(_), SceneCellClass::Interpolated)
            | (Evidence::Unknown(_), SceneCellClass::Extrapolated) => {
                return Err(SceneError::InvalidDocument("value/class mismatch"));
            }
            (Evidence::Unknown(_), SceneCellClass::Unknown) if !cell.contributors.is_empty() => {
                return Err(SceneError::InvalidDocument("unknown contributors"));
            }
            (Evidence::Known(_), _) if cell.contributors.is_empty() => {
                return Err(SceneError::InvalidDocument("known contributors"));
            }
            _ => {}
        }
        let mut sum = 0.0;
        let mut contributor_ids = BTreeSet::new();
        for (contribution_index, contribution) in cell.contributors.iter().enumerate() {
            poll_cancelled(cancelled, contribution_index)?;
            let index = contribution.location_group.get();
            if !contributor_ids.insert(index) {
                return Err(SceneError::InvalidDocument("duplicate contributor"));
            }
            if index >= wire.location_groups.len() as u64 || contribution.weight.get() <= 0.0 {
                return Err(SceneError::InvalidDocument("contributor"));
            }
            sum += contribution.weight.get();
        }
        if !cell.contributors.is_empty() && (sum - 1.0).abs() > 1e-9 {
            return Err(SceneError::InvalidDocument("contributor normalization"));
        }
        if cell.class == SceneCellClass::Observed
            && (cell.contributors.len() != 1
                || cell.value
                    != wire.location_groups[cell.contributors[0].location_group.get() as usize]
                        .signal_aggregate
                        .estimate)
        {
            return Err(SceneError::InvalidDocument(
                "observed value differs from measurement group",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyberia_domain::evidence::UnknownReason;
    use kyberia_spatial_analysis::{Contribution, Extrapolation, Method};

    #[test]
    fn duplicate_contributors_cannot_masquerade_as_independent_support() {
        let config = Config {
            method: Method::Idw { power: 2.0 },
            support_radius: Meters::new(5.0).unwrap(),
            minimum_locations: 1,
            maximum_neighbors: 8,
            extrapolation: Extrapolation::Disabled,
        };
        let mut cell = Cell {
            value: Evidence::Known(Dbm::new(-50.0).unwrap()),
            class: CellClass::Interpolated,
            support_locations: 2,
            support_observations: 2,
            nearest_distance: Evidence::Known(Meters::new(1.0).unwrap()),
            uncertainty_db: Evidence::Unknown(UnknownReason::NotMeasured),
            contributors: vec![
                Contribution {
                    location_group: 0,
                    weight: Probability::new(0.5).unwrap(),
                },
                Contribution {
                    location_group: 1,
                    weight: Probability::new(0.5).unwrap(),
                },
            ],
        };
        assert_eq!(validate_cell(&cell, 2, config), Ok(()));
        cell.contributors[1].location_group = 0;
        assert_eq!(
            validate_cell(&cell, 2, config),
            Err("duplicate contributor")
        );
    }
}

fn bounded_scene_bytes_with_cancellation(
    value: &impl Serialize,
    limit: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Vec<u8>, SceneError> {
    struct Writer<F> {
        bytes: Vec<u8>,
        limit: usize,
        exceeded: bool,
        interrupted: bool,
        cancelled: F,
    }
    impl<F: FnMut() -> bool> std::io::Write for Writer<F> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if (self.cancelled)() {
                self.interrupted = true;
                return Err(std::io::Error::other("scene serialization cancelled"));
            }
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                self.exceeded = true;
                return Err(std::io::Error::other("scene serialization limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: Vec::new(),
        limit,
        exceeded: false,
        interrupted: false,
        cancelled,
    };
    if serde_json::to_writer(&mut writer, value).is_err() {
        return Err(if writer.interrupted {
            SceneError::Cancelled
        } else if writer.exceeded {
            SceneError::ResourceLimit("scene bytes")
        } else {
            SceneError::MalformedBytes
        });
    }
    Ok(writer.bytes)
}

fn hash_bytes_with_cancellation(
    bytes: &[u8],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<[u8; 32], SceneError> {
    let mut hasher = Sha256::new();
    for (index, chunk) in bytes.chunks(64 * 1024).enumerate() {
        poll_cancelled(cancelled, index)?;
        hasher.update(chunk);
    }
    if cancelled() {
        return Err(SceneError::Cancelled);
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod serialization_budget_tests {
    use super::*;
    #[test]
    fn serialized_byte_budget_accepts_exact_limit_and_rejects_overflow() {
        let value = "quotes: \" and newline\n";
        let expected = serde_json::to_vec(value).unwrap();
        assert_eq!(
            bounded_scene_bytes_with_cancellation(&value, expected.len(), &mut || false).unwrap(),
            expected
        );
        assert!(matches!(
            bounded_scene_bytes_with_cancellation(&value, expected.len() - 1, &mut || false),
            Err(SceneError::ResourceLimit("scene bytes"))
        ));
        assert!(matches!(
            bounded_scene_bytes_with_cancellation(&value, 0, &mut || false),
            Err(SceneError::ResourceLimit("scene bytes"))
        ));
    }
}

/// Serialized size is an admission proxy, not an exact heap-size measurement.
/// The counting pass retains no serialized payload and precedes input clones.
fn admit_serialized_size(
    value: &impl Serialize,
    limit: usize,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<usize, SceneError> {
    struct Counter<F> {
        cancelled: F,
        interrupted: bool,
        count: usize,
        limit: usize,
        exceeded: bool,
    }
    impl<F: FnMut() -> bool> std::io::Write for Counter<F> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if (self.cancelled)() {
                self.interrupted = true;
                return Err(std::io::Error::other("tile admission cancelled"));
            }
            if bytes.len() > self.limit.saturating_sub(self.count) {
                self.exceeded = true;
                return Err(std::io::Error::other("tile admission limit"));
            }
            self.count += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter {
        cancelled,
        interrupted: false,
        count: 0,
        limit,
        exceeded: false,
    };
    serde_json::to_writer(&mut counter, value).map_err(|_| {
        if counter.interrupted {
            SceneError::Cancelled
        } else if counter.exceeded {
            SceneError::ResourceLimit("tile admission bytes")
        } else {
            SceneError::InvalidTile("tile serialization")
        }
    })?;
    Ok(counter.count)
}

#[cfg(test)]
mod admission_size_tests {
    use super::*;
    #[test]
    fn counting_admission_checks_encoded_bytes_without_retaining_payload() {
        let value = "escaped\n\"text";
        let encoded = serde_json::to_vec(value).unwrap();
        assert!(admit_serialized_size(&value, encoded.len(), &mut || false).is_ok());
        assert_eq!(
            admit_serialized_size(&value, encoded.len() - 1, &mut || false),
            Err(SceneError::ResourceLimit("tile admission bytes"))
        );
    }
}

#[cfg(test)]
mod admission_cancellation_tests {
    use super::*;

    #[test]
    fn admission_can_cancel_while_counting_before_projection() {
        let values = vec!["measurement"; 128];
        let mut polls = 0;
        assert_eq!(
            admit_serialized_size(&values, MAX_SCENE_BYTES, &mut || {
                polls += 1;
                polls == 8
            }),
            Err(SceneError::Cancelled)
        );
        assert_eq!(polls, 8);
    }

    #[test]
    fn structural_grid_validation_polls_before_large_scan() {
        let (floor_id, frame_id) = (
            FloorId::from_bytes([1; 16]).unwrap(),
            FrameId::from_bytes([2; 16]).unwrap(),
        );
        let grid = kyberia_spatial_analysis::Grid {
            floor_id,
            frame_id,
            origin: kyberia_spatial_analysis::Point2 {
                x: CoordinateMeters::new(0.0).unwrap(),
                y: CoordinateMeters::new(0.0).unwrap(),
            },
            resolution: Meters::new(1.0).unwrap(),
            column_offset: 0,
            row_offset: 0,
            width: 100_000,
            height: 1,
        };
        let mut polls = 0;
        assert_eq!(
            validate_spatial_grid(&grid, &mut || {
                polls += 1;
                polls == 1
            }),
            Err(SceneError::Cancelled)
        );
        assert_eq!(polls, 1);
    }

    #[test]
    fn resource_shape_rejects_work_and_memory_before_projection_allocations() {
        assert_eq!(
            admit_shape_resources(1, MAX_SAMPLES, 0, MAX_CELLS, 0, 0),
            Err(SceneError::ResourceLimit("scene replay work"))
        );
        assert_eq!(
            admit_shape_resources(MAX_SAMPLES, 1, 0, MAX_CELLS, MAX_TOTAL_CONTRIBUTIONS, 0,),
            Err(SceneError::ResourceLimit("scene working set"))
        );
        assert!(admit_shape_resources(1, 1, 1, 1, 1, 0).is_ok());
    }

    #[test]
    fn resource_accounting_includes_measured_layouts_and_nested_scratch() {
        let shape = SceneResourceShape::from_lengths(4, 3, 6, 2, 3, 17);
        let estimate = estimated_scene_working_bytes(shape).unwrap();
        let minimum_group_storage = checked_scaled(
            size_of::<kyberia_spatial_analysis::LocationGroup>()
                .max(size_of::<SceneLocationGroup>()),
            LIVE_GROUP_COLLECTIONS,
        )
        .unwrap();
        let minimum_nested_ids = checked_scaled(
            checked_scaled(6, size_of::<ObservationId>()).unwrap(),
            LIVE_GROUP_COLLECTIONS,
        )
        .unwrap();
        assert!(estimate >= MAX_SCENE_BYTES + 17 + minimum_group_storage);
        assert!(estimate >= MAX_SCENE_BYTES + 17 + minimum_nested_ids);
        assert!(size_of::<SceneLocationGroup>() >= 144);
        assert!(size_of::<kyberia_spatial_analysis::LocationGroup>() >= 144);
        assert!(size_of::<kyberia_wifi_semantics::StaticSignalSample>() >= 24);
    }

    #[test]
    fn oversized_existing_vec_capacity_is_rejected_before_projection() {
        let shape = SceneResourceShape {
            sample_count: 1,
            sample_capacity: MAX_SCENE_WORKING_BYTES,
            group_count: 1,
            group_capacity: 1,
            group_id_count: 1,
            group_id_capacity: 1,
            largest_group_id_count: 1,
            largest_group_id_capacity: 1,
            cell_count: 1,
            cell_capacity: 1,
            contribution_count: 1,
            contribution_capacity: 1,
            encoded_bytes: 0,
        };
        assert_eq!(
            admit_resource_shape(shape),
            Err(SceneError::ResourceLimit("scene working set"))
        );
    }
}

#[cfg(test)]
mod decode_cancellation_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn cancellation_interrupts_long_json_before_malformed_tail() {
        let mut bytes = vec![b' '; 8192];
        bytes.extend_from_slice(b"invalid");
        let mut polls = 0;
        assert_eq!(
            decode_scene(&bytes, &mut || {
                polls += 1;
                polls == 2
            }),
            Err(SceneError::Cancelled)
        );
        assert_eq!(polls, 2);
        assert_eq!(
            decode_scene(&bytes, &mut || false),
            Err(SceneError::MalformedBytes)
        );
    }

    #[test]
    fn preflight_rejects_oversized_nested_arrays_before_typed_decode() {
        let ids = std::iter::repeat_n("0", MAX_SAMPLES + 1)
            .collect::<Vec<_>>()
            .join(",");
        let bytes = format!("{{\"location_groups\":[{{\"observation_ids\":[{ids}]}}]}}");
        assert!(bytes.len() < MAX_SCENE_BYTES);
        let started = Instant::now();
        assert_eq!(
            SceneDocument::from_canonical_bytes(bytes.as_bytes()),
            Err(SceneError::ResourceLimit("group observation ids"))
        );
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(2));
        println!(
            "scene_measurement hostile_preflight_bytes={} hostile_preflight_us={}",
            bytes.len(),
            elapsed.as_micros()
        );
    }

    #[test]
    fn preflight_rejects_deep_unknown_values_before_typed_decode() {
        let nested = format!(
            "{}0{}",
            "[".repeat(MAX_SCENE_JSON_DEPTH + 1),
            "]".repeat(MAX_SCENE_JSON_DEPTH + 1)
        );
        let bytes = format!("{{\"unexpected\":{nested}}}");
        assert_eq!(
            SceneDocument::from_canonical_bytes(bytes.as_bytes()),
            Err(SceneError::ResourceLimit("scene json depth"))
        );
    }

    #[test]
    fn preflight_rejects_duplicate_known_fields_at_each_counted_level() {
        for bytes in [
            br#"{"samples":[],"samples":[]}"#.as_slice(),
            br#"{"location_groups":[{"observation_ids":[],"observation_ids":[]}] }"#
                .as_slice(),
            br#"{"location_groups":[{"signal_aggregate":{"observation_order":[],"observation_order":[]}}]}"#
                .as_slice(),
            br#"{"cells":[{"contributors":[],"contributors":[]}] }"#.as_slice(),
        ] {
            let mut cancelled = || false;
            assert_eq!(
                preflight_scene(bytes, &mut cancelled),
                Err(SceneError::MalformedBytes),
                "duplicate known field was admitted: {}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn lexical_preflight_bounds_escaped_strings_before_serde_materialization() {
        let escaped = "\\u0061".repeat(MAX_TEXT_BYTES);
        let bytes = format!("{{\"unexpected\":\"{escaped}\"}}");
        let started = Instant::now();
        assert_eq!(
            SceneDocument::from_canonical_bytes(bytes.as_bytes()),
            Err(SceneError::ResourceLimit("scene text"))
        );
        assert!(started.elapsed() < Duration::from_secs(2));

        let mut polls = 0;
        assert_eq!(
            preflight_json_strings(bytes.as_bytes(), &mut || {
                polls += 1;
                polls == 1
            }),
            Err(SceneError::Cancelled)
        );
        assert_eq!(polls, 1);
    }
}

#[cfg(test)]
mod projection_cancellation_tests {
    use super::*;

    #[test]
    fn bounded_projection_encoding_and_hashing_poll_cancellation() {
        let values = vec!["measurement"; 4096];
        let mut encoding_polls = 0;
        assert_eq!(
            bounded_scene_bytes_with_cancellation(&values, MAX_SCENE_BYTES, &mut || {
                encoding_polls += 1;
                encoding_polls == 2
            }),
            Err(SceneError::Cancelled)
        );
        assert_eq!(encoding_polls, 2);

        let bytes = vec![7_u8; 128 * 1024];
        let mut hash_polls = 0;
        assert_eq!(
            hash_bytes_with_cancellation(&bytes, &mut || {
                hash_polls += 1;
                hash_polls == 1
            }),
            Err(SceneError::Cancelled)
        );
        assert_eq!(hash_polls, 1);
    }
}
