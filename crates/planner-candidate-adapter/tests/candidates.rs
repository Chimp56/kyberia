use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
    units::{CoordinateMeters, Meters},
};
use kyberia_geometry_adapter::{BooleanError, ValidatedMultiPolygon, ValidatedPolygon};
use kyberia_planner_candidate_adapter::{
    CableRoute, CandidateGenerationError, MAX_EXCLUSION_REGIONS, MAX_ROUTE_STATIONS,
    generate_candidates,
};
use kyberia_planner_evaluator::CandidateId;

fn floor() -> FloorId {
    FloorId::from_bytes([1; 16]).unwrap()
}

fn frame() -> FrameId {
    FrameId::from_bytes([2; 16]).unwrap()
}

fn point(x: f64, y: f64) -> Point2 {
    Point2 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
    }
}

fn ring(points: &[(f64, f64)]) -> Vec<Point2> {
    points.iter().map(|&(x, y)| point(x, y)).collect()
}

fn region(exterior: &[(f64, f64)], holes: &[&[(f64, f64)]]) -> ValidatedMultiPolygon {
    let polygon = ValidatedPolygon::new(
        floor(),
        frame(),
        ring(exterior),
        holes.iter().map(|hole| ring(hole)).collect(),
    )
    .unwrap();
    polygon.as_multipolygon()
}

fn square() -> ValidatedMultiPolygon {
    region(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0., 0.)], &[])
}

fn route(points: &[(f64, f64)]) -> Vec<Point2> {
    ring(points)
}

#[test]
fn route_stations_are_deterministic_and_follow_metric_distance_through_a_turn() {
    let allowed = region(
        &[(-1., -1.), (5., -1.), (5., 5.), (-1., 5.), (-1., -1.)],
        &[],
    );
    let points = route(&[(0., 0.), (3., 0.), (3., 4.)]);
    let cable = CableRoute::new(floor(), frame(), &points);
    let first = generate_candidates(
        &allowed,
        &[],
        cable,
        Meters::new(2.).unwrap(),
        Meters::new(7.).unwrap(),
    )
    .unwrap();
    let repeated = generate_candidates(
        &allowed,
        &[],
        cable,
        Meters::new(2.).unwrap(),
        Meters::new(7.).unwrap(),
    )
    .unwrap();

    assert_eq!(first, repeated);
    assert_eq!(
        first.iter().map(|station| station.id).collect::<Vec<_>>(),
        [
            CandidateId(0),
            CandidateId(1),
            CandidateId(2),
            CandidateId(3)
        ]
    );
    assert_eq!(
        first
            .iter()
            .map(|station| station.point)
            .collect::<Vec<_>>(),
        [point(0., 0.), point(2., 0.), point(3., 1.), point(3., 3.)]
    );
    assert_eq!(
        first
            .iter()
            .map(|station| station.route_distance.get())
            .collect::<Vec<_>>(),
        [0., 2., 4., 6.]
    );
}

#[test]
fn station_exactly_at_route_vertex_is_emitted_at_the_shared_turn_point() {
    let allowed = region(
        &[(-1., -1.), (5., -1.), (5., 5.), (-1., 5.), (-1., -1.)],
        &[],
    );
    let points = route(&[(0., 0.), (3., 0.), (3., 4.)]);
    let candidates = generate_candidates(
        &allowed,
        &[],
        CableRoute::new(floor(), frame(), &points),
        Meters::new(3.).unwrap(),
        Meters::new(7.).unwrap(),
    )
    .unwrap();

    assert_eq!(
        candidates
            .iter()
            .map(|station| station.id)
            .collect::<Vec<_>>(),
        [CandidateId(0), CandidateId(1), CandidateId(2)]
    );
    assert_eq!(
        candidates
            .iter()
            .map(|station| station.point)
            .collect::<Vec<_>>(),
        [point(0., 0.), point(3., 0.), point(3., 3.)]
    );
    assert_eq!(
        candidates
            .iter()
            .map(|station| station.route_distance.get())
            .collect::<Vec<_>>(),
        [0., 3., 6.]
    );
}

