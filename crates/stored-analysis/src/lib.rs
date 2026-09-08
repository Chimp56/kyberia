//! Transactionally bounded composition of stored survey evidence and spatial
//! RSSI analysis.
//!
//! This crate is an outward application layer. The storage adapter remains the
//! authority for decoding and verifying immutable bytes; the inward
//! `kyberia-observation-analysis` crate receives only canonical observations,
//! survey values, and a receipt-derived source binding. No storage or adapter
//! type crosses that inward boundary.

use kyberia_domain::{
    analysis::ExactU64,
    evidence::ArtifactReference,
    identity::{
        AdapterId, ContentHash, FloorId, FrameId, MacAddress, ObservationId, ProjectId, SessionId,
        SnapshotId, SourceId,
    },
};
use kyberia_observation_analysis::{
    SelectionError, SelectionRequest, SelectionSourceBinding, SurveyInput, ValidatedObservedRssiSet,
};
use kyberia_project_store::{Bundle, Cancellation, StoreError};
use kyberia_spatial_analysis::{Config as SpatialConfig, Grid, MetricDefinitionBinding, Tile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const STORED_RSSI_ANALYSIS_SCHEMA: &str = "kyberia.stored-rssi-analysis/1";
pub const STORED_RSSI_ANALYSIS_MEDIA_TYPE: &str = "application/kyberia-stored-rssi-analysis+json";
pub const MAX_SNAPSHOT_INPUTS: usize = 1_024;
/// Aggregate declared snapshot bytes retained by one analysis. The store
/// bounds each individual snapshot; this bound prevents a large number of
/// valid snapshots from expanding application memory without limit.
pub const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_SNAPSHOT_RECORDS: usize = 262_144;
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TILE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_OUTPUT_DEPTH: usize = 32;

/// An explicit floor binding for an immutable survey snapshot. A point survey
/// owns its coordinate frame but deliberately does not infer a building floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotInput {
    pub snapshot_id: SnapshotId,
    pub floor_id: FloorId,
}

/// A fully explicit stored-evidence analysis request. Observation IDs are the
/// bounded query key; source, project, floor, and revision scope are recorded
/// in the resulting selection manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredRssiAnalysisRequest {
    pub project_id: ProjectId,
    pub project_revision: u64,
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub target_bssid: MacAddress,
    pub observation_ids: Vec<ObservationId>,
    pub snapshots: Vec<SnapshotInput>,
    pub session_scope: Option<SessionId>,
    pub source_scope: Option<SourceId>,
    pub adapter_scope: Option<AdapterId>,
    pub allow_uncalibrated: bool,
    pub metric: MetricDefinitionBinding,
    pub spatial_configuration: SpatialConfig,
    pub grid: Grid,
}

#[derive(Debug)]
pub enum StoredAnalysisError {
    Cancelled,
    Store(StoreError),
    Selection(SelectionError),
    InvalidRequest(&'static str),
    InvalidSnapshot(String),
    InvalidOutput(&'static str),
    Serialization,
}

impl std::fmt::Display for StoredAnalysisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("stored RSSI analysis cancelled"),
            Self::Store(error) => write!(formatter, "stored RSSI analysis store error: {error}"),
            Self::Selection(error) => {
                write!(formatter, "stored RSSI analysis selection error: {error}")
            }
            Self::InvalidRequest(message) => {
                write!(formatter, "invalid stored RSSI request: {message}")
            }
            Self::InvalidSnapshot(message) => {
                write!(formatter, "invalid stored survey snapshot: {message}")
            }
            Self::InvalidOutput(message) => {
                write!(formatter, "invalid stored RSSI output: {message}")
            }
            Self::Serialization => {
                formatter.write_str("stored RSSI analysis serialization failure")
            }
        }
    }
}
impl std::error::Error for StoredAnalysisError {}
impl From<StoreError> for StoredAnalysisError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Cancelled => Self::Cancelled,
            other => Self::Store(other),
        }
    }
}
impl From<SelectionError> for StoredAnalysisError {
    fn from(error: SelectionError) -> Self {
        match error {
            SelectionError::Spatial(kyberia_spatial_analysis::Error::Cancelled) => Self::Cancelled,
            other => Self::Selection(other),
        }
    }
}

