use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
    units::CoordinateMeters,
};
use kyberia_geometry_adapter::{PolygonError, ValidatedPolygon};
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
