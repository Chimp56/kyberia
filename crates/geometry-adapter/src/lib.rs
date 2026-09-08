//! Portable planar geometry adapter. Library types never cross this boundary.
//! Coordinates are meters in one explicitly identified floor-local frame.
mod polygon;
use geo::{
    Coord, Line,
    line_intersection::{LineIntersection, line_intersection},
};
use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
    units::CoordinateMeters,
};
pub use polygon::{MAX_POLYGON_COORDINATES, MAX_POLYGON_HOLES, PolygonError, ValidatedPolygon};

/// Numerical input bound, not a geographic projection or a snapping tolerance.
pub const MAX_ABSOLUTE_COORDINATE_METERS: f64 = 1_000_000_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub start: Point2,
    pub end: Point2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Intersection {
    Disjoint,
    Point(Point2),
    /// Endpoints are lexicographically ordered; no traversal direction is implied.
    Overlap {
        start: Point2,
        end: Point2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    FloorMismatch,
    FrameMismatch,
    DegenerateSegment,
    CoordinateOutOfBounds,
    InvalidKernelResult,
    UnsupportedCoordinateResolution,
}

impl std::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::FloorMismatch => "planar segments belong to different floors",
            Self::FrameMismatch => "planar segments belong to different coordinate frames",
            Self::DegenerateSegment => "planar segment has identical endpoints",
            Self::CoordinateOutOfBounds => {
                "planar coordinate exceeds the supported numerical range"
            }
            Self::InvalidKernelResult => "geometry kernel returned an invalid coordinate",
            Self::UnsupportedCoordinateResolution => {
                "geometry exceeds the supported coordinate resolution"
            }
        })
    }
}
impl std::error::Error for GeometryError {}

fn coordinate(point: Point2) -> Coord<f64> {
    Coord {
        x: point.x.get(),
        y: point.y.get(),
    }
}

fn line(segment: Segment) -> Result<Line<f64>, GeometryError> {
    let start = coordinate(segment.start);
    let end = coordinate(segment.end);
    if [start.x, start.y, end.x, end.y]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > MAX_ABSOLUTE_COORDINATE_METERS)
    {
        return Err(GeometryError::CoordinateOutOfBounds);
    }
    if start == end {
        return Err(GeometryError::DegenerateSegment);
    }
    // Canonicalize input direction so swapping endpoints cannot select a
    // different floating-point evaluation path inside the kernel.
    Ok(if (start.x, start.y) <= (end.x, end.y) {
        Line::new(start, end)
    } else {
        Line::new(end, start)
    })
}

fn point(c: Coord<f64>) -> Result<Point2, GeometryError> {
    Ok(Point2 {
        x: CoordinateMeters::new(if c.x == 0.0 { 0.0 } else { c.x })
            .map_err(|_| GeometryError::InvalidKernelResult)?,
        y: CoordinateMeters::new(if c.y == 0.0 { 0.0 } else { c.y })
            .map_err(|_| GeometryError::InvalidKernelResult)?,
    })
}

/// A topological XY intersection, not a material-penetration or attenuation
/// result. Cross-floor/frame inputs fail rather than being projected together.
pub fn intersect(a: Segment, b: Segment) -> Result<Intersection, GeometryError> {
    if a.floor_id != b.floor_id {
        return Err(GeometryError::FloorMismatch);
    }
    if a.frame_id != b.frame_id {
        return Err(GeometryError::FrameMismatch);
    }
    let mut a = line(a)?;
    let mut b = line(b)?;
    // Scale small local coordinates up before orientation products. Dividing
    // directly avoids an overflowing reciprocal for subnormal coordinates.
    let scale = [
        a.start.x, a.start.y, a.end.x, a.end.y, b.start.x, b.start.y, b.end.x, b.end.y,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(0.0_f64, f64::max)
    .min(1.0);
    let original_endpoints = [a.start, a.end, b.start, b.end];
    for line in [&mut a, &mut b] {
        for coordinate in [&mut line.start, &mut line.end] {
            coordinate.x /= scale;
            coordinate.y /= scale;
        }
    }
    let coordinates = [a.start, a.end, b.start, b.end];
    // Do not feed extreme mixed-scale input to the floating-point kernel.
    // This is an explicit numerical admission boundary, not snapping.
    for first in coordinates {
        for second in coordinates {
            for difference in [first.x - second.x, first.y - second.y] {
                if difference != 0.0 && difference.abs() < 1e-100 {
                    return Err(GeometryError::UnsupportedCoordinateResolution);
                }
            }
        }
    }
    let restore = |coordinate: Coord<f64>| {
        point(Coord {
            x: coordinate.x * scale,
            y: coordinate.y * scale,
        })
    };
    if (a.start.x, a.start.y, a.end.x, a.end.y) > (b.start.x, b.start.y, b.end.x, b.end.y) {
        std::mem::swap(&mut a, &mut b);
    }
    Ok(match line_intersection(a, b) {
        None => Intersection::Disjoint,
        Some(LineIntersection::SinglePoint {
            intersection,
            is_proper,
        }) => {
            let result = restore(intersection)?;
            if is_proper && original_endpoints.contains(&coordinate(result)) {
                return Err(GeometryError::UnsupportedCoordinateResolution);
            }
            Intersection::Point(result)
        }
        Some(LineIntersection::Collinear { intersection }) => {
            let (start, end) = if (intersection.start.x, intersection.start.y)
                <= (intersection.end.x, intersection.end.y)
            {
                (intersection.start, intersection.end)
            } else {
                (intersection.end, intersection.start)
            };
            Intersection::Overlap {
                start: restore(start)?,
                end: restore(end)?,
            }
        }
    })
}