/// Canonical, versioned output provenance. The exact selection manifest and
/// tile JSON are retained as bytes so consumers can independently verify what
/// evidence and computation produced the returned tile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredRssiAnalysisDocument {
    pub schema: StoredRssiAnalysisSchema,
    pub project_id: ProjectId,
    pub project_revision: ExactU64,
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub selection_artifact: ArtifactReference,
    pub selection_manifest: Vec<u8>,
    pub source_chunk_hashes: Vec<ContentHash>,
    pub snapshots: Vec<SnapshotProvenance>,
    pub tile_bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredRssiAnalysisSchema {
    #[serde(rename = "kyberia.stored-rssi-analysis/1")]
    V1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotProvenance {
    pub snapshot_id: SnapshotId,
    pub artifact_hash: ContentHash,
    pub snapshot_revision: ExactU64,
}

impl StoredRssiAnalysisDocument {
    fn validate(&self) -> Result<(), StoredAnalysisError> {
        if self.schema != StoredRssiAnalysisSchema::V1 {
            return Err(StoredAnalysisError::InvalidOutput("schema version"));
        }
        if self.project_revision.get() == 0 {
            return Err(StoredAnalysisError::InvalidOutput("project revision"));
        }
        if self.selection_manifest.is_empty() || self.selection_manifest.len() > MAX_OUTPUT_BYTES {
            return Err(StoredAnalysisError::InvalidOutput(
                "selection manifest size",
            ));
        }
        if self.source_chunk_hashes.is_empty()
            || self.source_chunk_hashes.len() > kyberia_observation_analysis::MAX_SOURCE_CHUNKS
            || self
                .source_chunk_hashes
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(StoredAnalysisError::InvalidOutput("source chunk binding"));
        }
        if self.snapshots.is_empty() || self.snapshots.len() > MAX_SNAPSHOT_INPUTS {
            return Err(StoredAnalysisError::InvalidOutput(
                "snapshot provenance size",
            ));
        }
        if self
            .snapshots
            .windows(2)
            .any(|pair| pair[0].snapshot_id >= pair[1].snapshot_id)
        {
            return Err(StoredAnalysisError::InvalidOutput(
                "snapshot provenance order",
            ));
        }
        if self.snapshots.iter().any(|snapshot| {
            snapshot.snapshot_revision.get() == 0
                || snapshot.snapshot_revision.get() > self.project_revision.get()
        }) {
            return Err(StoredAnalysisError::InvalidOutput(
                "snapshot provenance revision",
            ));
        }
        if self.tile_bytes.is_empty() || self.tile_bytes.len() > MAX_TILE_BYTES {
            return Err(StoredAnalysisError::InvalidOutput("tile size"));
        }
        let selection = kyberia_observation_analysis::SelectionManifest::from_canonical_bytes(
            &self.selection_manifest,
        )?;
        if selection.project_id != self.project_id
            || selection.policy.project_revision.get() != self.project_revision.get()
            || selection.policy.floor_id != self.floor_id
            || selection.policy.frame_id != self.frame_id
            || selection.policy.source.selected_chunk_hashes != self.source_chunk_hashes
            || self.selection_artifact.byte_length != self.selection_manifest.len() as u64
            || self.selection_artifact.sha256
                != ContentHash::from_sha256(Sha256::digest(&self.selection_manifest).into())
            || self.selection_artifact.media_type.as_str()
                != kyberia_observation_analysis::SELECTION_MEDIA_TYPE
        {
            return Err(StoredAnalysisError::InvalidOutput("selection binding"));
        }
        // This is an output-envelope integrity check. Numerical cell
        // invariants remain the spatial engine's responsibility; accepting a
        // stored output does not silently reimplement that validator here.
        let tile: serde_json::Value = serde_json::from_slice(&self.tile_bytes)
            .map_err(|_| StoredAnalysisError::InvalidOutput("malformed tile bytes"))?;
        let tile_object = tile
            .as_object()
            .ok_or(StoredAnalysisError::InvalidOutput("tile object"))?;
        let expected_tile_keys = [
            "schema_version",
            "algorithm_version",
            "signal_aggregation",
            "inputs",
            "configuration",
            "location_groups",
            "grid",
            "cells",
        ];
        if tile_object.len() != expected_tile_keys.len()
            || expected_tile_keys
                .iter()
                .any(|key| !tile_object.contains_key(*key))
            || tile_object
                .get("schema_version")
                .and_then(serde_json::Value::as_str)
                != Some("kyberia.numeric-rssi-tile/2")
            || tile_object
                .get("algorithm_version")
                .and_then(serde_json::Value::as_str)
                != Some(kyberia_spatial_analysis::ALGORITHM_VERSION)
        {
            return Err(StoredAnalysisError::InvalidOutput("tile schema"));
        }
        let grid_object = tile_object
            .get("grid")
            .and_then(serde_json::Value::as_object)
            .ok_or(StoredAnalysisError::InvalidOutput("tile grid"))?;
        let width = grid_object
            .get("width")
            .and_then(serde_json::Value::as_u64)
            .ok_or(StoredAnalysisError::InvalidOutput("tile grid width"))?;
        let height = grid_object
            .get("height")
            .and_then(serde_json::Value::as_u64)
            .ok_or(StoredAnalysisError::InvalidOutput("tile grid height"))?;
        let cell_count = width
            .checked_mul(height)
            .ok_or(StoredAnalysisError::InvalidOutput("tile cell count"))?;
        if cell_count == 0 || cell_count > kyberia_spatial_analysis::MAX_CELLS as u64 {
            return Err(StoredAnalysisError::InvalidOutput("tile cell count"));
        }
        if tile_object
            .get("cells")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|cells| cells.len() as u64 != cell_count)
        {
            return Err(StoredAnalysisError::InvalidOutput("tile cells"));
        }
        let tile_floor = tile_object
            .get("grid")
            .and_then(|grid| grid.get("floor_id"));
        let tile_frame = tile_object
            .get("grid")
            .and_then(|grid| grid.get("frame_id"));
        if tile_floor
            != Some(
                &serde_json::to_value(self.floor_id)
                    .map_err(|_| StoredAnalysisError::Serialization)?,
            )
            || tile_frame
                != Some(
                    &serde_json::to_value(self.frame_id)
                        .map_err(|_| StoredAnalysisError::Serialization)?,
                )
        {
            return Err(StoredAnalysisError::InvalidOutput("tile geometry binding"));
        }
        let tile_source = tile_object
            .get("inputs")
            .and_then(|inputs| inputs.get("source_artifact"));
        if tile_source
            != Some(
                &serde_json::to_value(&self.selection_artifact)
                    .map_err(|_| StoredAnalysisError::Serialization)?,
            )
        {
            return Err(StoredAnalysisError::InvalidOutput("tile source binding"));
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StoredAnalysisError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| StoredAnalysisError::Serialization)?;
        if bytes.len() > MAX_OUTPUT_BYTES {
            return Err(StoredAnalysisError::InvalidOutput("output size"));
        }
        Ok(bytes)
    }

    /// Decode only the exact canonical V1 output document. The tile's
    /// numerical invariants remain delegated to the spatial engine, while the
    /// envelope, hashes, geometry binding, and resource limits are checked
    /// here before bytes are accepted as an output artifact.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, StoredAnalysisError> {
        if bytes.len() > MAX_OUTPUT_BYTES {
            return Err(StoredAnalysisError::InvalidOutput("output size"));
        }
        validate_json_depth(bytes)?;
        let document: Self = serde_json::from_slice(bytes)
            .map_err(|_| StoredAnalysisError::InvalidOutput("malformed output document"))?;
        document.validate()?;
        if serde_json::to_vec(&document).map_err(|_| StoredAnalysisError::Serialization)? != bytes {
            return Err(StoredAnalysisError::InvalidOutput("noncanonical output"));
        }
        Ok(document)
    }
}

