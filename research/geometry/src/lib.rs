//! Minimal no-I/O exports used to behaviorally execute the portable Rust
//! geometry dependency in a real `wasm32-unknown-unknown` runtime.

use geo::algorithm::area::Area;
use geo::algorithm::bool_ops::BooleanOps;
use geo::algorithm::buffer::Buffer;
use geo::algorithm::line_intersection::{LineIntersection, line_intersection};
use geo::{Coord, Length, Line, LineString, MultiLineString, Polygon};

fn polygon_a() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0., 0.)]),
        vec![],
    )
}

fn polygon_b() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![(5., 5.), (15., 5.), (15., 15.), (5., 15.), (5., 5.)]),
        vec![],
    )
}

fn ground_polygon() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0., 0.)]),
        vec![LineString::from(vec![
            (3., 3.),
            (7., 3.),
            (7., 7.),
            (3., 7.),
            (3., 3.),
        ])],
    )
}

fn buffer_square() -> Polygon<f64> {
    Polygon::new(
        LineString::from(vec![(0., 0.), (4., 0.), (4., 4.), (0., 4.), (0., 0.)]),
        vec![],
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn geometry_union_area() -> f64 {
    polygon_a().union(&polygon_b()).unsigned_area()
}

#[unsafe(no_mangle)]
pub extern "C" fn geometry_intersection_area() -> f64 {
    polygon_a().intersection(&polygon_b()).unsigned_area()
}

#[unsafe(no_mangle)]
pub extern "C" fn geometry_difference_area() -> f64 {
    polygon_a().difference(&polygon_b()).unsigned_area()
}

#[unsafe(no_mangle)]
pub extern "C" fn geometry_buffer_area() -> f64 {
    buffer_square().buffer(1.0).unsigned_area()
}

#[unsafe(no_mangle)]
pub extern "C" fn geometry_hole_span_length() -> f64 {
    let span: LineString<f64> = vec![(-1., 5.), (11., 5.)].into();
    ground_polygon()
        .clip(&MultiLineString::new(vec![span]), false)
        .0
        .iter()
        .map(|line| geo::Euclidean.length(line))
        .sum()
}

#[unsafe(no_mangle)]
pub extern "C" fn geometry_crossing_x() -> f64 {
    let line = Line::new(Coord { x: 0., y: 5. }, Coord { x: 10., y: 5. });
    let wall = Line::new(Coord { x: 5., y: 0. }, Coord { x: 5., y: 10. });
    match line_intersection(line, wall) {
        Some(LineIntersection::SinglePoint { intersection, .. }) => intersection.x,
        _ => f64::NAN,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn geometry_semantics_version() -> u32 {
    1
}
