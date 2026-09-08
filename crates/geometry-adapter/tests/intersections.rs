use kyberia_domain::{
    identity::{FloorId, FrameId},
    spatial::Point2,
    units::CoordinateMeters,
};
use kyberia_geometry_adapter::{GeometryError, Intersection, Segment, intersect};

fn point(x: f64, y: f64) -> Point2 {
    Point2 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
    }
}
fn segment(a: (f64, f64), b: (f64, f64)) -> Segment {
    Segment {
        floor_id: FloorId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([2; 16]).unwrap(),
        start: point(a.0, a.1),
        end: point(b.0, b.1),
    }
}
fn reverse(s: Segment) -> Segment {
    Segment {
        start: s.end,
        end: s.start,
        ..s
    }
}

#[test]
fn analytic_crossing_touch_overlap_and_disjoint() {
    let horizontal = segment((0., 2.), (8., 2.));
    assert_eq!(
        intersect(horizontal, segment((3., -1.), (3., 5.))).unwrap(),
        Intersection::Point(point(3., 2.))
    );
    assert_eq!(
        intersect(horizontal, segment((8., 2.), (9., 3.))).unwrap(),
        Intersection::Point(point(8., 2.))
    );
    assert_eq!(
        intersect(horizontal, segment((6., 2.), (2., 2.))).unwrap(),
        Intersection::Overlap {
            start: point(2., 2.),
            end: point(6., 2.)
        }
    );
    assert_eq!(
        intersect(horizontal, segment((0., 3.), (8., 3.))).unwrap(),
        Intersection::Disjoint
    );
}

#[test]
fn swapping_and_reversing_are_exactly_deterministic() {
    for i in 1..100 {
        let x = f64::from(i) / 7.;
        let a = segment((0., 0.), (x, 10.));
        let b = segment((0., 10.), (x, 0.));
        let expected = intersect(a, b).unwrap();
        assert_eq!(expected, Intersection::Point(point(x / 2., 5.)));
        for (a, b) in [
            (b, a),
            (reverse(a), b),
            (a, reverse(b)),
            (reverse(b), reverse(a)),
        ] {
            assert_eq!(intersect(a, b).unwrap(), expected);
        }
    }
}

#[test]
fn scope_degeneracy_and_numerical_range_fail_explicitly() {
    let a = segment((0., 0.), (1., 1.));
    assert_eq!(
        intersect(
            a,
            Segment {
                floor_id: FloorId::from_bytes([3; 16]).unwrap(),
                ..a
            }
        ),
        Err(GeometryError::FloorMismatch)
    );
    assert_eq!(
        intersect(
            a,
            Segment {
                frame_id: FrameId::from_bytes([3; 16]).unwrap(),
                ..a
            }
        ),
        Err(GeometryError::FrameMismatch)
    );
    assert_eq!(
        intersect(a, segment((1., 1.), (1., 1.))),
        Err(GeometryError::DegenerateSegment)
    );
    assert_eq!(
        intersect(a, segment((1e100, 0.), (1., 1.))),
        Err(GeometryError::CoordinateOutOfBounds)
    );
}

#[test]
fn tiny_crossings_are_not_misclassified_as_collinear() {
    for scale in [1e-150, 1e-200, 1e-300] {
        let a = segment((0., 0.), (scale, scale));
        let b = segment((0., scale), (scale, 0.));
        assert_eq!(
            intersect(a, b).unwrap(),
            Intersection::Point(point(scale / 2., scale / 2.))
        );
        assert_eq!(intersect(b, reverse(a)).unwrap(), intersect(a, b).unwrap());
    }
    let scale = f64::from_bits(1);
    assert_eq!(
        intersect(
            segment((0., 0.), (scale, scale)),
            segment((0., scale), (scale, 0.))
        ),
        Err(GeometryError::UnsupportedCoordinateResolution)
    );
    assert_eq!(
        intersect(segment((0., 0.), (1., 1.)), segment((0., 1e-200), (1., 0.))),
        Err(GeometryError::UnsupportedCoordinateResolution)
    );
}