/// The successful, all-or-error result. `tile` is the usable numerical value;
/// `document` and `canonical_bytes` are its immutable evidence/provenance
/// envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredRssiAnalysisResult {
    document: StoredRssiAnalysisDocument,
    canonical_bytes: Vec<u8>,
    artifact: ArtifactReference,
    tile: Tile,
}

impl StoredRssiAnalysisResult {
    pub fn document(&self) -> &StoredRssiAnalysisDocument {
        &self.document
    }
    pub fn tile(&self) -> &Tile {
        &self.tile
    }
    pub fn selection_manifest(&self) -> &[u8] {
        &self.document.selection_manifest
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
    pub fn artifact(&self) -> &ArtifactReference {
        &self.artifact
    }
}

/// Run one revision-consistent stored-evidence analysis. All reads and the
/// numerical computation are bounded and cancellation-aware. No result is
/// returned until source selection, tile generation, canonical encoding, and
/// the final project revision check have succeeded.
pub fn run(
    bundle: &Bundle,
    request: StoredRssiAnalysisRequest,
    cancel: &dyn Cancellation,
) -> Result<StoredRssiAnalysisResult, StoredAnalysisError> {
    check_cancel(cancel)?;
    validate_request(&request)?;
    let initial_manifest = bundle.manifest()?;
    check_cancel(cancel)?;
    if initial_manifest.project_id != request.project_id {
        return Err(StoredAnalysisError::InvalidRequest("project identity"));
    }
    if initial_manifest.revision != request.project_revision {
        return Err(StoredAnalysisError::InvalidRequest("project revision"));
    }

    let mut loaded = Vec::with_capacity(request.snapshots.len());
    let mut snapshot_provenance = Vec::with_capacity(request.snapshots.len());
    let mut snapshot_bytes = 0_u64;
    let mut snapshot_records = 0_usize;
    for input in &request.snapshots {
        check_cancel(cancel)?;
        let snapshot = bundle.load_survey_snapshot_with_cancel(input.snapshot_id, None, cancel)?;
        if snapshot.record.project_id != request.project_id
            || snapshot.record.revision == 0
            || snapshot.record.revision > request.project_revision
        {
            return Err(StoredAnalysisError::InvalidSnapshot(
                "snapshot project or revision binding".into(),
            ));
        }
        let artifact_hash = ContentHash::try_from(snapshot.record.artifact_hash.clone())
            .map_err(|_| StoredAnalysisError::InvalidSnapshot("snapshot hash".into()))?;
        let declared_bytes = initial_manifest
            .artifacts
            .get(&snapshot.record.artifact_hash)
            .map(|entry| entry.bytes)
            .ok_or_else(|| {
                StoredAnalysisError::InvalidSnapshot("snapshot artifact registration".into())
            })?;
        let progress = snapshot.survey.progress();
        let retained = progress
            .observation_ids
            .len()
            .checked_add(progress.associated_observation_ids.len())
            .ok_or(StoredAnalysisError::InvalidSnapshot(
                "snapshot record count overflow".into(),
            ))?;
        (snapshot_bytes, snapshot_records) =
            checked_snapshot_budget(snapshot_bytes, snapshot_records, declared_bytes, retained)?;
        snapshot_provenance.push(SnapshotProvenance {
            snapshot_id: input.snapshot_id,
            artifact_hash,
            snapshot_revision: ExactU64::new(snapshot.record.revision),
        });
        loaded.push((input.floor_id, snapshot.survey));
    }
    check_cancel(cancel)?;
    let queried =
        bundle.read_observation_selection_by_id_with_cancel(&request.observation_ids, cancel)?;
    check_cancel(cancel)?;
    let receipt = queried.receipt();
    if receipt.project_revision() != request.project_revision {
        return Err(StoredAnalysisError::InvalidRequest(
            "query project revision",
        ));
    }
    let mut chunk_hashes = Vec::with_capacity(receipt.selected_chunks().len());
    for descriptor in receipt.selected_chunks() {
        chunk_hashes.push(
            ContentHash::try_from(descriptor.hash().to_owned())
                .map_err(|_| StoredAnalysisError::InvalidRequest("query chunk hash"))?,
        );
    }
    let source =
        SelectionSourceBinding::from_verified_query(receipt.project_revision(), chunk_hashes)?;
    let observations = queried.observations();
    let selection_request = SelectionRequest {
        project_id: request.project_id,
        project_revision: request.project_revision,
        floor_id: request.floor_id,
        frame_id: request.frame_id,
        target_bssid: request.target_bssid,
        observation_ids: request.observation_ids.clone(),
        source,
        session_scope: request.session_scope,
        source_scope: request.source_scope,
        adapter_scope: request.adapter_scope,
        allow_uncalibrated: request.allow_uncalibrated,
    };
    let surveys = loaded
        .iter()
        .map(|(floor_id, survey)| SurveyInput {
            survey,
            floor_id: *floor_id,
        })
        .collect::<Vec<_>>();
    check_cancel(cancel)?;
    let selected = ValidatedObservedRssiSet::build(
        selection_request,
        request.metric,
        request.spatial_configuration,
        &surveys,
        observations,
    )?;
    check_cancel(cancel)?;
    if request.grid.floor_id != request.floor_id || request.grid.frame_id != request.frame_id {
        return Err(StoredAnalysisError::InvalidRequest("grid geometry binding"));
    }
    let tile = selected
        .tile(request.grid, || cancel.is_cancelled())
        .map_err(StoredAnalysisError::from)?;
    check_cancel(cancel)?;
    let tile_bytes = serde_json::to_vec(&tile).map_err(|_| StoredAnalysisError::Serialization)?;
    if tile_bytes.len() > MAX_TILE_BYTES {
        return Err(StoredAnalysisError::InvalidOutput("tile size"));
    }
    let document = StoredRssiAnalysisDocument {
        schema: StoredRssiAnalysisSchema::V1,
        project_id: request.project_id,
        project_revision: ExactU64::new(request.project_revision),
        floor_id: request.floor_id,
        frame_id: request.frame_id,
        selection_artifact: selected.artifact().clone(),
        selection_manifest: selected.canonical_manifest().to_vec(),
        source_chunk_hashes: selected
            .manifest()
            .policy
            .source
            .selected_chunk_hashes
            .clone(),
        snapshots: snapshot_provenance,
        tile_bytes,
    };
    check_cancel(cancel)?;
    let canonical_bytes = document.canonical_bytes()?;
    check_cancel(cancel)?;
    let artifact = ArtifactReference {
        sha256: ContentHash::from_sha256(Sha256::digest(&canonical_bytes).into()),
        media_type: kyberia_domain::identity::Text::new(STORED_RSSI_ANALYSIS_MEDIA_TYPE)
            .map_err(|_| StoredAnalysisError::InvalidOutput("output media type"))?,
        byte_length: canonical_bytes.len() as u64,
    };
    check_cancel(cancel)?;
    let final_manifest = bundle.manifest()?;
    check_cancel(cancel)?;
    if final_manifest.project_id != request.project_id
        || final_manifest.revision != request.project_revision
    {
        return Err(StoredAnalysisError::InvalidRequest(
            "project revision changed during analysis",
        ));
    }
    check_cancel(cancel)?;
    Ok(StoredRssiAnalysisResult {
        document,
        canonical_bytes,
        artifact,
        tile,
    })
}

fn validate_request(request: &StoredRssiAnalysisRequest) -> Result<(), StoredAnalysisError> {
    if request.project_revision == 0 {
        return Err(StoredAnalysisError::InvalidRequest("project revision"));
    }
    if request.observation_ids.is_empty()
        || request.observation_ids.len() > kyberia_observation_analysis::MAX_OBSERVATIONS
    {
        return Err(StoredAnalysisError::InvalidRequest("observation IDs"));
    }
    let ids = request
        .observation_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if ids.len() != request.observation_ids.len() {
        return Err(StoredAnalysisError::InvalidRequest(
            "duplicate observation ID",
        ));
    }
    if request.snapshots.is_empty() || request.snapshots.len() > MAX_SNAPSHOT_INPUTS {
        return Err(StoredAnalysisError::InvalidRequest("snapshot inputs"));
    }
    if request
        .snapshots
        .windows(2)
        .any(|pair| pair[0].snapshot_id >= pair[1].snapshot_id)
    {
        return Err(StoredAnalysisError::InvalidRequest(
            "snapshot inputs must be strictly ordered",
        ));
    }
    if request
        .snapshots
        .iter()
        .any(|snapshot| snapshot.floor_id != request.floor_id)
    {
        return Err(StoredAnalysisError::InvalidRequest(
            "snapshot floor binding",
        ));
    }
    if request.grid.floor_id != request.floor_id || request.grid.frame_id != request.frame_id {
        return Err(StoredAnalysisError::InvalidRequest("grid binding"));
    }
    request
        .spatial_configuration
        .validate()
        .map_err(|_| StoredAnalysisError::InvalidRequest("spatial configuration"))?;
    request
        .grid
        .validate()
        .map_err(|_| StoredAnalysisError::InvalidRequest("grid"))?;
    Ok(())
}

fn checked_snapshot_budget(
    current_bytes: u64,
    current_records: usize,
    declared_bytes: u64,
    retained_records: usize,
) -> Result<(u64, usize), StoredAnalysisError> {
    let bytes =
        current_bytes
            .checked_add(declared_bytes)
            .ok_or(StoredAnalysisError::InvalidSnapshot(
                "snapshot byte count overflow".into(),
            ))?;
    if bytes > MAX_SNAPSHOT_BYTES {
        return Err(StoredAnalysisError::InvalidSnapshot(
            "aggregate snapshot bytes exceed resource limit".into(),
        ));
    }
    let records = current_records.checked_add(retained_records).ok_or(
        StoredAnalysisError::InvalidSnapshot("snapshot record count overflow".into()),
    )?;
    if records > MAX_SNAPSHOT_RECORDS {
        return Err(StoredAnalysisError::InvalidSnapshot(
            "aggregate snapshot records exceed resource limit".into(),
        ));
    }
    Ok((bytes, records))
}

fn check_cancel(cancel: &dyn Cancellation) -> Result<(), StoredAnalysisError> {
    if cancel.is_cancelled() {
        Err(StoredAnalysisError::Cancelled)
    } else {
        Ok(())
    }
}

fn validate_json_depth(bytes: &[u8]) -> Result<(), StoredAnalysisError> {
    let mut depth = 0_usize;
    let mut string = false;
    let mut escaped = false;
    for byte in bytes {
        if string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                string = false;
            }
            continue;
        }
        match *byte {
            b'"' => string = true,
            b'{' | b'[' => {
                depth = depth
                    .checked_add(1)
                    .ok_or(StoredAnalysisError::InvalidOutput("output depth"))?;
                if depth > MAX_OUTPUT_DEPTH {
                    return Err(StoredAnalysisError::InvalidOutput("output depth"));
                }
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Err(StoredAnalysisError::InvalidOutput("output structure"));
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    if string || escaped || depth != 0 {
        return Err(StoredAnalysisError::InvalidOutput("output structure"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_snapshot_budget_accepts_boundary_and_rejects_overflow() {
        assert_eq!(
            checked_snapshot_budget(MAX_SNAPSHOT_BYTES - 1, 0, 1, 0).unwrap(),
            (MAX_SNAPSHOT_BYTES, 0)
        );
        assert!(matches!(
            checked_snapshot_budget(MAX_SNAPSHOT_BYTES, 0, 1, 0),
            Err(StoredAnalysisError::InvalidSnapshot(message))
                if message.contains("bytes")
        ));
        assert_eq!(
            checked_snapshot_budget(0, MAX_SNAPSHOT_RECORDS - 1, 0, 1).unwrap(),
            (0, MAX_SNAPSHOT_RECORDS)
        );
        assert!(matches!(
            checked_snapshot_budget(0, MAX_SNAPSHOT_RECORDS, 0, 1),
            Err(StoredAnalysisError::InvalidSnapshot(message))
                if message.contains("records")
        ));
    }
}
