use crate::{MAX_ABSOLUTE_COORDINATE_METERS, normalization_scale};
use geo::{
    Area, BooleanOps, Coord, Covers, Intersects, LineString, MultiPolygon, Polygon, Relate,
    Validation, coordinate_position::CoordPos, dimensions::Dimensions,
};
use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
};

pub const MAX_POLYGON_COORDINATES: usize = 4096;
pub const MAX_POLYGON_HOLES: usize = 128;
pub const MAX_MULTIPOLYGON_POLYGONS: usize = 256;
pub const MAX_MULTIPOLYGON_COORDINATES: usize = 16_384;
/// Conservative bound on the pairwise kernel work estimate. The estimate is
/// checked before a boolean operation starts; callers receive an explicit
/// resource error instead of allowing an unbounded overlay workload.
pub const MAX_BOOLEAN_WORK: usize = 4_194_304;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BooleanOperation {
    Intersection,
    Union,
    Difference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BooleanError {
    FloorMismatch,
    FrameMismatch,
    ResourceLimit,
    UnsupportedCoordinateResolution,
    InvalidKernelResult,
    TouchingTopologyUnsupported,
}
impl std::fmt::Display for BooleanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "planar boolean operation rejected: {self:?}")
    }
}
impl std::error::Error for BooleanError {}

/// Validated floor-local polygon with immutable, explicitly closed input rings.
/// No repair, snapping, reprojection or implicit ring closure is performed.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedPolygon {
    floor_id: FloorId,
    frame_id: FrameId,
    exterior: Vec<Point2>,
    holes: Vec<Vec<Point2>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedMultiPolygon {
    floor_id: FloorId,
    frame_id: FrameId,
    polygons: Vec<ValidatedPolygon>,
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

    pub fn as_multipolygon(&self) -> ValidatedMultiPolygon {
        ValidatedMultiPolygon {
            floor_id: self.floor_id,
            frame_id: self.frame_id,
            polygons: vec![self.clone()],
        }
    }

    pub fn boolean(
        &self,
        other: &Self,
        operation: BooleanOperation,
    ) -> Result<ValidatedMultiPolygon, BooleanError> {
        self.as_multipolygon()
            .boolean(&other.as_multipolygon(), operation)
    }

    pub fn union(&self, other: &Self) -> Result<ValidatedMultiPolygon, BooleanError> {
        self.boolean(other, BooleanOperation::Union)
    }

    pub fn intersection(&self, other: &Self) -> Result<ValidatedMultiPolygon, BooleanError> {
        self.boolean(other, BooleanOperation::Intersection)
    }

    pub fn difference(&self, other: &Self) -> Result<ValidatedMultiPolygon, BooleanError> {
        self.boolean(other, BooleanOperation::Difference)
    }
}

impl ValidatedMultiPolygon {
    pub fn new(
        floor_id: FloorId,
        frame_id: FrameId,
        polygons: Vec<ValidatedPolygon>,
    ) -> Result<Self, BooleanError> {
        if polygons.len() > MAX_MULTIPOLYGON_POLYGONS {
            return Err(BooleanError::ResourceLimit);
        }
        let mut total_coordinates = 0usize;
        for polygon in &polygons {
            let hole_coordinates = polygon
                .holes
                .iter()
                .try_fold(0usize, |count, hole| count.checked_add(hole.len()))
                .ok_or(BooleanError::ResourceLimit)?;
            let polygon_coordinates = polygon
                .exterior
                .len()
                .checked_add(hole_coordinates)
                .ok_or(BooleanError::ResourceLimit)?;
            total_coordinates = total_coordinates
                .checked_add(polygon_coordinates)
                .ok_or(BooleanError::ResourceLimit)?;
        }
        if total_coordinates > MAX_MULTIPOLYGON_COORDINATES {
            return Err(BooleanError::ResourceLimit);
        }

        let mut canonical = Vec::with_capacity(polygons.len());
        for polygon in polygons {
            if polygon.floor_id != floor_id {
                return Err(BooleanError::FloorMismatch);
            }
            if polygon.frame_id != frame_id {
                return Err(BooleanError::FrameMismatch);
            }
            let exterior = canonical_ring(&polygon.exterior);
            let mut holes: Vec<Vec<Point2>> = polygon
                .holes
                .iter()
                .map(|hole| canonical_ring(hole))
                .collect();
            holes.sort_by(|left, right| ring_cmp(left, right));
            let polygon = ValidatedPolygon::new(floor_id, frame_id, exterior, holes)
                .map_err(map_polygon_error)?;
            canonical.push(polygon);
        }
        canonical.sort_by(polygon_cmp);

        let scale = normalization_scale(
            canonical
                .iter()
                .flat_map(|polygon| {
                    polygon
                        .exterior
                        .iter()
                        .chain(polygon.holes.iter().flatten())
                        .flat_map(|point| [point.x.get().abs(), point.y.get().abs()])
                })
                .fold(0.0_f64, f64::max),
        );
        let shapes = canonical
            .iter()
            .map(|polygon| geo_polygon(polygon, scale))
            .collect::<Vec<_>>();
        for (index, shape) in shapes.iter().enumerate() {
            if shapes[index + 1..]
                .iter()
                .any(|other| shape.intersects(other))
            {
                return Err(BooleanError::TouchingTopologyUnsupported);
            }
        }

        Ok(Self {
            floor_id,
            frame_id,
            polygons: canonical,
        })
    }

