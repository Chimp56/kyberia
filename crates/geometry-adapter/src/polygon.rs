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
/// checked before a boolean operation starts and while sequential component
/// results grow; callers receive an explicit resource error instead of
/// allowing an unbounded overlay workload.
pub const MAX_BOOLEAN_WORK: usize = 4_194_304;
/// Maximum tolerated area discrepancy in normalized coordinate-area units. A fixed
/// absolute bound prevents a large valid residual from masking a missing
/// small residual through relative-error scaling.
const BOOLEAN_AREA_ERROR_TOLERANCE: f64 = 1e-7;
// i_overlay's independent component buffer and its unioned multi-polygon
// buffer can differ by a few last-place operations. This envelope is applied
// only after exact coverage fails and is deliberately far below the supported
// coordinate-resolution checks; larger missing regions remain unsupported.
const OFFSET_COVERAGE_ROUNDING_TOLERANCE: f64 = 1e-7;

pub(crate) fn offset(
    input: &ValidatedMultiPolygon,
    options: crate::OffsetOptions,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<ValidatedMultiPolygon, crate::OffsetError> {
    use crate::OffsetError;
    use geo::{
        Buffer,
        algorithm::buffer::{BufferStyle, LineJoin},
    };
    if cancelled() {
        return Err(OffsetError::Cancelled);
    }
    let distance = options.signed_distance();
    let count = input.polygons.iter().try_fold(0usize, |count, polygon| {
        if cancelled() {
            return Err(OffsetError::Cancelled);
        }
        polygon_rings(polygon).try_fold(count, |count, ring| {
            count
                .checked_add(ring.len())
                .ok_or(OffsetError::ResourceLimit)
        })
    })?;
    // geo/i_overlay clamps round joins to [0.01*pi, 0.25*pi]. Account for
    // that effective angle so the work estimate never undercounts a request
    // such as pi/2, which the kernel narrows to pi/4.
    let effective_arc_angle = options
        .max_arc_angle()
        .get()
        .clamp(0.01 * std::f64::consts::PI, 0.25 * std::f64::consts::PI);
    let arc_vertices = (std::f64::consts::TAU / effective_arc_angle).ceil() as usize + 2;
    let expanded = count
        .checked_mul(arc_vertices)
        .ok_or(OffsetError::ResourceLimit)?;
    if expanded
        .checked_mul(expanded)
        .is_none_or(|work| work > crate::MAX_OFFSET_WORK)
    {
        return Err(OffsetError::ResourceLimit);
    }
    if distance == 0.0 || input.is_empty() {
        if cancelled() {
            return Err(OffsetError::Cancelled);
        }
        return Ok(input.clone());
    }
    let maximum = input
        .polygons
        .iter()
        .flat_map(polygon_rings)
        .flatten()
        .flat_map(|p| [p.x.get().abs(), p.y.get().abs()])
        .fold(distance.abs(), f64::max);
    let scale = normalization_scale(maximum);
    // The kernel subtracts the input bounding-box midpoint before its
    // float-to-integer conversion. Use that local extent for the resolution
    // test; using absolute world coordinates would reject a valid small offset
    // on a translated floor. Two integer-grid steps plus a second safety
    // factor leave room for the rounded join's 1.1 radius reserve.
    let local_span = input.polygons.iter().flat_map(polygon_rings).try_fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |bounds, ring| {
            ring.iter()
                .try_fold(bounds, |(min_x, max_x, min_y, max_y), point| {
                    let x = point.x.get() / scale;
                    let y = point.y.get() / scale;
                    if !x.is_finite() || !y.is_finite() {
                        return Err(OffsetError::UnsupportedCoordinateResolution);
                    }
                    Ok((min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y)))
                })
        },
    )?;
    let span = (local_span.1 - local_span.0).max(local_span.3 - local_span.2);
    let expanded_half_span = (span + 2.2 * distance.abs() / scale) / 2.0;
    if !expanded_half_span.is_finite() || expanded_half_span <= 0.0 {
        return Err(OffsetError::UnsupportedCoordinateResolution);
    }
    let exponent = expanded_half_span.log2().floor();
    if !exponent.is_finite()
        || exponent < f64::from(i32::MIN + 29)
        || exponent > f64::from(i32::MAX - 29)
    {
        return Err(OffsetError::UnsupportedCoordinateResolution);
    }
    let kernel_grid = 2.0_f64.powi(exponent as i32 - 29);
    if !kernel_grid.is_finite() || distance.abs() / scale < 4.0 * kernel_grid {
        return Err(OffsetError::UnsupportedCoordinateResolution);
    }
    let shape = geo_multi_polygon(input, scale);
    if cancelled() {
        return Err(OffsetError::Cancelled);
    }
    let style = BufferStyle::new(distance / scale)
        .line_join(LineJoin::Round(options.max_arc_angle().get()));
    let result = shape.buffer_with_style(style.clone());
    if cancelled() {
        return Err(OffsetError::Cancelled);
    }
    bounded_output_coordinates(&result).map_err(OffsetError::KernelResult)?;
    if matches!(options.direction(), crate::OffsetDirection::Outward(_)) {
        if result.0.is_empty() || !multipolygon_covers(&result, &shape) {
            return Err(OffsetError::UnsupportedCoordinateResolution);
        }
    } else {
        if result.0.is_empty() && !inward_empty_is_provable(input, distance) {
            return Err(OffsetError::UnsupportedCoordinateResolution);
        }
        if !result.0.is_empty() && !multipolygon_covers(&shape, &result) {
            return Err(OffsetError::KernelResult(BooleanError::InvalidKernelResult));
        }
    }
    // A global multi-polygon buffer can alias a small component against a
    // distant large one. Buffer each component in the same normalized space
    // and require the complete expected component to be covered by the global
    // result. An intersection check would accept a result that retained only a
    // small fragment of a component. This is a bounded completeness check
    // under MAX_OFFSET_WORK.
    for polygon in &input.polygons {
        if cancelled() {
            return Err(OffsetError::Cancelled);
        }
        let expected = geo_polygon(polygon, scale).buffer_with_style(style.clone());
        bounded_output_coordinates(&expected).map_err(OffsetError::KernelResult)?;
        if matches!(options.direction(), crate::OffsetDirection::Outward(_))
            && expected.0.is_empty()
        {
            return Err(OffsetError::UnsupportedCoordinateResolution);
        }
        if !multipolygon_covers(&result, &expected)
            && !multipolygon_covers_with_tolerance_floor(
                &result,
                &expected,
                OFFSET_COVERAGE_ROUNDING_TOLERANCE,
            )
        {
            return Err(OffsetError::UnsupportedCoordinateResolution);
        }
    }
    let result = from_geo_multi_polygon(input.floor_id, input.frame_id, result, scale)
        .map_err(OffsetError::KernelResult)?;
    if cancelled() {
        return Err(OffsetError::Cancelled);
    }
    Ok(result)
}

