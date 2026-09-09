//! Project geometry ownership and pure operation admission. All state changes
//! produce receipts; neither storage nor UI types enter this module.
mod commands;
mod entities;
use crate::{ValidationError, evidence::*, identity::*, spatial::FrameKind};
pub use commands::*;
pub use entities::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Project aggregate encoding versions are scoped to this contract.  The
/// shared evidence schema tag intentionally remains V1-only for unrelated
/// capture and capability envelopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectSchemaVersion {
    #[serde(rename = "1")]
    V1,
    #[serde(rename = "2")]
    V2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectError {
    InvalidValue(ValidationError),
    WrongProject,
    DuplicateEntity,
    MissingEntity,
    InvalidReference,
    DuplicateFrame,
    HasDependents,
    FrameMigrationRequired,
    DuplicateOperation,
    RevisionConflict { expected: u64, actual: u64 },
    StaleLogicalTime,
    InvalidReceipt,
    Limit,
}
impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ProjectError {}
impl From<ValidationError> for ProjectError {
    fn from(e: ValidationError) -> Self {
        Self::InvalidValue(e)
    }
}

const MAX_ENTITIES: usize = 10_000;
const MAX_OPERATIONS: usize = 100_000;

// Reject repeated wire map keys instead of allowing last-write-wins decoding.
fn unique_map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    struct Visitor<K, V>(std::marker::PhantomData<(K, V)>);
    impl<'de, K: Deserialize<'de> + Ord, V: Deserialize<'de>> serde::de::Visitor<'de>
        for Visitor<K, V>
    {
        type Value = BTreeMap<K, V>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bounded object with unique IDs")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((key, value)) = map.next_entry()? {
                if result.len() >= MAX_OPERATIONS || result.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate ID or inventory limit"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Visitor(std::marker::PhantomData))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ProjectData {
    schema_version: ProjectSchemaVersion,
    id: ProjectId,
    name: Text,
    revision: u64,
    logical_time: u64,
    #[serde(deserialize_with = "unique_map")]
    sites: BTreeMap<SiteId, Site>,
    #[serde(deserialize_with = "unique_map")]
    buildings: BTreeMap<BuildingId, Building>,
    #[serde(deserialize_with = "unique_map")]
    floors: BTreeMap<FloorId, Floor>,
    #[serde(deserialize_with = "unique_map")]
    maps: BTreeMap<MapAssetId, MapAsset>,
    #[serde(deserialize_with = "unique_map")]
    calibrations: BTreeMap<CalibrationId, MapCalibration>,
    #[serde(deserialize_with = "unique_map")]
    active_calibrations: BTreeMap<MapAssetId, Evidence<CalibrationId>>,
    #[serde(deserialize_with = "unique_map")]
    bound_evidence: BTreeMap<FloorId, ArtifactReference>,
    #[serde(deserialize_with = "unique_map")]
    applied_operations: BTreeMap<OperationId, u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ProjectData", into = "ProjectData")]
pub struct Project(ProjectData);
impl Project {
    pub fn new(id: ProjectId, name: Text) -> Self {
        Self(ProjectData {
            schema_version: ProjectSchemaVersion::V1,
            id,
            name,
            revision: 0,
            logical_time: 0,
            sites: BTreeMap::new(),
            buildings: BTreeMap::new(),
            floors: BTreeMap::new(),
            maps: BTreeMap::new(),
            calibrations: BTreeMap::new(),
            active_calibrations: BTreeMap::new(),
            bound_evidence: BTreeMap::new(),
            applied_operations: BTreeMap::new(),
        })
    }
    pub const fn id(&self) -> ProjectId {
        self.0.id
    }
    pub const fn schema_version(&self) -> ProjectSchemaVersion {
        self.0.schema_version
    }
    pub fn name(&self) -> &Text {
        &self.0.name
    }
    pub const fn revision(&self) -> u64 {
        self.0.revision
    }
    pub const fn logical_time(&self) -> u64 {
        self.0.logical_time
    }
    pub fn has_applied_operation(&self, id: OperationId) -> bool {
        self.0.applied_operations.contains_key(&id)
    }
    pub fn site(&self, id: SiteId) -> Option<&Site> {
        self.0.sites.get(&id)
    }
    pub fn building(&self, id: BuildingId) -> Option<&Building> {
        self.0.buildings.get(&id)
    }
    pub fn floor(&self, id: FloorId) -> Option<&Floor> {
        self.0.floors.get(&id)
    }
    pub fn map(&self, id: MapAssetId) -> Option<&MapAsset> {
        self.0.maps.get(&id)
    }
    pub fn calibration(&self, id: CalibrationId) -> Option<&MapCalibration> {
        self.0.calibrations.get(&id)
    }
    pub fn active_calibration(&self, id: MapAssetId) -> Option<Evidence<CalibrationId>> {
        self.0.active_calibrations.get(&id).cloned()
    }
    pub fn sites(&self) -> impl Iterator<Item = &Site> {
        self.0.sites.values()
    }
    pub fn floors(&self) -> impl Iterator<Item = &Floor> {
        self.0.floors.values()
    }

    /// Apply one already-admitted operation effect to a DAG materialized
    /// aggregate.  This is deliberately separate from [`Self::execute`]:
    /// linear receipts require a strictly increasing logical time, while
    /// independent DAG branches may carry the same Lamport value.  The
    /// materialized aggregate uses schema V2 and keeps its dense local
    /// revision independent from operation logical time.
    pub fn apply_materialized(
        &self,
        operation_id: OperationId,
        logical_time: u64,
        command: ProjectCommand,
    ) -> Result<Self, ProjectError> {
        if logical_time == 0 {
            return Err(ProjectError::StaleLogicalTime);
        }
        if self.0.applied_operations.contains_key(&operation_id) {
            return Err(ProjectError::DuplicateOperation);
        }
        let revision_after = self.0.revision.checked_add(1).ok_or(ProjectError::Limit)?;
        let mut next = self.clone();
        next.0.schema_version = ProjectSchemaVersion::V2;
        next.apply(&command)?;
        next.0.revision = revision_after;
        next.0.logical_time = next.0.logical_time.max(logical_time);
        next.0
            .applied_operations
            .insert(operation_id, revision_after);
        next.validate()?;
        Ok(next)
    }

    fn check_calibration(&self, calibration: &MapCalibration) -> Result<(), ProjectError> {
        let map = self
            .0
            .maps
            .get(&calibration.map_id)
            .ok_or(ProjectError::MissingEntity)?;
        let floor = self
            .0
            .floors
            .get(&map.data().floor_id)
            .ok_or(ProjectError::MissingEntity)?;
        let controls = calibration.transform.controls();
        if controls.source_frame != map.data().image_frame.id
            || controls.target_frame != floor.data().frame.id
        {
            return Err(ProjectError::InvalidReference);
        }
        for p in [controls.image_first, controls.image_second] {
            if p.x.get() < 0.0
                || p.y.get() < 0.0
                || p.x.get() > f64::from(map.data().width.get())
                || p.y.get() > f64::from(map.data().height.get())
            {
                return Err(ProjectError::InvalidReference);
            }
        }
        Ok(())
    }
    fn check_calibration_ref(
        &self,
        map_id: MapAssetId,
        id: &Evidence<CalibrationId>,
    ) -> Result<(), ProjectError> {
        match id {
            Evidence::Known(id) => {
                if self
                    .0
                    .calibrations
                    .get(id)
                    .ok_or(ProjectError::MissingEntity)?
                    .map_id
                    != map_id
                {
                    return Err(ProjectError::InvalidReference);
                }
            }
            Evidence::Unknown(UnknownReason::NotMeasured) => (),
            Evidence::Unknown(_) => return Err(ProjectError::InvalidReference),
        }
        Ok(())
    }
    fn validate(&self) -> Result<(), ProjectError> {
        if self.0.sites.len()
            + self.0.buildings.len()
            + self.0.floors.len()
            + self.0.maps.len()
            + self.0.calibrations.len()
            > MAX_ENTITIES
            || self.0.applied_operations.len() > MAX_OPERATIONS
        {
            return Err(ProjectError::Limit);
        }
        if self.0.revision != self.0.applied_operations.len() as u64 {
            return Err(ProjectError::InvalidReceipt);
        }
        if self.0.revision > 0 && self.0.logical_time == 0 {
            return Err(ProjectError::InvalidReceipt);
        }
        if self.0.schema_version == ProjectSchemaVersion::V1
            && self.0.logical_time < self.0.revision
        {
            return Err(ProjectError::InvalidReceipt);
        }
        let revisions: BTreeSet<_> = self.0.applied_operations.values().copied().collect();
        if revisions.len() != self.0.applied_operations.len()
            || revisions.iter().copied().ne(1..=self.0.revision)
        {
            return Err(ProjectError::InvalidReceipt);
        }
        let mut frames = BTreeSet::new();
        for (id, site) in &self.0.sites {
            if id != &site.id {
                return Err(ProjectError::InvalidReference);
            }
        }
        for (id, building) in &self.0.buildings {
            let d = building.data();
            if id != &d.id
                || !self.0.sites.contains_key(&d.site_id)
                || d.frame.kind != FrameKind::BuildingLocalMeters
            {
                return Err(ProjectError::InvalidReference);
            }
            if !frames.insert(d.frame.id) {
                return Err(ProjectError::DuplicateFrame);
            }
        }
        for (id, floor) in &self.0.floors {
            let d = floor.data();
            let building = self
                .0
                .buildings
                .get(&d.building_id)
                .ok_or(ProjectError::MissingEntity)?;
            if id != &d.id || d.building_frame != building.data().frame.id {
                return Err(ProjectError::InvalidReference);
            }
            if !frames.insert(d.frame.id) {
                return Err(ProjectError::DuplicateFrame);
            }
        }
        for (id, map) in &self.0.maps {
            let d = map.data();
            if id != &d.id || !self.0.floors.contains_key(&d.floor_id) {
                return Err(ProjectError::InvalidReference);
            }
            if !frames.insert(d.image_frame.id) {
                return Err(ProjectError::DuplicateFrame);
            }
            self.check_calibration_ref(
                *id,
                self.0
                    .active_calibrations
                    .get(id)
                    .ok_or(ProjectError::InvalidReference)?,
            )?;
        }
        if self.0.maps.len() != self.0.active_calibrations.len() {
            return Err(ProjectError::InvalidReference);
        }
        for (id, calibration) in &self.0.calibrations {
            if id != &calibration.id {
                return Err(ProjectError::InvalidReference);
            }
            self.check_calibration(calibration)?;
        }
        for (floor_id, evidence) in &self.0.bound_evidence {
            if !self.0.floors.contains_key(floor_id) || evidence.byte_length == 0 {
                return Err(ProjectError::InvalidReference);
            }
        }
        Ok(())
    }
}
impl TryFrom<ProjectData> for Project {
    type Error = ProjectError;
    fn try_from(data: ProjectData) -> Result<Self, Self::Error> {
        let project = Self(data);
        project.validate()?;
        Ok(project)
    }
}
impl From<Project> for ProjectData {
    fn from(p: Project) -> Self {
        p.0
    }
}