    pub fn empty(floor_id: FloorId, frame_id: FrameId) -> Self {
        Self {
            floor_id,
            frame_id,
            polygons: Vec::new(),
        }
    }

    pub fn floor_id(&self) -> FloorId {
        self.floor_id
    }

    pub fn frame_id(&self) -> FrameId {
        self.frame_id
    }

    pub fn polygons(&self) -> &[ValidatedPolygon] {
        &self.polygons
    }

    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    pub fn boolean(&self, other: &Self, operation: BooleanOperation) -> Result<Self, BooleanError> {
        if self.floor_id != other.floor_id {
            return Err(BooleanError::FloorMismatch);
        }
        if self.frame_id != other.frame_id {
            return Err(BooleanError::FrameMismatch);
        }
        match operation {
            BooleanOperation::Union if self.is_empty() => return Ok(other.clone()),
            BooleanOperation::Union if other.is_empty() => return Ok(self.clone()),
            BooleanOperation::Intersection if self.is_empty() || other.is_empty() => {
                return Ok(Self::empty(self.floor_id, self.frame_id));
            }
            BooleanOperation::Difference if self.is_empty() => {
                return Ok(Self::empty(self.floor_id, self.frame_id));
            }
            BooleanOperation::Difference if other.is_empty() => return Ok(self.clone()),
            _ => {}
        }

        let left_coordinates = self.coordinate_count();
        let right_coordinates = other.coordinate_count();
        let work = left_coordinates
            .checked_mul(right_coordinates)
            .and_then(|work| work.checked_add(left_coordinates))
            .and_then(|work| work.checked_add(right_coordinates))
            .ok_or(BooleanError::ResourceLimit)?;
        if work > MAX_BOOLEAN_WORK {
            return Err(BooleanError::ResourceLimit);
        }

        let scale = normalization_scale(
            self.max_absolute_coordinate()
                .max(other.max_absolute_coordinate()),
        );
        let left = geo_multi_polygon(self, scale);
        let right = geo_multi_polygon(other, scale);
        if all_components_disjoint(&left, &right) {
            return match operation {
                BooleanOperation::Union => {
                    let mut polygons = self.polygons.clone();
                    polygons.extend(other.polygons.iter().cloned());
                    ValidatedMultiPolygon::new(self.floor_id, self.frame_id, polygons)
                }
                BooleanOperation::Intersection => Ok(Self::empty(self.floor_id, self.frame_id)),
                BooleanOperation::Difference => Ok(self.clone()),
            };
        }
        ensure_kernel_precision(self, other, scale)?;
        let result = match operation {
            BooleanOperation::Intersection => left.intersection(&right),
            BooleanOperation::Union => left.union(&right),
            BooleanOperation::Difference => left.difference(&right),
        };
        if operation == BooleanOperation::Union && result.0.is_empty() {
            return Err(BooleanError::InvalidKernelResult);
        }
        match operation {
            BooleanOperation::Union if !result.covers(&left) || !result.covers(&right) => {
                return Err(BooleanError::UnsupportedCoordinateResolution);
            }
            BooleanOperation::Intersection => {
                if result.0.is_empty() && has_interior_overlap(&left, &right) {
                    return Err(BooleanError::UnsupportedCoordinateResolution);
                }
                if result.0.is_empty() {
                    return Ok(Self::empty(self.floor_id, self.frame_id));
                }
                if !left.covers(&result) || !right.covers(&result) {
                    return Err(BooleanError::InvalidKernelResult);
                }
            }
            BooleanOperation::Difference => {
                if result.0.is_empty() {
                    if !left.relate(&right).is_coveredby() {
                        return Err(BooleanError::UnsupportedCoordinateResolution);
                    }
                    return Ok(Self::empty(self.floor_id, self.frame_id));
                }
                if !left.covers(&result) {
                    return Err(BooleanError::InvalidKernelResult);
                }
            }
            _ => {}
        }
        from_geo_multi_polygon(self.floor_id, self.frame_id, result, scale)
    }