/// An empty inward result is accepted only when the distance is at least the
/// half-width of every input bounding box. That is a conservative certificate
/// that no disk of the requested radius can fit inside any component. Smaller
/// empty results are rejected because a float-to-integer buffer could have
/// erased a narrow surviving feature.
fn inward_empty_is_provable(input: &ValidatedMultiPolygon, distance: f64) -> bool {
    input.polygons.iter().all(|polygon| {
        let bounds = polygon.exterior.iter().fold(
            (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ),
            |(min_x, max_x, min_y, max_y), point| {
                (
                    min_x.min(point.x.get()),
                    max_x.max(point.x.get()),
                    min_y.min(point.y.get()),
                    max_y.max(point.y.get()),
                )
            },
        );
        let width = bounds.1 - bounds.0;
        let height = bounds.3 - bounds.2;
        width.is_finite() && height.is_finite() && distance.abs() >= width.min(height) / 2.0
    })
}

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
    UnsupportedTopology,
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
        if matches!(
            operation,
            BooleanOperation::Intersection | BooleanOperation::Difference
        ) {
            ensure_supported_overlay_topology(self, other, scale)?;
        }
        let result = match operation {
            BooleanOperation::Intersection => componentwise_intersection(&left, &right)?,
            BooleanOperation::Union => left.union(&right),
            BooleanOperation::Difference => componentwise_difference(&left, &right)?,
        };
        if operation == BooleanOperation::Union && result.0.is_empty() {
            return Err(BooleanError::InvalidKernelResult);
        }
        match operation {
            BooleanOperation::Union
                if !multipolygon_covers(&result, &left)
                    || !multipolygon_covers(&result, &right) =>
            {
                return Err(BooleanError::UnsupportedCoordinateResolution);
            }
            BooleanOperation::Intersection => {
                if result.0.is_empty() && has_interior_overlap(&left, &right) {
                    return Err(BooleanError::UnsupportedCoordinateResolution);
                }
                if result.0.is_empty() {
                    return Ok(Self::empty(self.floor_id, self.frame_id));
                }
                if !multipolygon_covers_with_tolerance(&left, &result)
                    || !multipolygon_covers_with_tolerance(&right, &result)
                {
                    return Err(BooleanError::InvalidKernelResult);
                }
                ensure_intersection_completeness(&left, &right, &result)?;
            }
            BooleanOperation::Difference => {
                if result.0.is_empty() {
                    if !left.relate(&right).is_coveredby() {
                        return Err(BooleanError::UnsupportedCoordinateResolution);
                    }
                    return Ok(Self::empty(self.floor_id, self.frame_id));
                }
                if !multipolygon_covers_with_tolerance(&left, &result) {
                    return Err(BooleanError::InvalidKernelResult);
                }
                ensure_difference_completeness(&left, &right, &result)?;
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

fn componentwise_intersection(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
) -> Result<MultiPolygon<f64>, BooleanError> {
    let mut result = Vec::new();
    let mut total_coordinates = 0usize;
    for left in &left.0 {
        for right in &right.0 {
            let kernel = left.intersection(right);
            bounded_output_coordinates(&kernel)?;
            let expected = convex_intersection_area(left, right)?;
            // The pinned overlay kernel is used as an independent topological
            // sanity check. Its floating output is not trusted for coordinates
            // or area: the bounded convex clipper below supplies the result.
            if kernel.0.len() > 1 || (expected > 0.0) == kernel.0.is_empty() {
                return Err(BooleanError::UnsupportedCoordinateResolution);
            }
            if let Some(polygon) = convex_intersection_polygon(left, right)? {
                append_bounded_polygon(&mut result, &mut total_coordinates, polygon)?;
            }
        }
    }
    Ok(MultiPolygon(result))
}

fn componentwise_difference(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
) -> Result<MultiPolygon<f64>, BooleanError> {
    let mut result = Vec::new();
    let mut total_coordinates = 0usize;
    let mut work_used = 0usize;
    for left in &left.0 {
        let mut remaining = vec![left.clone()];
        for right in &right.0 {
            let mut next = Vec::new();
            let mut next_coordinates = 0usize;
            for remaining in remaining {
                let pair_work = polygon_coordinate_count(&remaining)
                    .checked_mul(polygon_coordinate_count(right))
                    .ok_or(BooleanError::ResourceLimit)?;
                work_used = work_used
                    .checked_add(pair_work)
                    .ok_or(BooleanError::ResourceLimit)?;
                if work_used > MAX_BOOLEAN_WORK {
                    return Err(BooleanError::ResourceLimit);
                }
                let difference = remaining.difference(right);
                bounded_output_coordinates(&difference)?;
                for polygon in difference.0 {
                    append_bounded_polygon(&mut next, &mut next_coordinates, polygon)?;
                }
            }
            remaining = next;
            if remaining.is_empty() {
                break;
            }
        }
        for polygon in remaining {
            append_bounded_polygon(&mut result, &mut total_coordinates, polygon)?;
        }
    }
    Ok(MultiPolygon(result))
}

fn polygon_coordinate_count(polygon: &Polygon<f64>) -> usize {
    polygon.exterior().0.len()
        + polygon
            .interiors()
            .iter()
            .map(|ring| ring.0.len())
            .sum::<usize>()
}

fn append_bounded_polygon(
    polygons: &mut Vec<Polygon<f64>>,
    total_coordinates: &mut usize,
    polygon: Polygon<f64>,
) -> Result<(), BooleanError> {
    if polygons.len() >= MAX_MULTIPOLYGON_POLYGONS {
        return Err(BooleanError::ResourceLimit);
    }
    *total_coordinates = total_coordinates
        .checked_add(polygon_coordinate_count(&polygon))
        .ok_or(BooleanError::ResourceLimit)?;
    if *total_coordinates > MAX_MULTIPOLYGON_COORDINATES {
        return Err(BooleanError::ResourceLimit);
    }
    polygons.push(polygon);
    Ok(())
}

fn bounded_output_coordinates(multi: &MultiPolygon<f64>) -> Result<(), BooleanError> {
    if multi.0.len() > MAX_MULTIPOLYGON_POLYGONS {
        return Err(BooleanError::ResourceLimit);
    }
    let coordinates = multi.0.iter().try_fold(0usize, |total, polygon| {
        total
            .checked_add(polygon_coordinate_count(polygon))
            .ok_or(BooleanError::ResourceLimit)
    })?;
    if coordinates > MAX_MULTIPOLYGON_COORDINATES {
        return Err(BooleanError::ResourceLimit);
    }
    Ok(())
}

fn ensure_supported_overlay_topology(
    left: &ValidatedMultiPolygon,
    right: &ValidatedMultiPolygon,
    scale: f64,
) -> Result<(), BooleanError> {
    if left
        .polygons
        .iter()
        .chain(&right.polygons)
        .all(|polygon| polygon.holes.is_empty() && ring_is_convex(&polygon.exterior, scale))
    {
        return Ok(());
    }
    Err(BooleanError::UnsupportedTopology)
}

fn ring_is_convex(ring: &[Point2], scale: f64) -> bool {
    let body = &ring[..ring.len() - 1];
    let mut orientation = 0.0;
    for index in 0..body.len() {
        let first = body[index];
        let second = body[(index + 1) % body.len()];
        let third = body[(index + 2) % body.len()];
        let first_x = first.x.get() / scale;
        let first_y = first.y.get() / scale;
        let second_x = second.x.get() / scale;
        let second_y = second.y.get() / scale;
        let third_x = third.x.get() / scale;
        let third_y = third.y.get() / scale;
        let cross =
            (second_x - first_x) * (third_y - first_y) - (second_y - first_y) * (third_x - first_x);
        if cross == 0.0 {
            continue;
        }
        if orientation == 0.0 {
            orientation = cross.signum();
        } else if cross.signum() != orientation {
            return false;
        }
    }
    orientation != 0.0
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
    if boundary_is_below_kernel_resolution(left, right, scale, minimum_separation) {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    Ok(())
}

fn polygon_rings(polygon: &ValidatedPolygon) -> impl Iterator<Item = &[Point2]> {
    std::iter::once(polygon.exterior.as_slice()).chain(polygon.holes.iter().map(Vec::as_slice))
}

fn boundary_is_below_kernel_resolution(
    left: &ValidatedMultiPolygon,
    right: &ValidatedMultiPolygon,
    scale: f64,
    minimum_separation: f64,
) -> bool {
    let point_is_too_close = |point: Point2, start: Point2, end: Point2| {
        let start_x = start.x.get() / scale;
        let start_y = start.y.get() / scale;
        let end_x = end.x.get() / scale;
        let end_y = end.y.get() / scale;
        let point_x = point.x.get() / scale;
        let point_y = point.y.get() / scale;
        let edge_x = end_x - start_x;
        let edge_y = end_y - start_y;
        let length_squared = edge_x.mul_add(edge_x, edge_y * edge_y);
        if !length_squared.is_finite() || length_squared == 0.0 {
            return false;
        }
        let projection =
            ((point_x - start_x) * edge_x + (point_y - start_y) * edge_y) / length_squared;
        if !(0.0..=1.0).contains(&projection) {
            return false;
        }
        let cross = (point_x - start_x) * edge_y - (point_y - start_y) * edge_x;
        let distance = cross.abs() / length_squared.sqrt();
        distance != 0.0 && distance < minimum_separation
    };
    let one_direction = |source: &ValidatedMultiPolygon, target: &ValidatedMultiPolygon| {
        source.polygons.iter().any(|source_polygon| {
            target.polygons.iter().any(|target_polygon| {
                polygon_rings(source_polygon).any(|source_ring| {
                    source_ring.iter().any(|point| {
                        polygon_rings(target_polygon).any(|target_ring| {
                            target_ring
                                .windows(2)
                                .any(|edge| point_is_too_close(*point, edge[0], edge[1]))
                        })
                    })
                })
            })
        })
    };
    one_direction(left, right) || one_direction(right, left)
}

fn has_interior_overlap(left: &MultiPolygon<f64>, right: &MultiPolygon<f64>) -> bool {
    left.0.iter().any(|left| {
        right
            .0
            .iter()
            .any(|right| polygons_have_interior_overlap(left, right))
    })
}

fn polygons_have_interior_overlap(left: &Polygon<f64>, right: &Polygon<f64>) -> bool {
    left.relate(right).get(CoordPos::Inside, CoordPos::Inside) == Dimensions::TwoDimensional
}

fn multipolygon_covers(container: &MultiPolygon<f64>, target: &MultiPolygon<f64>) -> bool {
    target
        .0
        .iter()
        .all(|target| container.0.iter().any(|container| container.covers(target)))
}

fn multipolygon_covers_with_tolerance(
    container: &MultiPolygon<f64>,
    target: &MultiPolygon<f64>,
) -> bool {
    multipolygon_covers_with_tolerance_floor(container, target, 1e-8)
}

fn multipolygon_covers_with_tolerance_floor(
    container: &MultiPolygon<f64>,
    target: &MultiPolygon<f64>,
    minimum_tolerance: f64,
) -> bool {
    target.0.iter().all(|target| {
        container.0.iter().any(|container| {
            polygon_covers_with_tolerance_floor(container, target, minimum_tolerance)
        })
    })
}

fn polygon_covers_with_tolerance(container: &Polygon<f64>, target: &Polygon<f64>) -> bool {
    polygon_covers_with_tolerance_floor(container, target, 1e-8)
}

fn polygon_covers_with_tolerance_floor(
    container: &Polygon<f64>,
    target: &Polygon<f64>,
    minimum_tolerance: f64,
) -> bool {
    let points = &container.exterior().0;
    if points.len() < 4 || points.first() != points.last() {
        return false;
    }
    let orientation = signed_area(points);
    if !orientation.is_finite() || orientation == 0.0 {
        return false;
    }
    let origin = points[0];
    let extent = points.iter().fold(0.0_f64, |extent, point| {
        extent
            .max((point.x - origin.x).abs())
            .max((point.y - origin.y).abs())
    });
    // Allow only ordinary f64 evaluation error. Kernel grid quantization that
    // exceeds this bound is rejected instead of being hidden by a scale-sized
    // geometric tolerance.
    let tolerance = (extent * f64::EPSILON * 32.0).max(minimum_tolerance);
    target.exterior().0.iter().all(|point| {
        points.windows(2).all(|edge| {
            let edge_x = edge[1].x - edge[0].x;
            let edge_y = edge[1].y - edge[0].y;
            let cross = edge_x * (point.y - edge[0].y) - edge_y * (point.x - edge[0].x);
            cross * orientation.signum() >= -tolerance * edge_x.hypot(edge_y)
        })
    })
}

fn ensure_intersection_completeness(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
    result: &MultiPolygon<f64>,
) -> Result<(), BooleanError> {
    for left in &left.0 {
        for right in &right.0 {
            let expected = convex_intersection_area(left, right)?;
            let actual = checked_sum(
                result
                    .0
                    .iter()
                    .filter(|candidate| {
                        polygon_covers_with_tolerance(left, candidate)
                            && polygon_covers_with_tolerance(right, candidate)
                    })
                    .map(polygon_area),
            )?;
            if !areas_match(actual, expected) {
                return Err(BooleanError::UnsupportedCoordinateResolution);
            }
        }
    }
    Ok(())
}

fn ensure_difference_completeness(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
    result: &MultiPolygon<f64>,
) -> Result<(), BooleanError> {
    // For each left component, containment of the result in left, negligible
    // result/right overlap, and area conservation provide a bounded numerical
    // certificate under the explicit finite-precision tolerance. This is stronger than
    // merely requiring one surviving result component per left component.
    if has_significant_interior_overlap(result, right)? {
        return Err(BooleanError::InvalidKernelResult);
    }
    for left in &left.0 {
        let expected_overlap = right
            .0
            .iter()
            .map(|right| convex_intersection_area(left, right))
            .collect::<Result<Vec<_>, _>>()
            .and_then(checked_sum)?;
        let left_area = polygon_area(left);
        if !left_area.is_finite() || left_area <= 0.0 {
            return Err(BooleanError::InvalidKernelResult);
        }
        let expected = left_area - expected_overlap;
        if !expected.is_finite() || expected < 0.0 {
            return Err(BooleanError::UnsupportedCoordinateResolution);
        }
        if expected > 0.0 && expected / left_area < 1e-12 {
            return Err(BooleanError::UnsupportedCoordinateResolution);
        }
        let actual = checked_sum(
            result
                .0
                .iter()
                .filter(|candidate| polygon_covers_with_tolerance(left, candidate))
                .map(polygon_area),
        )?;
        if !areas_match(actual, expected) {
            return Err(BooleanError::UnsupportedCoordinateResolution);
        }
    }
    Ok(())
}

fn has_significant_interior_overlap(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
) -> Result<bool, BooleanError> {
    let mut work_used = 0usize;
    for left in &left.0 {
        for right in &right.0 {
            let pair_work = polygon_coordinate_count(left)
                .checked_mul(polygon_coordinate_count(right))
                .ok_or(BooleanError::ResourceLimit)?;
            work_used = work_used
                .checked_add(pair_work)
                .ok_or(BooleanError::ResourceLimit)?;
            if work_used > MAX_BOOLEAN_WORK {
                return Err(BooleanError::ResourceLimit);
            }
            let overlap = left.intersection(right);
            bounded_output_coordinates(&overlap)?;
            let area = checked_sum(overlap.0.iter().map(polygon_area))?;
            if area > BOOLEAN_AREA_ERROR_TOLERANCE {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Independently compute the intersection of two convex exteriors. The
/// operation boundary rejects non-convex and holed inputs before reaching this
/// helper. Sutherland-Hodgman clipping supplies a bounded candidate whose
/// containment and per-component area are checked independently of the pinned
/// overlay kernel.
fn convex_intersection_polygon(
    left: &Polygon<f64>,
    right: &Polygon<f64>,
) -> Result<Option<Polygon<f64>>, BooleanError> {
    let subject = open_exterior(left)?;
    let clip = open_exterior(right)?;
    let orientation = signed_area(&clip);
    if !orientation.is_finite() || orientation == 0.0 {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    let work = subject
        .len()
        .checked_mul(clip.len())
        .ok_or(BooleanError::ResourceLimit)?;
    if work > MAX_BOOLEAN_WORK {
        return Err(BooleanError::ResourceLimit);
    }
    let mut subject = subject;
    for index in 0..clip.len() {
        let edge_start = clip[index];
        let edge_end = clip[(index + 1) % clip.len()];
        let previous = subject;
        if previous.is_empty() {
            return Ok(None);
        }
        let mut next = Vec::new();
        let mut previous_point = *previous.last().ok_or(BooleanError::InvalidKernelResult)?;
        let mut previous_inside = inside_clip(previous_point, edge_start, edge_end, orientation)?;
        for current_point in previous {
            if next.len() >= MAX_POLYGON_COORDINATES {
                return Err(BooleanError::ResourceLimit);
            }
            let current_inside = inside_clip(current_point, edge_start, edge_end, orientation)?;
            if current_inside != previous_inside {
                if next.len() >= MAX_POLYGON_COORDINATES {
                    return Err(BooleanError::ResourceLimit);
                }
                next.push(line_boundary_intersection(
                    previous_point,
                    current_point,
                    edge_start,
                    edge_end,
                )?);
            }
            if current_inside {
                next.push(current_point);
            }
            previous_point = current_point;
            previous_inside = current_inside;
        }
        subject = next;
    }
    if subject.len() < 3 {
        return Ok(None);
    }
    let area = signed_area(&subject).abs();
    if !area.is_finite() {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    if area == 0.0 {
        return Ok(None);
    }
    let mut closed = subject;
    closed.push(closed[0]);
    Ok(Some(Polygon::new(LineString::from(closed), Vec::new())))
}

fn convex_intersection_area(
    left: &Polygon<f64>,
    right: &Polygon<f64>,
) -> Result<f64, BooleanError> {
    match convex_intersection_polygon(left, right)? {
        Some(polygon) => Ok(polygon_area(&polygon)),
        None => Ok(0.0),
    }
}

fn checked_sum<I>(values: I) -> Result<f64, BooleanError>
where
    I: IntoIterator<Item = f64>,
{
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in values {
        if !value.is_finite() {
            return Err(BooleanError::UnsupportedCoordinateResolution);
        }
        let next = sum + value;
        if !next.is_finite() {
            return Err(BooleanError::ResourceLimit);
        }
        compensation += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        sum = next;
    }
    let result = sum + compensation;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(BooleanError::ResourceLimit)
    }
}

fn open_exterior(polygon: &Polygon<f64>) -> Result<Vec<Coord<f64>>, BooleanError> {
    let points = polygon.exterior().0.as_slice();
    if points.len() < 4 || points.first() != points.last() {
        return Err(BooleanError::InvalidKernelResult);
    }
    Ok(points[..points.len() - 1].to_vec())
}

fn signed_area(points: &[Coord<f64>]) -> f64 {
    let Some(origin) = points.first().copied() else {
        return 0.0;
    };
    let mut sum = 0.0;
    let mut compensation = 0.0;
    let terms = points.iter().enumerate().map(|(index, first)| {
        let second = points[(index + 1) % points.len()];
        let first_x = first.x - origin.x;
        let first_y = first.y - origin.y;
        let second_x = second.x - origin.x;
        let second_y = second.y - origin.y;
        first_x * second_y - second_x * first_y
    });
    for term in terms {
        let next = sum + term;
        let correction = if sum.abs() >= term.abs() {
            (sum - next) + term
        } else {
            (term - next) + sum
        };
        compensation += correction;
        sum = next;
    }
    (sum + compensation) / 2.0
}

fn polygon_area(polygon: &Polygon<f64>) -> f64 {
    let hole_area = polygon
        .interiors()
        .iter()
        .map(|ring| signed_area(&ring.0).abs())
        .fold(0.0, |sum, area| sum + area);
    signed_area(&polygon.exterior().0).abs() - hole_area
}

fn inside_clip(
    point: Coord<f64>,
    edge_start: Coord<f64>,
    edge_end: Coord<f64>,
    orientation: f64,
) -> Result<bool, BooleanError> {
    let cross = (edge_end.x - edge_start.x) * (point.y - edge_start.y)
        - (edge_end.y - edge_start.y) * (point.x - edge_start.x);
    if !cross.is_finite() {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    Ok(cross * orientation >= 0.0)
}

fn line_boundary_intersection(
    subject_start: Coord<f64>,
    subject_end: Coord<f64>,
    edge_start: Coord<f64>,
    edge_end: Coord<f64>,
) -> Result<Coord<f64>, BooleanError> {
    let subject_delta = Coord {
        x: subject_end.x - subject_start.x,
        y: subject_end.y - subject_start.y,
    };
    let edge_delta = Coord {
        x: edge_end.x - edge_start.x,
        y: edge_end.y - edge_start.y,
    };
    let denominator = subject_delta.x * edge_delta.y - subject_delta.y * edge_delta.x;
    if !denominator.is_finite() || denominator == 0.0 {
        return Err(BooleanError::UnsupportedCoordinateResolution);
    }
    let offset = Coord {
        x: edge_start.x - subject_start.x,
        y: edge_start.y - subject_start.y,
    };
    let parameter = (offset.x * edge_delta.y - offset.y * edge_delta.x) / denominator;
    let point = Coord {
        x: subject_start.x + parameter * subject_delta.x,
        y: subject_start.y + parameter * subject_delta.y,
    };
    if point.x.is_finite() && point.y.is_finite() {
        Ok(point)
    } else {
        Err(BooleanError::UnsupportedCoordinateResolution)
    }
}

fn areas_match(actual: f64, expected: f64) -> bool {
    if !actual.is_finite() || !expected.is_finite() || actual < 0.0 || expected < 0.0 {
        return false;
    }
    (actual - expected).abs() <= BOOLEAN_AREA_ERROR_TOLERANCE
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]),
            Vec::new(),
        )
    }

    #[test]
    fn convex_verifier_rejects_excess_intermediate_work() {
        let mut coordinates = Vec::with_capacity(MAX_POLYGON_COORDINATES + 1);
        for index in 0..MAX_POLYGON_COORDINATES {
            let angle = index as f64 * std::f64::consts::TAU / MAX_POLYGON_COORDINATES as f64;
            coordinates.push(Coord {
                x: angle.cos(),
                y: angle.sin(),
            });
        }
        coordinates.push(coordinates[0]);
        let polygon = Polygon::new(LineString::from(coordinates), Vec::new());

        assert_eq!(
            convex_intersection_polygon(&polygon, &polygon),
            Err(BooleanError::ResourceLimit)
        );
    }

    #[test]
    fn difference_verifier_rejects_missing_small_residual_beside_large_one() {
        let left = MultiPolygon(vec![rectangle(0.0, 0.0, 1_000_000_000.0, 1_000_000_000.0)]);
        let right = MultiPolygon(vec![
            rectangle(1.0, 0.0, 2.0, 1_000_000_000.0),
            rectangle(999_999_998.0, 0.0, 999_999_999.9, 1_000_000_000.0),
        ]);
        let result = MultiPolygon(vec![
            rectangle(0.0, 0.0, 1.0, 1_000_000_000.0),
            rectangle(2.0, 0.0, 999_999_998.0, 1_000_000_000.0),
        ]);

        assert_eq!(
            ensure_difference_completeness(&left, &right, &result),
            Err(BooleanError::UnsupportedCoordinateResolution)
        );
    }
}
