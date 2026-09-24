//! Deterministic candidate stations along one caller-supplied cable route.
//!
//! This component does not infer routes from building infrastructure, generate
//! floor-wide grids/Poisson samples, model mounting or power, predict RF, or
//! optimize placements. The supplied polyline is treated as the complete
//! cable-length evidence for this bounded operation.

use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
    units::Meters,
};
use kyberia_geometry_adapter::{
    GeometryError, MAX_ABSOLUTE_COORDINATE_METERS, PointLocation, ValidatedMultiPolygon,
};
use kyberia_planner_evaluator::CandidateId;

/// Maximum number of vertices in a single supplied cable route.
pub const MAX_ROUTE_POINTS: usize = 4_096;
/// Maximum number of regularly spaced stations considered, before region
/// pruning. No station list is truncated to meet this limit.
pub const MAX_ROUTE_STATIONS: usize = 65_536;
/// Maximum conservative work units admitted per request, including route
/// traversal, station enumeration, region-object visits, and coordinate scans.
pub const MAX_CANDIDATE_WORK: usize = 4_194_304;
/// Maximum number of exclusion regions supplied in one request.
pub const MAX_EXCLUSION_REGIONS: usize = 256;
/// Maximum total coordinates across the allowed region and exclusions.
pub const MAX_TOTAL_REGION_COORDINATES: usize = 32_768;

/// A polyline with explicit floor/frame identity; coordinates are already in
/// that floor-local metric frame and are never transformed by this adapter.
#[derive(Clone, Copy, Debug)]
pub struct CableRoute<'a> {
    floor_id: FloorId,
    frame_id: FrameId,
    points: &'a [Point2],
}

impl<'a> CableRoute<'a> {
    pub fn new(floor_id: FloorId, frame_id: FrameId, points: &'a [Point2]) -> Self {
        Self {
            floor_id,
            frame_id,
            points,
        }
    }

    pub fn floor_id(self) -> FloorId {
        self.floor_id
    }

    pub fn frame_id(self) -> FrameId {
        self.frame_id
    }

    pub fn points(self) -> &'a [Point2] {
        self.points
    }
}

/// A candidate's ID is its zero-based station ordinal along the original
/// route. Pruning never renumbers surviving candidates, so IDs remain stable
/// when allowed/excluded regions change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidateStation {
    pub id: CandidateId,
    pub route_distance: Meters,
    pub point: Point2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateGenerationError {
    InvalidSpacing,
    RouteTooShort,
    TooManyRoutePoints,
    TooManyExclusionRegions,
    TooManyRegionCoordinates,
    FloorMismatch,
    FrameMismatch,
    CoordinateOutOfBounds,
    DegenerateRouteSegment,
    UnsupportedRouteResolution,
    CableLengthExceeded,
    ResourceLimit,
    ArithmeticOverflow,
    Geometry(GeometryError),
}

impl std::fmt::Display for CandidateGenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "candidate generation rejected: {self:?}")
    }
}

impl std::error::Error for CandidateGenerationError {}

impl From<GeometryError> for CandidateGenerationError {
    fn from(value: GeometryError) -> Self {
        Self::Geometry(value)
    }
}

