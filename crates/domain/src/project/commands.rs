use super::*;
use crate::time::UtcTimestamp;
use std::num::NonZeroU64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProjectCommand {
    SetProjectName {
        name: Text,
    },
    SetSiteName {
        site_id: SiteId,
        name: Text,
    },
    CreateSite(Site),
    RemoveSite {
        site_id: SiteId,
    },
    CreateBuilding(Building),
    RemoveBuilding {
        building_id: BuildingId,
    },
    CreateFloor(Floor),
    RemoveFloor {
        floor_id: FloorId,
    },
    ImportMap(MapAsset),
    RemoveMap {
        map_id: MapAssetId,
    },
    CalibrateMap(MapCalibration),
    /// Switch an active revision without deleting historical calibrations.
    ActivateCalibration {
        map_id: MapAssetId,
        calibration: Evidence<CalibrationId>,
    },
    /// Once spatial evidence exists, scale/frame changes require a migration.
    BindFloorEvidence {
        floor_id: FloorId,
        evidence: ArtifactReference,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: OperationId,
    pub project_id: ProjectId,
    pub actor_id: ActorId,
    pub device_id: ActorDeviceId,
    pub logical_time: NonZeroU64,
    pub expected_revision: u64,
    pub wall_time: Evidence<UtcTimestamp>,
    pub command: ProjectCommand,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum EntityRef {
    Site(SiteId),
    Building(BuildingId),
    Floor(FloorId),
    Map(MapAssetId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectEvent {
    ProjectNameChanged {
        previous: Text,
        current: Text,
    },
    SiteNameChanged {
        site_id: SiteId,
        previous: Text,
        current: Text,
    },
    Created {
        entity: EntityRef,
    },
    Removed {
        entity: EntityRef,
    },
    CalibrationActivated {
        map_id: MapAssetId,
        previous: Evidence<CalibrationId>,
        current: Evidence<CalibrationId>,
    },
    FloorEvidenceBound {
        floor_id: FloorId,
        evidence: ContentHash,
    },
}

/// Serialized receipts are untrusted until Project::replay verifies the event,
/// inverse command, expected revision and deterministic result against state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub request: CommandRequest,
    pub revision_after: u64,
    pub event: ProjectEvent,
    pub undo: Evidence<ProjectCommand>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedOperation {
    pub project: Project,
    pub record: OperationRecord,
}

impl Project {
    pub fn execute(&self, request: CommandRequest) -> Result<AppliedOperation, ProjectError> {
        if request.project_id != self.id() {
            return Err(ProjectError::WrongProject);
        }
        if self
            .0
            .applied_operations
            .contains_key(&request.operation_id)
        {
            return Err(ProjectError::DuplicateOperation);
        }
        if request.expected_revision != self.revision() {
            return Err(ProjectError::RevisionConflict {
                expected: request.expected_revision,
                actual: self.revision(),
            });
        }
        if request.logical_time.get() <= self.logical_time() {
            return Err(ProjectError::StaleLogicalTime);
        }
        let revision_after = self.revision().checked_add(1).ok_or(ProjectError::Limit)?;
        let mut next = self.clone();
        let (event, undo) = next.apply(&request.command)?;
        next.0.revision = revision_after;
        next.0.logical_time = request.logical_time.get();
        next.0
            .applied_operations
            .insert(request.operation_id, revision_after);
        next.validate()?;
        Ok(AppliedOperation {
            project: next,
            record: OperationRecord {
                request,
                revision_after,
                event,
                undo,
            },
        })
    }

    pub fn replay(&self, record: &OperationRecord) -> Result<Self, ProjectError> {
        let applied = self.execute(record.request.clone())?;
        if &applied.record != record {
            return Err(ProjectError::InvalidReceipt);
        }
        Ok(applied.project)
    }

    fn unlocked(&self, floor_id: FloorId) -> Result<(), ProjectError> {
        if !self.0.floors.contains_key(&floor_id) {
            return Err(ProjectError::MissingEntity);
        }
        if self.0.bound_evidence.contains_key(&floor_id) {
            return Err(ProjectError::FrameMigrationRequired);
        }
        Ok(())
    }

    fn activate(
        &mut self,
        map_id: MapAssetId,
        current: Evidence<CalibrationId>,
    ) -> Result<(ProjectEvent, Evidence<ProjectCommand>), ProjectError> {
        let map = self
            .0
            .maps
            .get(&map_id)
            .ok_or(ProjectError::MissingEntity)?;
        self.unlocked(map.data().floor_id)?;
        self.check_calibration_ref(map_id, &current)?;
        let previous = self
            .0
            .active_calibrations
            .get(&map_id)
            .ok_or(ProjectError::MissingEntity)?
            .clone();
        self.0.active_calibrations.insert(map_id, current.clone());
        Ok((
            ProjectEvent::CalibrationActivated {
                map_id,
                previous: previous.clone(),
                current,
            },
            Evidence::Known(ProjectCommand::ActivateCalibration {
                map_id,
                calibration: previous,
            }),
        ))
    }

    pub(super) fn apply(
        &mut self,
        command: &ProjectCommand,
    ) -> Result<(ProjectEvent, Evidence<ProjectCommand>), ProjectError> {
        use ProjectCommand::*;
        match command {
            SetProjectName { name } => {
                let previous = std::mem::replace(&mut self.0.name, name.clone());
                Ok((
                    ProjectEvent::ProjectNameChanged {
                        previous: previous.clone(),
                        current: name.clone(),
                    },
                    Evidence::Known(SetProjectName { name: previous }),
                ))
            }
            SetSiteName { site_id, name } => {
                let site = self
                    .0
                    .sites
                    .get_mut(site_id)
                    .ok_or(ProjectError::MissingEntity)?;
                let previous = std::mem::replace(&mut site.name, name.clone());
                Ok((
                    ProjectEvent::SiteNameChanged {
                        site_id: *site_id,
                        previous: previous.clone(),
                        current: name.clone(),
                    },
                    Evidence::Known(SetSiteName {
                        site_id: *site_id,
                        name: previous,
                    }),
                ))
            }
            CreateSite(site) => {
                if self.0.sites.insert(site.id, site.clone()).is_some() {
                    return Err(ProjectError::DuplicateEntity);
                }
                Ok((
                    ProjectEvent::Created {
                        entity: EntityRef::Site(site.id),
                    },
                    Evidence::Known(RemoveSite { site_id: site.id }),
                ))
            }
            RemoveSite { site_id } => {
                if self
                    .0
                    .buildings
                    .values()
                    .any(|b| b.data().site_id == *site_id)
                {
                    return Err(ProjectError::HasDependents);
                }
                let old = self
                    .0
                    .sites
                    .remove(site_id)
                    .ok_or(ProjectError::MissingEntity)?;
                Ok((
                    ProjectEvent::Removed {
                        entity: EntityRef::Site(*site_id),
                    },
                    Evidence::Known(CreateSite(old)),
                ))
            }
            CreateBuilding(building) => {
                let id = building.data().id;
                if self.0.buildings.insert(id, building.clone()).is_some() {
                    return Err(ProjectError::DuplicateEntity);
                }
                Ok((
                    ProjectEvent::Created {
                        entity: EntityRef::Building(id),
                    },
                    Evidence::Known(RemoveBuilding { building_id: id }),
                ))
            }
            RemoveBuilding { building_id } => {
                if self
                    .0
                    .floors
                    .values()
                    .any(|f| f.data().building_id == *building_id)
                {
                    return Err(ProjectError::HasDependents);
                }
                let old = self
                    .0
                    .buildings
                    .remove(building_id)
                    .ok_or(ProjectError::MissingEntity)?;
                Ok((
                    ProjectEvent::Removed {
                        entity: EntityRef::Building(*building_id),
                    },
                    Evidence::Known(CreateBuilding(old)),
                ))
            }
            CreateFloor(floor) => {
                let id = floor.data().id;
                if self.0.floors.insert(id, floor.clone()).is_some() {
                    return Err(ProjectError::DuplicateEntity);
                }
                Ok((
                    ProjectEvent::Created {
                        entity: EntityRef::Floor(id),
                    },
                    Evidence::Known(RemoveFloor { floor_id: id }),
                ))
            }
            RemoveFloor { floor_id } => {
                self.unlocked(*floor_id)?;
                if self.0.maps.values().any(|m| m.data().floor_id == *floor_id) {
                    return Err(ProjectError::HasDependents);
                }
                let old = self
                    .0
                    .floors
                    .remove(floor_id)
                    .ok_or(ProjectError::MissingEntity)?;
                Ok((
                    ProjectEvent::Removed {
                        entity: EntityRef::Floor(*floor_id),
                    },
                    Evidence::Known(CreateFloor(old)),
                ))
            }
            ImportMap(map) => {
                let id = map.data().id;
                self.unlocked(map.data().floor_id)?;
                if self.0.maps.insert(id, map.clone()).is_some() {
                    return Err(ProjectError::DuplicateEntity);
                }
                self.0
                    .active_calibrations
                    .insert(id, Evidence::Unknown(UnknownReason::NotMeasured));
                Ok((
                    ProjectEvent::Created {
                        entity: EntityRef::Map(id),
                    },
                    Evidence::Known(RemoveMap { map_id: id }),
                ))
            }
            RemoveMap { map_id } => {
                let map = self.0.maps.get(map_id).ok_or(ProjectError::MissingEntity)?;
                self.unlocked(map.data().floor_id)?;
                if self.0.calibrations.values().any(|c| c.map_id == *map_id) {
                    return Err(ProjectError::HasDependents);
                }
                let old = self
                    .0
                    .maps
                    .remove(map_id)
                    .ok_or(ProjectError::MissingEntity)?;
                self.0.active_calibrations.remove(map_id);
                Ok((
                    ProjectEvent::Removed {
                        entity: EntityRef::Map(*map_id),
                    },
                    Evidence::Known(ImportMap(old)),
                ))
            }
            CalibrateMap(calibration) => {
                if self.0.calibrations.contains_key(&calibration.id) {
                    return Err(ProjectError::DuplicateEntity);
                }
                self.check_calibration(calibration)?;
                self.0
                    .calibrations
                    .insert(calibration.id, calibration.clone());
                self.activate(calibration.map_id, Evidence::Known(calibration.id))
            }
            ActivateCalibration {
                map_id,
                calibration,
            } => self.activate(*map_id, calibration.clone()),
            BindFloorEvidence { floor_id, evidence } => {
                self.unlocked(*floor_id)?;
                if evidence.byte_length == 0 {
                    return Err(ProjectError::InvalidReference);
                }
                self.0.bound_evidence.insert(*floor_id, evidence.clone());
                Ok((
                    ProjectEvent::FloorEvidenceBound {
                        floor_id: *floor_id,
                        evidence: evidence.sha256,
                    },
                    Evidence::Unknown(UnknownReason::NotApplicable),
                ))
            }
        }
    }
}
