use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
    units::{CoordinateMeters, Meters, Radians},
};
use kyberia_geometry_adapter::{
    OffsetDirection, OffsetError, OffsetOptions, ValidatedMultiPolygon, ValidatedPolygon,
};

fn floor() -> FloorId {
    FloorId::from_bytes([1; 16]).unwrap()
}

fn frame() -> FrameId {
    FrameId::from_bytes([2; 16]).unwrap()
}

fn ring(points: &[(f64, f64)]) -> Vec<Point2> {
    points
        .iter()
        .map(|&(x, y)| Point2 {
            x: CoordinateMeters::new(x).unwrap(),
            y: CoordinateMeters::new(y).unwrap(),
        })
        .collect()
}

fn polygon(points: &[(f64, f64)], holes: Vec<Vec<Point2>>) -> ValidatedPolygon {
    ValidatedPolygon::new(floor(), frame(), ring(points), holes).unwrap()
}

fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> ValidatedPolygon {
    polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)], vec![])
}

fn options(direction: OffsetDirection, angle: f64) -> OffsetOptions {
    OffsetOptions::new(direction, Radians::new(angle).unwrap()).unwrap()
}

fn area_ring(ring: &[Point2]) -> f64 {
    let origin = ring[0];
    ring.windows(2)
        .map(|points| {
            let first_x = points[0].x.get() - origin.x.get();
            let first_y = points[0].y.get() - origin.y.get();
            let second_x = points[1].x.get() - origin.x.get();
            let second_y = points[1].y.get() - origin.y.get();
            first_x * second_y - second_x * first_y
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn area(multi: &ValidatedMultiPolygon) -> f64 {
    multi
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

fn bounds(multi: &ValidatedMultiPolygon) -> (f64, f64, f64, f64) {
    multi
        .polygons()
        .iter()
        .flat_map(|polygon| {
            polygon
                .exterior()
                .iter()
                .chain(polygon.holes().iter().flatten())
        })
        .fold(
            (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |(min_x, min_y, max_x, max_y), point| {
                (
                    min_x.min(point.x.get()),
                    min_y.min(point.y.get()),
                    max_x.max(point.x.get()),
                    max_y.max(point.y.get()),
                )
            },
        )
}

#[test]
fn square_outward_uses_rounded_corners_with_bounded_area_error() {
    let input = rectangle(0.0, 0.0, 10.0, 10.0).as_multipolygon();
    let result = input.offset(options(
        OffsetDirection::Outward(Meters::new(1.0).unwrap()),
        0.25,
    ));
    let result = result.unwrap();
    let (min_x, min_y, max_x, max_y) = bounds(&result);
    assert_eq!(result.polygons().len(), 1);
    assert!((min_x + 1.0).abs() < 1e-8);
    assert!((min_y + 1.0).abs() < 1e-8);
    assert!((max_x - 11.0).abs() < 1e-8);
    assert!((max_y - 11.0).abs() < 1e-8);
    assert!(area(&result) > 143.0 && area(&result) < 144.0);
}

#[test]
fn inward_offset_shrinks_and_can_be_empty() {
    let input = rectangle(0.0, 0.0, 10.0, 10.0).as_multipolygon();
    let shrunk = input
        .offset(options(
            OffsetDirection::Inward(Meters::new(1.0).unwrap()),
            0.25,
        ))
        .unwrap();
    let (min_x, min_y, max_x, max_y) = bounds(&shrunk);
    assert_eq!(shrunk.polygons().len(), 1);
    assert!((min_x - 1.0).abs() < 1e-8);
    assert!((min_y - 1.0).abs() < 1e-8);
    assert!((max_x - 9.0).abs() < 1e-8);
    assert!((max_y - 9.0).abs() < 1e-8);
    assert!(area(&shrunk) < 64.0 + 1e-8);
    let erased = input
        .offset(options(
            OffsetDirection::Inward(Meters::new(6.0).unwrap()),
            0.25,
        ))
        .unwrap();
    assert!(erased.is_empty());
}

#[test]
fn holes_grow_or_shrink_with_the_requested_direction() {
    let input = polygon(
        &[
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 20.0),
            (0.0, 20.0),
            (0.0, 0.0),
        ],
        vec![ring(&[
            (6.0, 6.0),
            (14.0, 6.0),
            (14.0, 14.0),
            (6.0, 14.0),
            (6.0, 6.0),
        ])],
    )
    .as_multipolygon();
    let outward = input
        .offset(options(
            OffsetDirection::Outward(Meters::new(1.0).unwrap()),
            0.25,
        ))
        .unwrap();
    let inward = input
        .offset(options(
            OffsetDirection::Inward(Meters::new(1.0).unwrap()),
            0.25,
        ))
        .unwrap();
    assert_eq!(outward.polygons().len(), 1);
    assert_eq!(inward.polygons().len(), 1);
    assert_eq!(outward.polygons()[0].holes().len(), 1);
    assert_eq!(inward.polygons()[0].holes().len(), 1);
    assert!(area(&outward) > area(&input));
    assert!(area(&inward) < area(&input));
    let outward_hole = bounds(
        &ValidatedMultiPolygon::new(
            floor(),
            frame(),
            vec![
                ValidatedPolygon::new(
                    floor(),
                    frame(),
                    outward.polygons()[0].holes()[0].clone(),
                    vec![],
                )
                .unwrap(),
            ],
        )
        .unwrap(),
    );
    let inward_hole = bounds(
        &ValidatedMultiPolygon::new(
            floor(),
            frame(),
            vec![
                ValidatedPolygon::new(
                    floor(),
                    frame(),
                    inward.polygons()[0].holes()[0].clone(),
                    vec![],
                )
                .unwrap(),
            ],
        )
        .unwrap(),
    );
    assert!(outward_hole.2 - outward_hole.0 < 8.0);
    assert!(inward_hole.2 - inward_hole.0 > 8.0);
}

#[test]
fn disjoint_components_and_concave_inward_split_are_preserved() {
    let input = ValidatedMultiPolygon::new(
        floor(),
        frame(),
        vec![
            rectangle(0.0, 0.0, 10.0, 10.0),
            rectangle(100.0, 100.0, 110.0, 110.0),
        ],
    )
    .unwrap();
    let result = input.offset(options(
        OffsetDirection::Outward(Meters::new(1.0).unwrap()),
        0.25,
    ));
    let result = result.unwrap();
    assert_eq!(result.polygons().len(), 2);

    let mixed_scale = ValidatedMultiPolygon::new(
        floor(),
        frame(),
        vec![
            rectangle(0.0, 0.0, 0.01, 0.01),
            rectangle(1.0, 1.0, 100.0, 100.0),
        ],
    )
    .unwrap();
    let mixed_result = mixed_scale
        .offset(options(
            OffsetDirection::Outward(Meters::new(0.001).unwrap()),
            0.25,
        ))
        .unwrap();
    assert_eq!(mixed_result.polygons().len(), 2);
    assert!(mixed_result.polygons().iter().any(|polygon| {
        polygon.exterior().iter().any(|point| point.x.get() < 0.0)
            && polygon.exterior().iter().any(|point| point.x.get() > 0.01)
    }));

    let concave = polygon(
        &[
            (0.0, 0.0),
            (12.0, 0.0),
            (12.0, 12.0),
            (8.0, 12.0),
            (8.0, 4.0),
            (4.0, 4.0),
            (4.0, 12.0),
            (0.0, 12.0),
            (0.0, 0.0),
        ],
        vec![],
    )
    .as_multipolygon();
    let split = concave
        .offset(options(
            OffsetDirection::Inward(Meters::new(2.0).unwrap()),
            0.25,
        ))
        .unwrap();
    assert_eq!(split.polygons().len(), 2);
    assert!(
        split
            .polygons()
            .iter()
            .all(|polygon| area_ring(polygon.exterior()) > 0.0)
    );
    assert!(area(&split) < area(&concave));
}

#[test]
fn mixed_scale_partial_component_coverage_is_rejected() {
    // A large component and a component several orders of magnitude smaller can
    // survive normalization but still be partially lost by the offset kernel.
    // The completeness guard must reject that result instead of accepting a
    // merely intersecting fragment of the small component.
    let input = ValidatedMultiPolygon::new(
        floor(),
        frame(),
        vec![
            rectangle(-1000.0, -1000.0, 0.0, 0.0),
            rectangle(2e-6, 2e-6, 2.1e-6, 2.1e-6),
        ],
    )
    .expect("the fixture components are valid and disjoint");

    let result = input.offset(options(
        OffsetDirection::Outward(Meters::new(1e-5).unwrap()),
        0.25,
    ));

    assert_eq!(result, Err(OffsetError::UnsupportedCoordinateResolution));
}

#[test]
fn offset_is_deterministic_for_operand_and_ring_order() {
    let forward = polygon(
        &[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ],
        vec![],
    );
    let reverse = polygon(
        &[
            (0.0, 0.0),
            (0.0, 10.0),
            (10.0, 10.0),
            (10.0, 0.0),
            (0.0, 0.0),
        ],
        vec![],
    );
    let options = options(OffsetDirection::Outward(Meters::new(1.0).unwrap()), 0.25);
    assert_eq!(
        forward.as_multipolygon().offset(options).unwrap(),
        reverse.as_multipolygon().offset(options).unwrap()
    );
    let a = ValidatedMultiPolygon::new(
        floor(),
        frame(),
        vec![rectangle(100.0, 100.0, 110.0, 110.0), forward],
    )
    .unwrap();
    let b = ValidatedMultiPolygon::new(
        floor(),
        frame(),
        vec![reverse, rectangle(100.0, 100.0, 110.0, 110.0)],
    )
    .unwrap();
    let a_result = a.offset(options);
    let b_result = b.offset(options);
    assert_eq!(a_result.unwrap(), b_result.unwrap());
}

#[test]
fn translated_and_tiny_geometry_are_not_rejected_by_absolute_origin() {
    let translated =
        rectangle(999_999_900.0, 999_999_900.0, 999_999_910.0, 999_999_910.0).as_multipolygon();
    let result = translated
        .offset(options(
            OffsetDirection::Outward(Meters::new(1.0).unwrap()),
            0.25,
        ))
        .unwrap();
    assert_eq!(result.polygons().len(), 1);
    assert!((bounds(&result).0 - 999_999_899.0).abs() < 1e-5);

    let tiny = rectangle(0.0, 0.0, 1e-200, 1e-200).as_multipolygon();
    let tiny_result = tiny
        .offset(options(
            OffsetDirection::Outward(Meters::new(1e-201).unwrap()),
            0.25,
        ))
        .unwrap();
    assert_eq!(tiny_result.polygons().len(), 1);
    assert!(
        tiny_result.polygons()[0]
            .exterior()
            .iter()
            .all(|point| point.x.get().is_finite() && point.y.get().is_finite())
    );
}

#[test]
fn options_resolution_work_and_cancellation_fail_closed() {
    let input = rectangle(0.0, 0.0, 10.0, 10.0).as_multipolygon();
    assert_eq!(
        OffsetOptions::new(
            OffsetDirection::Outward(Meters::new(1.0).unwrap()),
            Radians::new(0.024).unwrap()
        ),
        Err(OffsetError::InvalidOptions)
    );
    assert_eq!(
        OffsetOptions::new(
            OffsetDirection::Outward(Meters::new(1.0).unwrap()),
            Radians::new(std::f64::consts::FRAC_PI_2 + 0.01).unwrap()
        ),
        Err(OffsetError::InvalidOptions)
    );
    assert_eq!(
        OffsetOptions::new(
            OffsetDirection::Outward(Meters::new(1_000_000_001.0).unwrap()),
            Radians::new(0.25).unwrap()
        ),
        Err(OffsetError::InvalidOptions)
    );
    let translated =
        rectangle(999_999_900.0, 999_999_900.0, 999_999_910.0, 999_999_910.0).as_multipolygon();
    assert_eq!(
        translated.offset(options(
            OffsetDirection::Outward(Meters::new(1e-8).unwrap()),
            0.25
        )),
        Err(OffsetError::UnsupportedCoordinateResolution)
    );

    let many = (0..700)
        .map(|index| {
            let angle = index as f64 * std::f64::consts::TAU / 700.0;
            (angle.cos() * 10.0, angle.sin() * 10.0)
        })
        .collect::<Vec<_>>();
    let mut closed = many;
    closed.push(closed[0]);
    let many = polygon(&closed.to_vec(), vec![]);
    assert_eq!(
        many.as_multipolygon().offset(options(
            OffsetDirection::Outward(Meters::new(1.0).unwrap()),
            0.25
        )),
        Err(OffsetError::ResourceLimit)
    );

    let calls = std::cell::Cell::new(0);
    let cancelled = input.offset_with_cancellation(
        options(OffsetDirection::Outward(Meters::new(1.0).unwrap()), 0.25),
        || {
            let count = calls.get();
            calls.set(count + 1);
            count >= 1
        },
    );
    assert_eq!(cancelled, Err(OffsetError::Cancelled));

    let calls = std::cell::Cell::new(0);
    let cancelled_after_kernel = input.offset_with_cancellation(
        options(OffsetDirection::Outward(Meters::new(1.0).unwrap()), 0.25),
        || {
            let count = calls.get();
            calls.set(count + 1);
            count >= 3
        },
    );
    assert_eq!(cancelled_after_kernel, Err(OffsetError::Cancelled));

    let calls = std::cell::Cell::new(0);
    let cancelled_noop = input.offset_with_cancellation(
        options(OffsetDirection::Outward(Meters::new(0.0).unwrap()), 0.25),
        || {
            let count = calls.get();
            calls.set(count + 1);
            count >= 1
        },
    );
    assert_eq!(cancelled_noop, Err(OffsetError::Cancelled));
}