/// Generate stations at route distances `0, spacing, 2*spacing, ...` that do
/// not exceed the complete polyline length. Exact route vertices use the
/// following segment for interpolation; the point remains the shared vertex.
/// Allowed-region boundaries (including hole boundaries) are included, while
/// exclusion-region boundaries and hole interiors are excluded. Any failure
/// returns no partial candidate vector.
pub fn generate_candidates(
    allowed_region: &ValidatedMultiPolygon,
    exclusion_regions: &[ValidatedMultiPolygon],
    cable_route: CableRoute<'_>,
    spacing: Meters,
    maximum_cable_length: Meters,
) -> Result<Vec<CandidateStation>, CandidateGenerationError> {
    if spacing.get() <= 0.0 {
        return Err(CandidateGenerationError::InvalidSpacing);
    }
    if cable_route.points.len() < 2 {
        return Err(CandidateGenerationError::RouteTooShort);
    }
    if cable_route.points.len() > MAX_ROUTE_POINTS {
        return Err(CandidateGenerationError::TooManyRoutePoints);
    }
    if exclusion_regions.len() > MAX_EXCLUSION_REGIONS {
        return Err(CandidateGenerationError::TooManyExclusionRegions);
    }
    if allowed_region.floor_id() != cable_route.floor_id
        || exclusion_regions
            .iter()
            .any(|region| region.floor_id() != cable_route.floor_id)
    {
        return Err(CandidateGenerationError::FloorMismatch);
    }
    if allowed_region.frame_id() != cable_route.frame_id
        || exclusion_regions
            .iter()
            .any(|region| region.frame_id() != cable_route.frame_id)
    {
        return Err(CandidateGenerationError::FrameMismatch);
    }

    let region_coordinates = region_coordinate_count(allowed_region, exclusion_regions)?;
    if region_coordinates > MAX_TOTAL_REGION_COORDINATES {
        return Err(CandidateGenerationError::TooManyRegionCoordinates);
    }

    let mut segment_lengths = Vec::with_capacity(cable_route.points.len() - 1);
    let mut total_length = 0.0_f64;
    for point in cable_route.points {
        for coordinate in [point.x.get(), point.y.get()] {
            if !coordinate.is_finite() || coordinate.abs() > MAX_ABSOLUTE_COORDINATE_METERS {
                return Err(CandidateGenerationError::CoordinateOutOfBounds);
            }
        }
    }
    for pair in cable_route.points.windows(2) {
        let dx = pair[1].x.get() - pair[0].x.get();
        let dy = pair[1].y.get() - pair[0].y.get();
        let length = dx.hypot(dy);
        if !length.is_finite() {
            return Err(CandidateGenerationError::ArithmeticOverflow);
        }
        if length == 0.0 {
            return Err(CandidateGenerationError::DegenerateRouteSegment);
        }
        let next_total = total_length + length;
        if !next_total.is_finite() {
            return Err(CandidateGenerationError::ArithmeticOverflow);
        }
        if next_total <= total_length {
            return Err(CandidateGenerationError::UnsupportedRouteResolution);
        }
        segment_lengths.push(length);
        total_length = next_total;
    }
    if total_length > maximum_cable_length.get() {
        return Err(CandidateGenerationError::CableLengthExceeded);
    }

    let station_count = checked_station_count(total_length, spacing.get())?;
    let region_objects = exclusion_regions
        .len()
        .checked_add(1)
        .ok_or(CandidateGenerationError::ArithmeticOverflow)?;
    let region_polygons = exclusion_regions
        .iter()
        .try_fold(allowed_region.polygons().len(), |count, region| {
            count.checked_add(region.polygons().len())
        })
        .ok_or(CandidateGenerationError::ArithmeticOverflow)?;
    // Each point-location query can make four linear passes over each
    // region's coordinates (bounds, normalization scale, geo input mapping,
    // and point-in-ring classification), plus a visit to every polygon and
    // region object. Station preflight/emission adds two units per station. Route validation,
    // segment-length calculation and monotone interpolation are bounded by
    // three passes over route points. Count the initial region-size scan too.
    let per_station_work = region_coordinates
        .checked_mul(4)
        .and_then(|work| work.checked_add(region_polygons))
        .and_then(|work| work.checked_add(region_objects))
        .and_then(|work| work.checked_add(2))
        .ok_or(CandidateGenerationError::ArithmeticOverflow)?;
    let total_work = station_count
        .checked_mul(per_station_work)
        .and_then(|work| work.checked_add(region_coordinates))
        .and_then(|work| {
            cable_route
                .points
                .len()
                .checked_mul(3)
                .and_then(|route_work| work.checked_add(route_work))
        })
        .ok_or(CandidateGenerationError::ArithmeticOverflow)?;
    if total_work > MAX_CANDIDATE_WORK {
        return Err(CandidateGenerationError::ResourceLimit);
    }

    let mut candidates = Vec::new();
    let mut segment_index = 0usize;
    let mut segment_start_distance = 0.0_f64;
    for station_index in 0..station_count {
        let route_distance = spacing.get() * station_index as f64;
        if !route_distance.is_finite() {
            return Err(CandidateGenerationError::ArithmeticOverflow);
        }
        while segment_index + 1 < segment_lengths.len()
            && route_distance >= segment_start_distance + segment_lengths[segment_index]
        {
            segment_start_distance += segment_lengths[segment_index];
            segment_index += 1;
        }
        let point = interpolate(
            cable_route.points[segment_index],
            cable_route.points[segment_index + 1],
            segment_lengths[segment_index],
            route_distance - segment_start_distance,
        )?;
        if !eligible_point(allowed_region, exclusion_regions, cable_route, point)? {
            continue;
        }
        let station_ordinal =
            u32::try_from(station_index).map_err(|_| CandidateGenerationError::ResourceLimit)?;
        candidates.push(CandidateStation {
            id: CandidateId(station_ordinal),
            route_distance: Meters::new(route_distance)
                .map_err(|_| CandidateGenerationError::ArithmeticOverflow)?,
            point,
        });
    }
    Ok(candidates)
}

