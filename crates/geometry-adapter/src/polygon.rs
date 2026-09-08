use crate::{MAX_ABSOLUTE_COORDINATE_METERS, normalization_scale};
use geo::{Area, Intersects, LineString, Polygon, Validation};
use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
};

pub const MAX_POLYGON_COORDINATES: usize = 4096;
pub const MAX_POLYGON_HOLES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolygonError {
    ResourceLimit,
    UnclosedOrDegenerateRing,
    UnsupportedCoordinateResolution,
    CoordinateOutOfBounds,
    InvalidTopology,
    /// geo does not fully validate connected interiors with touching rings.
    TouchingRingsUnsupported,
}
impl std::fmt::Display for PolygonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "planar polygon rejected: {self:?}")
    }
}
impl std::error::Error for PolygonError {}

/// Validated floor-local polygon with immutable, explicitly closed input rings.
/// No repair, snapping, reprojection or implicit ring closure is performed.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedPolygon {
    floor_id: FloorId,
    frame_id: FrameId,
    exterior: Vec<Point2>,
    holes: Vec<Vec<Point2>>,
}
impl ValidatedPolygon {
    pub fn new(
        floor_id: FloorId,
        frame_id: FrameId,
        exterior: Vec<Point2>,
        holes: Vec<Vec<Point2>>,
    ) -> Result<Self, PolygonError> {
        if holes.len() > MAX_POLYGON_HOLES {
            return Err(PolygonError::ResourceLimit);
        }
        let count = holes
            .iter()
            .try_fold(exterior.len(), |n, r| n.checked_add(r.len()))
            .ok_or(PolygonError::ResourceLimit)?;
        if count > MAX_POLYGON_COORDINATES {
            return Err(PolygonError::ResourceLimit);
        }
        for ring in std::iter::once(&exterior).chain(holes.iter()) {
            if ring.len() < 4
                || ring.first() != ring.last()
                || ring.windows(2).any(|p| p[0] == p[1])
            {
                return Err(PolygonError::UnclosedOrDegenerateRing);
            }
        }
        let coordinates: Vec<_> = std::iter::once(&exterior)
            .chain(holes.iter())
            .flatten()
            .collect();
        if coordinates.iter().any(|p| {
            [p.x.get(), p.y.get()]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > MAX_ABSOLUTE_COORDINATE_METERS)
        }) {
            return Err(PolygonError::CoordinateOutOfBounds);
        }
        let scale = normalization_scale(
            coordinates
                .iter()
                .flat_map(|p| [p.x.get().abs(), p.y.get().abs()])
                .fold(0.0_f64, f64::max),
        );
        if scale == 0.0 {
            return Err(PolygonError::UnclosedOrDegenerateRing);
        }
        for axis in [0, 1] {
            let mut values: Vec<f64> = coordinates
                .iter()
                .map(|p| {
                    if axis == 0 {
                        p.x.get() / scale
                    } else {
                        p.y.get() / scale
                    }
                })
                .collect();
            values.sort_by(f64::total_cmp);
            if values
                .windows(2)
                .any(|p| p[0] != p[1] && (p[1] - p[0]).abs() < 1e-100)
            {
                return Err(PolygonError::UnsupportedCoordinateResolution);
            }
        }
        let normalized = |ring: &[Point2]| {
            LineString::from(
                ring.iter()
                    .map(|p| (p.x.get() / scale, p.y.get() / scale))
                    .collect::<Vec<_>>(),
            )
        };
        let shape = Polygon::new(
            normalized(&exterior),
            holes.iter().map(|r| normalized(r)).collect(),
        );
        shape
            .check_validation()
            .map_err(|_| PolygonError::InvalidTopology)?;
        let area = shape.unsigned_area();
        if !area.is_finite() || area <= 0.0 {
            return Err(PolygonError::InvalidTopology);
        }
        let rings: Vec<_> = std::iter::once(shape.exterior())
            .chain(shape.interiors().iter())
            .collect();
        for (index, ring) in rings.iter().enumerate() {
            for other in &rings[index + 1..] {
                if ring.intersects(*other) {
                    return Err(PolygonError::TouchingRingsUnsupported);
                }
            }
        }
        Ok(Self {
            floor_id,
            frame_id,
            exterior,
            holes,
        })
    }
    pub fn floor_id(&self) -> FloorId {
        self.floor_id
    }
    pub fn frame_id(&self) -> FrameId {
        self.frame_id
    }
    pub fn exterior(&self) -> &[Point2] {
        &self.exterior
    }
    pub fn holes(&self) -> &[Vec<Point2>] {
        &self.holes
    }
}
