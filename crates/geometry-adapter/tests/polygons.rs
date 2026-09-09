use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
    units::CoordinateMeters,
};
use kyberia_geometry_adapter::{
    BooleanError, PolygonError, ValidatedMultiPolygon, ValidatedPolygon,
};
fn ring(points: &[(f64, f64)]) -> Vec<Point2> {
    points
        .iter()
        .map(|&(x, y)| Point2 {
            x: CoordinateMeters::new(x).unwrap(),
            y: CoordinateMeters::new(y).unwrap(),
        })
        .collect()
}
fn polygon(
    exterior: Vec<Point2>,
    holes: Vec<Vec<Point2>>,
) -> Result<ValidatedPolygon, PolygonError> {
    ValidatedPolygon::new(
        FloorId::from_bytes([1; 16]).unwrap(),
        FrameId::from_bytes([2; 16]).unwrap(),
        exterior,
        holes,
    )
}
fn square() -> Vec<Point2> {
    ring(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0., 0.)])
}
#[test]
fn closed_polygon_preserves_floor_frame_holes_and_input_coordinates() {
    let exterior = square();
    let hole = ring(&[(2., 2.), (4., 2.), (4., 4.), (2., 4.), (2., 2.)]);
    let result = polygon(exterior.clone(), vec![hole.clone()]).unwrap();
    assert_eq!(result.exterior(), exterior);
    assert_eq!(result.holes(), vec![hole]);
    assert_eq!(result.floor_id(), FloorId::from_bytes([1; 16]).unwrap());
    assert_eq!(result.frame_id(), FrameId::from_bytes([2; 16]).unwrap());
}
#[test]
fn malformed_and_crossing_rings_fail_without_implicit_repair() {
    assert_eq!(
        polygon(ring(&[(0., 0.), (1., 0.), (2., 0.), (0., 0.)]), vec![]),
        Err(PolygonError::InvalidTopology)
    );
    let mut open = square();
    open.pop();
    assert_eq!(
        polygon(open, vec![]),
        Err(PolygonError::UnclosedOrDegenerateRing)
    );
    assert_eq!(
        polygon(
            ring(&[(0., 0.), (2., 2.), (0., 2.), (2., 0.), (0., 0.)]),
            vec![]
        ),
        Err(PolygonError::InvalidTopology)
    );
    assert_eq!(
        polygon(
            square(),
            vec![ring(&[
                (20., 20.),
                (22., 20.),
                (22., 22.),
                (20., 22.),
                (20., 20.)
            ])]
        ),
        Err(PolygonError::InvalidTopology)
    );
}
#[test]
fn touching_hole_and_resource_excess_are_explicitly_unsupported() {
    assert_eq!(
        polygon(
            square(),
            vec![ring(&[(0., 5.), (2., 4.), (2., 6.), (0., 5.)])]
        ),
        Err(PolygonError::TouchingRingsUnsupported)
    );
    assert_eq!(
        polygon(square(), vec![square(); 129]),
        Err(PolygonError::ResourceLimit)
    );
}
#[test]
fn tiny_polygon_is_normalized_for_validation_without_altering_source() {
    let input = ring(&[
        (0., 0.),
        (1e-200, 0.),
        (1e-200, 1e-200),
        (0., 1e-200),
        (0., 0.),
    ]);
    let result = polygon(input.clone(), vec![]).unwrap();
    assert_eq!(result.exterior(), input);
}

fn area_ring(ring: &[Point2]) -> f64 {
    ring.windows(2)
        .map(|points| points[0].x.get() * points[1].y.get() - points[1].x.get() * points[0].y.get())
        .sum::<f64>()
        .abs()
        / 2.0
}

fn area(result: &ValidatedMultiPolygon) -> f64 {
    result
        .polygons()
        .iter()
        .map(|polygon| {
            area_ring(polygon.exterior())
                - polygon
                    .holes()
                    .iter()
                    .map(|hole| area_ring(hole))
                    .sum::<f64>()
        })
        .sum()
}

fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<ValidatedPolygon, PolygonError> {
    polygon(
        ring(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]),
        vec![],
    )
}

#[test]
fn boolean_overlapping_squares_have_expected_geometry() {
    let left = rectangle(0., 0., 2., 2.).unwrap();
    let right = rectangle(1., 1., 3., 3.).unwrap();

    let union = left.union(&right).unwrap();
    assert_eq!(union.polygons().len(), 1);
    assert_eq!(union.polygons()[0].holes().len(), 0);
    assert_eq!(area(&union), 7.);
    assert!(union.polygons()[0].exterior().len() >= 8);

    let intersection = left.intersection(&right).unwrap();
    assert_eq!(intersection.polygons().len(), 1);
    assert_eq!(area(&intersection), 1.);
    assert_eq!(area_ring(intersection.polygons()[0].exterior()), 1.);

    let difference = left.difference(&right).unwrap();
    assert_eq!(difference.polygons().len(), 1);
    assert_eq!(area(&difference), 3.);
    assert_eq!(difference.polygons()[0].holes().len(), 0);
}

#[test]
fn boolean_difference_preserves_a_contained_hole() {
    let outer = rectangle(0., 0., 10., 10.).unwrap();
    let inner = rectangle(2., 2., 8., 8.).unwrap();
    let result = outer.difference(&inner).unwrap();

    assert_eq!(result.polygons().len(), 1);
    assert_eq!(result.polygons()[0].holes().len(), 1);
    assert_eq!(area(&result), 64.);
    assert_eq!(area_ring(result.polygons()[0].exterior()), 100.);
    assert_eq!(area_ring(&result.polygons()[0].holes()[0]), 36.);
}

#[test]
fn boolean_disjoint_and_empty_results_are_explicit() {
    let left = rectangle(0., 0., 1., 1.).unwrap();
    let right = rectangle(3., 0., 4., 1.).unwrap();

    let union = left.union(&right).unwrap();
    assert_eq!(union.polygons().len(), 2);
    assert_eq!(area(&union), 2.);
    assert!(union.polygons()[0].exterior()[0].x.get() < union.polygons()[1].exterior()[0].x.get());

    let intersection = left.intersection(&right).unwrap();
    assert!(intersection.is_empty());
    assert_eq!(area(&intersection), 0.);

    let difference = left.difference(&left).unwrap();
    assert!(difference.is_empty());

    let empty = ValidatedMultiPolygon::empty(left.floor_id(), left.frame_id());
    assert_eq!(
        empty.union(&left.as_multipolygon()).unwrap(),
        left.as_multipolygon()
    );
    assert!(
        empty
            .intersection(&left.as_multipolygon())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        left.as_multipolygon().difference(&empty).unwrap(),
        left.as_multipolygon()
    );
}

#[test]
fn boolean_boundary_contact_and_permutations_are_deterministic() {
    let left = rectangle(0., 0., 2., 2.).unwrap();
    let right = rectangle(2., 0., 4., 2.).unwrap();

    let union = left.union(&right).unwrap();
    assert_eq!(union.polygons().len(), 1);
    assert_eq!(area(&union), 8.);
    assert_eq!(union, right.union(&left).unwrap());
    assert!(left.intersection(&right).unwrap().is_empty());

    let reversed_right = polygon(
        ring(&[(2., 0.), (2., 2.), (4., 2.), (4., 0.), (2., 0.)]),
        vec![],
    )
    .unwrap();
    assert_eq!(union, left.union(&reversed_right).unwrap());
    assert_eq!(
        left.intersection(&right).unwrap(),
        right.intersection(&left).unwrap()
    );
}