fn checked_station_count(
    route_length: f64,
    spacing: f64,
) -> Result<usize, CandidateGenerationError> {
    let mut count = 0usize;
    let mut previous = None;
    loop {
        let distance = spacing * count as f64;
        if !distance.is_finite() {
            return Err(CandidateGenerationError::ArithmeticOverflow);
        }
        if distance > route_length {
            break;
        }
        if previous.is_some_and(|last| distance <= last) {
            return Err(CandidateGenerationError::UnsupportedRouteResolution);
        }
        count = count
            .checked_add(1)
            .ok_or(CandidateGenerationError::ArithmeticOverflow)?;
        if count > MAX_ROUTE_STATIONS {
            return Err(CandidateGenerationError::ResourceLimit);
        }
        previous = Some(distance);
    }
    Ok(count)
}

fn interpolate(
    start: Point2,
    end: Point2,
    segment_length: f64,
    distance_from_start: f64,
) -> Result<Point2, CandidateGenerationError> {
    if distance_from_start <= 0.0 {
        return Ok(start);
    }
    if distance_from_start >= segment_length {
        return Ok(end);
    }
    let fraction = distance_from_start / segment_length;
    let x = start.x.get() + (end.x.get() - start.x.get()) * fraction;
    let y = start.y.get() + (end.y.get() - start.y.get()) * fraction;
    if !x.is_finite() || !y.is_finite() {
        return Err(CandidateGenerationError::ArithmeticOverflow);
    }
    Ok(Point2 {
        x: kyberia_domain::units::CoordinateMeters::new(x)
            .map_err(|_| CandidateGenerationError::ArithmeticOverflow)?,
        y: kyberia_domain::units::CoordinateMeters::new(y)
            .map_err(|_| CandidateGenerationError::ArithmeticOverflow)?,
    })
}

fn eligible_point(
    allowed_region: &ValidatedMultiPolygon,
    exclusion_regions: &[ValidatedMultiPolygon],
    route: CableRoute<'_>,
    point: Point2,
) -> Result<bool, CandidateGenerationError> {
    match allowed_region.locate_point(route.floor_id, route.frame_id, point)? {
        PointLocation::Outside => return Ok(false),
        PointLocation::Boundary | PointLocation::Inside => {}
    }
    for exclusion in exclusion_regions {
        if exclusion.locate_point(route.floor_id, route.frame_id, point)? != PointLocation::Outside
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn region_coordinate_count(
    allowed_region: &ValidatedMultiPolygon,
    exclusions: &[ValidatedMultiPolygon],
) -> Result<usize, CandidateGenerationError> {
    let mut total = coordinate_count(allowed_region)?;
    for region in exclusions {
        total = total
            .checked_add(coordinate_count(region)?)
            .ok_or(CandidateGenerationError::ArithmeticOverflow)?;
    }
    Ok(total)
}

fn coordinate_count(region: &ValidatedMultiPolygon) -> Result<usize, CandidateGenerationError> {
    region.polygons().iter().try_fold(0usize, |total, polygon| {
        let count = polygon
            .holes()
            .iter()
            .try_fold(polygon.exterior().len(), |count, hole| {
                count.checked_add(hole.len())
            })
            .ok_or(CandidateGenerationError::ArithmeticOverflow)?;
        total
            .checked_add(count)
            .ok_or(CandidateGenerationError::ArithmeticOverflow)
    })
}