#[test]
fn allowed_boundary_is_included_and_exclusion_boundary_is_excluded_without_renumbering() {
    let allowed = square();
    let exclusion = region(&[(4., 4.), (6., 4.), (6., 6.), (4., 6.), (4., 4.)], &[]);
    let points = route(&[(0., 5.), (10., 5.)]);
    let candidates = generate_candidates(
        &allowed,
        &[exclusion],
        CableRoute::new(floor(), frame(), &points),
        Meters::new(2.).unwrap(),
        Meters::new(10.).unwrap(),
    )
    .unwrap();

    assert_eq!(
        candidates
            .iter()
            .map(|station| station.id)
            .collect::<Vec<_>>(),
        [
            CandidateId(0),
            CandidateId(1),
            CandidateId(4),
            CandidateId(5)
        ]
    );
    assert_eq!(
        candidates
            .iter()
            .map(|station| station.point)
            .collect::<Vec<_>>(),
        [point(0., 5.), point(2., 5.), point(8., 5.), point(10., 5.)]
    );
}

#[test]
fn allowed_hole_interior_is_pruned_and_its_boundary_follows_boundary_policy() {
    let allowed = region(
        &[(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0., 0.)],
        &[&[(4., 4.), (6., 4.), (6., 6.), (4., 6.), (4., 4.)]],
    );
    let points = route(&[(0., 5.), (10., 5.)]);
    let candidates = generate_candidates(
        &allowed,
        &[],
        CableRoute::new(floor(), frame(), &points),
        Meters::new(1.).unwrap(),
        Meters::new(10.).unwrap(),
    )
    .unwrap();

    assert_eq!(
        candidates
            .iter()
            .map(|station| station.id)
            .collect::<Vec<_>>(),
        [
            CandidateId(0),
            CandidateId(1),
            CandidateId(2),
            CandidateId(3),
            CandidateId(4),
            CandidateId(6),
            CandidateId(7),
            CandidateId(8),
            CandidateId(9),
            CandidateId(10)
        ]
    );
}

#[test]
fn cable_length_threshold_is_inclusive_and_excess_rejects_the_whole_request() {
    let allowed = square();
    let points = route(&[(0., 5.), (10., 5.)]);
    let cable = CableRoute::new(floor(), frame(), &points);
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            cable,
            Meters::new(10.).unwrap(),
            Meters::new(10.).unwrap(),
        )
        .unwrap()
        .len(),
        2
    );
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            cable,
            Meters::new(10.).unwrap(),
            Meters::new(9.999).unwrap(),
        ),
        Err(CandidateGenerationError::CableLengthExceeded)
    );
}

#[test]
fn invalid_spacing_route_identity_and_degenerate_segments_fail_closed() {
    let allowed = square();
    let points = route(&[(0., 5.), (10., 5.)]);
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            CableRoute::new(floor(), frame(), &points),
            Meters::new(0.).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::InvalidSpacing)
    );
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            CableRoute::new(FloorId::from_bytes([3; 16]).unwrap(), frame(), &points),
            Meters::new(1.).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::FloorMismatch)
    );
    let duplicate = route(&[(0., 5.), (0., 5.), (10., 5.)]);
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            CableRoute::new(floor(), frame(), &duplicate),
            Meters::new(1.).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::DegenerateRouteSegment)
    );
}

#[test]
fn candidate_generation_rejects_route_frame_mismatch() {
    let allowed = square();
    let points = route(&[(0., 5.), (10., 5.)]);
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            CableRoute::new(floor(), FrameId::from_bytes([3; 16]).unwrap(), &points),
            Meters::new(1.).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::FrameMismatch)
    );
}

#[test]
fn station_and_aggregate_geometry_work_limits_reject_without_truncation() {
    let allowed = square();
    let points = route(&[(0., 5.), (10., 5.)]);
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            CableRoute::new(floor(), frame(), &points),
            Meters::new(0.0001).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::ResourceLimit)
    );

    let many_vertices = (0..MAX_ROUTE_STATIONS + 1)
        .map(|index| point(index as f64, 5.))
        .collect::<Vec<_>>();
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            CableRoute::new(floor(), frame(), &many_vertices),
            Meters::new(1.).unwrap(),
            Meters::new(100_000.).unwrap(),
        ),
        Err(CandidateGenerationError::TooManyRoutePoints)
    );
}