    pub fn union(&self, other: &Self) -> Result<Self, BooleanError> {
        self.boolean(other, BooleanOperation::Union)
    }

    pub fn intersection(&self, other: &Self) -> Result<Self, BooleanError> {
        self.boolean(other, BooleanOperation::Intersection)
    }

    pub fn difference(&self, other: &Self) -> Result<Self, BooleanError> {
        self.boolean(other, BooleanOperation::Difference)
    }

    fn coordinate_count(&self) -> usize {
        self.polygons
            .iter()
            .map(|polygon| {
                polygon.exterior.len() + polygon.holes.iter().map(Vec::len).sum::<usize>()
            })
            .sum()
    }

    fn max_absolute_coordinate(&self) -> f64 {
        self.polygons
            .iter()
            .flat_map(|polygon| {
                polygon
                    .exterior
                    .iter()
                    .chain(polygon.holes.iter().flatten())
                    .flat_map(|point| [point.x.get().abs(), point.y.get().abs()])
            })
            .fold(0.0_f64, f64::max)
    }
}

fn map_polygon_error(error: PolygonError) -> BooleanError {
    match error {
        PolygonError::UnsupportedCoordinateResolution => {
            BooleanError::UnsupportedCoordinateResolution
        }
        PolygonError::TouchingRingsUnsupported => BooleanError::TouchingTopologyUnsupported,
        PolygonError::ResourceLimit => BooleanError::ResourceLimit,
        PolygonError::UnclosedOrDegenerateRing
        | PolygonError::CoordinateOutOfBounds
        | PolygonError::InvalidTopology => BooleanError::InvalidKernelResult,
    }
}

fn point_cmp(left: &Point2, right: &Point2) -> std::cmp::Ordering {
    left.x
        .get()
        .total_cmp(&right.x.get())
        .then_with(|| left.y.get().total_cmp(&right.y.get()))
}

fn ring_cmp(left: &[Point2], right: &[Point2]) -> std::cmp::Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| point_cmp(left, right))
        .find(|ordering| *ordering != std::cmp::Ordering::Equal)
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

fn polygon_cmp(left: &ValidatedPolygon, right: &ValidatedPolygon) -> std::cmp::Ordering {
    ring_cmp(&left.exterior, &right.exterior).then_with(|| {
        left.holes
            .iter()
            .zip(&right.holes)
            .map(|(left, right)| ring_cmp(left, right))
            .find(|ordering| *ordering != std::cmp::Ordering::Equal)
            .unwrap_or_else(|| left.holes.len().cmp(&right.holes.len()))
    })
}

fn canonical_ring(ring: &[Point2]) -> Vec<Point2> {
    let body = &ring[..ring.len() - 1];
    let mut best: Option<Vec<Point2>> = None;
    for reversed in [false, true] {
        for offset in 0..body.len() {
            let mut candidate = Vec::with_capacity(ring.len());
            for step in 0..body.len() {
                let index = if reversed {
                    (offset + body.len() - step) % body.len()
                } else {
                    (offset + step) % body.len()
                };
                candidate.push(body[index]);
            }
            candidate.push(candidate[0]);
            if best
                .as_ref()
                .is_none_or(|current| ring_cmp(&candidate, current).is_lt())
            {
                best = Some(candidate);
            }
        }
    }
    best.expect("validated ring has at least three distinct points")
}