#[test]
fn boolean_scope_and_resource_errors_are_explicit() {
    let left = rectangle(0., 0., 2., 2.).unwrap();
    let mismatched = ValidatedPolygon::new(
        FloorId::from_bytes([9; 16]).unwrap(),
        FrameId::from_bytes([2; 16]).unwrap(),
        ring(&[(0., 0.), (2., 0.), (2., 2.), (0., 2.), (0., 0.)]),
        vec![],
    )
    .unwrap();
    assert_eq!(left.union(&mismatched), Err(BooleanError::FloorMismatch));

    let frame_mismatched = ValidatedPolygon::new(
        FloorId::from_bytes([1; 16]).unwrap(),
        FrameId::from_bytes([9; 16]).unwrap(),
        ring(&[(0., 0.), (2., 0.), (2., 2.), (0., 2.), (0., 0.)]),
        vec![],
    )
    .unwrap();
    assert_eq!(
        left.union(&frame_mismatched),
        Err(BooleanError::FrameMismatch)
    );

    let many = (0..257)
        .map(|index| rectangle(index as f64 * 3., 0., index as f64 * 3. + 1., 1.).unwrap())
        .collect();
    assert_eq!(
        ValidatedMultiPolygon::new(
            FloorId::from_bytes([1; 16]).unwrap(),
            FrameId::from_bytes([2; 16]).unwrap(),
            many,
        ),
        Err(BooleanError::ResourceLimit)
    );

    let mut points = (0..2048)
        .map(|index| {
            let angle = index as f64 * std::f64::consts::TAU / 2048.;
            (angle.cos() * 10., angle.sin() * 10.)
        })
        .collect::<Vec<_>>();
    points.push(points[0]);
    let large = polygon(ring(&points), vec![]).unwrap();
    assert_eq!(large.union(&large), Err(BooleanError::ResourceLimit));
}

#[test]
fn boolean_tiny_coordinates_are_normalized_without_false_empty() {
    let tiny = rectangle(0., 0., 1e-200, 1e-200).unwrap();
    let result = tiny.intersection(&tiny).unwrap();
    assert_eq!(result.polygons().len(), 1);
    assert_eq!(result.polygons()[0].exterior().len(), 5);
    assert!(
        result.polygons()[0]
            .exterior()
            .iter()
            .all(|point| point.x.get().is_finite() && point.y.get().is_finite())
    );
    assert!(
        result.polygons()[0]
            .exterior()
            .iter()
            .any(|point| point.x.get() > 0.0 && point.y.get() > 0.0)
    );
}

#[test]
fn boolean_disjoint_mixed_scales_bypass_kernel_without_losing_components() {
    let tiny = rectangle(0., 0., 1e-8, 1e-8).unwrap();
    let large = rectangle(1., 1., 100., 100.).unwrap();

    let union = tiny.union(&large).unwrap();
    assert_eq!(union.polygons().len(), 2);
    assert_eq!(area(&union), 1e-16 + 9801.);
    assert!(union.polygons().iter().any(|polygon| {
        polygon
            .exterior()
            .iter()
            .all(|point| point.x.get() <= 1e-8 && point.y.get() <= 1e-8)
    }));
    assert_eq!(tiny.difference(&large).unwrap().polygons().len(), 1);
    assert!(tiny.intersection(&large).unwrap().is_empty());
}

#[test]
fn boolean_narrow_near_boundary_feature_is_explicitly_unsupported() {
    let large = rectangle(0., 0., 100., 100.).unwrap();
    let narrow = rectangle(99.99999999, 99.99999999, 100.00000001, 100.00000001).unwrap();

    assert_eq!(
        large.intersection(&narrow),
        Err(BooleanError::UnsupportedCoordinateResolution)
    );
    assert_eq!(
        large.difference(&narrow),
        Err(BooleanError::UnsupportedCoordinateResolution)
    );
}

#[test]
fn boolean_near_collinear_sliver_is_explicitly_unsupported() {
    let near_collinear = polygon(
        ring(&[(0., 0.), (1., 1.), (2., 2.000000000001), (0., 0.)]),
        vec![],
    )
    .unwrap();

    assert_eq!(
        near_collinear.intersection(&near_collinear),
        Err(BooleanError::UnsupportedCoordinateResolution)
    );
}