#[test]
fn excessive_exclusion_region_count_is_not_silently_truncated() {
    let allowed = square();
    let exclusions = vec![square(); MAX_EXCLUSION_REGIONS + 1];
    let points = route(&[(0., 5.), (10., 5.)]);
    assert_eq!(
        generate_candidates(
            &allowed,
            &exclusions,
            CableRoute::new(floor(), frame(), &points),
            Meters::new(1.).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::TooManyExclusionRegions)
    );
}

#[test]
fn excessive_total_region_coordinates_fail_before_candidate_classification() {
    let allowed = square();
    let mut circle = (0..128)
        .map(|index| {
            let angle = std::f64::consts::TAU * f64::from(index) / 128.0;
            point(20. + angle.cos(), 20. + angle.sin())
        })
        .collect::<Vec<_>>();
    circle.push(circle[0]);
    let exclusion = ValidatedPolygon::new(floor(), frame(), circle, vec![])
        .unwrap()
        .as_multipolygon();
    let exclusions = vec![exclusion; MAX_EXCLUSION_REGIONS];
    let points = route(&[(0., 5.), (10., 5.)]);

    assert_eq!(
        generate_candidates(
            &allowed,
            &exclusions,
            CableRoute::new(floor(), frame(), &points),
            Meters::new(1.).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::TooManyRegionCoordinates)
    );
}

#[test]
fn aggregate_work_includes_visits_to_empty_exclusion_regions() {
    let allowed = square();
    let exclusions = vec![ValidatedMultiPolygon::empty(floor(), frame()); MAX_EXCLUSION_REGIONS];
    let points = route(&[(0., 5.), (10., 5.)]);

    assert_eq!(
        generate_candidates(
            &allowed,
            &exclusions,
            CableRoute::new(floor(), frame(), &points),
            Meters::new(0.0005).unwrap(),
            Meters::new(10.).unwrap(),
        ),
        Err(CandidateGenerationError::ResourceLimit)
    );
}

#[test]
fn removing_an_exclusion_only_restores_its_stations_with_original_ids() {
    let allowed = square();
    let exclusion = region(&[(4., 4.), (6., 4.), (6., 6.), (4., 6.), (4., 4.)], &[]);
    let points = route(&[(0., 5.), (10., 5.)]);
    let cable = CableRoute::new(floor(), frame(), &points);
    let without = generate_candidates(
        &allowed,
        &[],
        cable,
        Meters::new(2.).unwrap(),
        Meters::new(10.).unwrap(),
    )
    .unwrap();
    let with = generate_candidates(
        &allowed,
        &[exclusion],
        cable,
        Meters::new(2.).unwrap(),
        Meters::new(10.).unwrap(),
    )
    .unwrap();

    assert_eq!(
        without
            .iter()
            .filter(|candidate| !with.iter().any(|retained| retained.id == candidate.id))
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>(),
        [CandidateId(2), CandidateId(3)]
    );
}

#[test]
fn route_length_uses_the_full_polyline_not_just_endpoint_distance() {
    let allowed = region(
        &[(-1., -1.), (11., -1.), (11., 11.), (-1., 11.), (-1., -1.)],
        &[],
    );
    let points = route(&[(0., 0.), (10., 0.), (10., 10.)]);
    assert_eq!(
        generate_candidates(
            &allowed,
            &[],
            CableRoute::new(floor(), frame(), &points),
            Meters::new(2.).unwrap(),
            Meters::new(15.).unwrap(),
        ),
        Err(CandidateGenerationError::CableLengthExceeded)
    );
}

#[test]
fn multipolygon_constructor_rejects_mismatched_components_before_generation() {
    let first = ValidatedPolygon::new(
        floor(),
        frame(),
        ring(&[(0., 0.), (5., 0.), (5., 5.), (0., 5.), (0., 0.)]),
        vec![],
    )
    .unwrap();
    let other_floor = ValidatedPolygon::new(
        FloorId::from_bytes([3; 16]).unwrap(),
        frame(),
        ring(&[(6., 6.), (8., 6.), (8., 8.), (6., 8.), (6., 6.)]),
        vec![],
    )
    .unwrap();
    assert!(matches!(
        ValidatedMultiPolygon::new(floor(), frame(), vec![first, other_floor]),
        Err(BooleanError::FloorMismatch)
    ));
}