fn geo_point(point: Point2, scale: f64) -> Coord<f64> {
    Coord {
        x: point.x.get() / scale,
        y: point.y.get() / scale,
    }
}

fn geo_polygon(polygon: &ValidatedPolygon, scale: f64) -> Polygon<f64> {
    let exterior = canonical_ring(&polygon.exterior)
        .into_iter()
        .map(|point| geo_point(point, scale))
        .collect::<Vec<_>>();
    let holes = polygon
        .holes
        .iter()
        .map(|hole| {
            LineString::from(
                canonical_ring(hole)
                    .into_iter()
                    .map(|point| geo_point(point, scale))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    Polygon::new(LineString::from(exterior), holes)
}

fn geo_multi_polygon(multi: &ValidatedMultiPolygon, scale: f64) -> MultiPolygon<f64> {
    MultiPolygon(
        multi
            .polygons
            .iter()
            .map(|polygon| geo_polygon(polygon, scale))
            .collect(),
    )
}

fn all_components_disjoint(left: &MultiPolygon<f64>, right: &MultiPolygon<f64>) -> bool {
    left.0.iter().map(polygon_bounds).all(|left| {
        right
            .0
            .iter()
            .map(polygon_bounds)
            .all(|right| bounds_are_strictly_separate(left, right))
    })
}

fn polygon_bounds(polygon: &Polygon<f64>) -> (f64, f64, f64, f64) {
    polygon.exterior().0.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, min_y, max_x, max_y), point| {
            (
                min_x.min(point.x),
                min_y.min(point.y),
                max_x.max(point.x),
                max_y.max(point.y),
            )
        },
    )
}

fn bounds_are_strictly_separate(left: (f64, f64, f64, f64), right: (f64, f64, f64, f64)) -> bool {
    left.2 < right.0 || right.2 < left.0 || left.3 < right.1 || right.3 < left.1
}

fn ensure_kernel_precision(
    left: &ValidatedMultiPolygon,
    right: &ValidatedMultiPolygon,
    scale: f64,
) -> Result<(), BooleanError> {
    let mut x_values = Vec::with_capacity(left.coordinate_count() + right.coordinate_count());
    let mut y_values = Vec::with_capacity(x_values.capacity());
    for multi in [left, right] {
        for polygon in &multi.polygons {
            for point in polygon
                .exterior
                .iter()
                .chain(polygon.holes.iter().flatten())
            {
                x_values.push(point.x.get() / scale);
                y_values.push(point.y.get() / scale);
            }
        }
    }
    let span = |values: &mut Vec<f64>| -> Result<f64, BooleanError> {
        values.sort_by(f64::total_cmp);
        let minimum = *values.first().ok_or(BooleanError::InvalidKernelResult)?;
        let maximum = *values.last().ok_or(BooleanError::InvalidKernelResult)?;
        Ok(maximum - minimum)
    };
    let maximum_half_span = span(&mut x_values)?.max(span(&mut y_values)?) / 2.0;
    if !maximum_half_span.is_finite() || maximum_half_span == 0.0 {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    // i_overlay converts the normalized float input to signed 32-bit integer
    // coordinates. Its automatic scale is approximately 2^29 / half_span.
    // Use a power-of-two-safe exponent and two integer steps of headroom so
    // narrow input features cannot silently alias in that conversion.
    let exponent = maximum_half_span.log2().ceil();
    if !exponent.is_finite() || exponent < f64::from(i32::MIN) || exponent > f64::from(i32::MAX) {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    let direction_scale = 2.0_f64.powi(29 - exponent as i32);
    let minimum_separation = 2.0 / direction_scale;
    if !minimum_separation.is_finite() || minimum_separation <= 0.0 {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    let has_unsupported_separation = |values: &[f64]| {
        values.windows(2).any(|pair| {
            let difference = pair[1] - pair[0];
            difference != 0.0 && difference.abs() < minimum_separation
        })
    };
    if has_unsupported_separation(&x_values) || has_unsupported_separation(&y_values) {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    let check_ring = |ring: &[Point2]| {
        let body = &ring[..ring.len() - 1];
        (0..body.len()).any(|index| {
            let first = body[index];
            let second = body[(index + 1) % body.len()];
            let third = body[(index + 2) % body.len()];
            let first_x = first.x.get() / scale;
            let first_y = first.y.get() / scale;
            let second_x = second.x.get() / scale;
            let second_y = second.y.get() / scale;
            let third_x = third.x.get() / scale;
            let third_y = third.y.get() / scale;
            let cross = (second_x - first_x) * (third_y - first_y)
                - (second_y - first_y) * (third_x - first_x);
            if cross == 0.0 {
                return false;
            }
            let edge = (second_x - first_x)
                .abs()
                .max((second_y - first_y).abs())
                .max((third_x - second_x).abs())
                .max((third_y - second_y).abs());
            cross.abs() < minimum_separation * edge
        })
    };
    if left.polygons.iter().chain(&right.polygons).any(|polygon| {
        check_ring(&polygon.exterior) || polygon.holes.iter().any(|hole| check_ring(hole))
    }) {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    Ok(())
}

fn has_interior_overlap(left: &MultiPolygon<f64>, right: &MultiPolygon<f64>) -> bool {
    left.0.iter().any(|left| {
        right.0.iter().any(|right| {
            left.relate(right).get(CoordPos::Inside, CoordPos::Inside) == Dimensions::TwoDimensional
        })
    })
}

fn from_geo_ring(ring: &LineString<f64>, scale: f64) -> Result<Vec<Point2>, BooleanError> {
    ring.0
        .iter()
        .map(|point| {
            let x = point.x * scale;
            let y = point.y * scale;
            if !x.is_finite()
                || !y.is_finite()
                || x.abs() > MAX_ABSOLUTE_COORDINATE_METERS
                || y.abs() > MAX_ABSOLUTE_COORDINATE_METERS
            {
                return Err(BooleanError::InvalidKernelResult);
            }
            Ok(Point2 {
                x: kyberia_domain::units::CoordinateMeters::new(x)
                    .map_err(|_| BooleanError::InvalidKernelResult)?,
                y: kyberia_domain::units::CoordinateMeters::new(y)
                    .map_err(|_| BooleanError::InvalidKernelResult)?,
            })
        })
        .collect()
}

fn from_geo_multi_polygon(
    floor_id: FloorId,
    frame_id: FrameId,
    result: MultiPolygon<f64>,
    scale: f64,
) -> Result<ValidatedMultiPolygon, BooleanError> {
    if result.0.len() > MAX_MULTIPOLYGON_POLYGONS {
        return Err(BooleanError::ResourceLimit);
    }
    let mut total_coordinates = 0usize;
    let mut polygons = Vec::with_capacity(result.0.len());
    for polygon in result.0 {
        let count = polygon.exterior().0.len().checked_add(
            polygon
                .interiors()
                .iter()
                .try_fold(0usize, |count, ring| count.checked_add(ring.0.len()))
                .ok_or(BooleanError::ResourceLimit)?,
        );
        total_coordinates = total_coordinates
            .checked_add(count.ok_or(BooleanError::ResourceLimit)?)
            .ok_or(BooleanError::ResourceLimit)?;
        if total_coordinates > MAX_MULTIPOLYGON_COORDINATES {
            return Err(BooleanError::ResourceLimit);
        }
        let exterior = from_geo_ring(polygon.exterior(), scale)?;
        let holes = polygon
            .interiors()
            .iter()
            .map(|ring| from_geo_ring(ring, scale))
            .collect::<Result<Vec<_>, _>>()?;
        polygons.push(
            ValidatedPolygon::new(floor_id, frame_id, exterior, holes)
                .map_err(map_polygon_error)?,
        );
    }
    if polygons.is_empty() {
        return Ok(ValidatedMultiPolygon::empty(floor_id, frame_id));
    }
    ValidatedMultiPolygon::new(floor_id, frame_id, polygons)
}
