use crate::{ValidationError, evidence::ArtifactReference, identity::*, spatial::*, units::*};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub id: SiteId,
    pub name: Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingData {
    pub id: BuildingId,
    pub site_id: SiteId,
    pub name: Text,
    pub frame: CoordinateFrame,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BuildingData", into = "BuildingData")]
pub struct Building(BuildingData);
impl Building {
    pub fn new(data: BuildingData) -> Result<Self, ValidationError> {
        if data.frame.kind != FrameKind::BuildingLocalMeters {
            return Err(ValidationError::Inconsistent(
                "building requires metric building frame",
            ));
        }
        Ok(Self(data))
    }
    pub const fn data(&self) -> &BuildingData {
        &self.0
    }
}
impl TryFrom<BuildingData> for Building {
    type Error = ValidationError;
    fn try_from(d: BuildingData) -> Result<Self, Self::Error> {
        Self::new(d)
    }
}
impl From<Building> for BuildingData {
    fn from(v: Building) -> Self {
        v.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloorData {
    pub id: FloorId,
    pub building_id: BuildingId,
    pub name: Text,
    pub frame: CoordinateFrame,
    pub building_frame: FrameId,
    /// Floor origin in building coordinates; z is signed floor elevation.
    pub origin: Point3,
    pub yaw: Radians,
    pub clear_height: Meters,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FloorData", into = "FloorData")]
pub struct Floor(FloorData);
impl Floor {
    pub fn new(data: FloorData) -> Result<Self, ValidationError> {
        if data.frame.kind != FrameKind::FloorLocalMeters || data.frame.id == data.building_frame {
            return Err(ValidationError::Inconsistent(
                "floor frame must be distinct metric frame",
            ));
        }
        if data.clear_height.get() == 0.0 {
            return Err(ValidationError::OutOfRange("floor clear height"));
        }
        Ok(Self(data))
    }
    pub const fn data(&self) -> &FloorData {
        &self.0
    }
    pub fn to_building(
        &self,
        source_frame: FrameId,
        point: Point3,
    ) -> Result<Point3, ValidationError> {
        if source_frame != self.0.frame.id {
            return Err(ValidationError::Inconsistent("floor frame mismatch"));
        }
        let (s, c) = self.0.yaw.get().sin_cos();
        Ok(Point3 {
            x: CoordinateMeters::new(
                self.0.origin.x.get() + point.x.get() * c - point.y.get() * s,
            )?,
            y: CoordinateMeters::new(
                self.0.origin.y.get() + point.x.get() * s + point.y.get() * c,
            )?,
            z: CoordinateMeters::new(self.0.origin.z.get() + point.z.get())?,
        })
    }
    pub fn from_building(
        &self,
        source_frame: FrameId,
        point: Point3,
    ) -> Result<Point3, ValidationError> {
        if source_frame != self.0.building_frame {
            return Err(ValidationError::Inconsistent("building frame mismatch"));
        }
        let (s, c) = self.0.yaw.get().sin_cos();
        let x = point.x.get() - self.0.origin.x.get();
        let y = point.y.get() - self.0.origin.y.get();
        Ok(Point3 {
            x: CoordinateMeters::new(x * c + y * s)?,
            y: CoordinateMeters::new(-x * s + y * c)?,
            z: CoordinateMeters::new(point.z.get() - self.0.origin.z.get())?,
        })
    }
}
impl TryFrom<FloorData> for Floor {
    type Error = ValidationError;
    fn try_from(d: FloorData) -> Result<Self, Self::Error> {
        Self::new(d)
    }
}
impl From<Floor> for FloorData {
    fn from(v: Floor) -> Self {
        v.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapAssetData {
    pub id: MapAssetId,
    pub floor_id: FloorId,
    pub name: Text,
    pub image_frame: CoordinateFrame,
    pub width: NonZeroU32,
    pub height: NonZeroU32,
    pub source: ArtifactReference,
    pub provenance: Text,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MapAssetData", into = "MapAssetData")]
pub struct MapAsset(MapAssetData);
impl MapAsset {
    pub fn new(data: MapAssetData) -> Result<Self, ValidationError> {
        if data.image_frame.kind != FrameKind::ImagePixels {
            return Err(ValidationError::Inconsistent("map requires pixel frame"));
        }
        if data.source.byte_length == 0 {
            return Err(ValidationError::OutOfRange("empty source map"));
        }
        Ok(Self(data))
    }
    pub const fn data(&self) -> &MapAssetData {
        &self.0
    }
}
impl TryFrom<MapAssetData> for MapAsset {
    type Error = ValidationError;
    fn try_from(d: MapAssetData) -> Result<Self, Self::Error> {
        Self::new(d)
    }
}
impl From<MapAsset> for MapAssetData {
    fn from(v: MapAsset) -> Self {
        v.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapCalibration {
    pub id: CalibrationId,
    pub map_id: MapAssetId,
    pub transform: TwoPointCalibration,
    pub provenance: Text,
    pub method_version: Text,
}
