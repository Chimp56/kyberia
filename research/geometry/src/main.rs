mod import;

use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::time::Instant;

use geo::algorithm::area::Area;
use geo::algorithm::bool_ops::BooleanOps;
use geo::algorithm::buffer::Buffer;
use geo::algorithm::intersects::Intersects;
use geo::algorithm::line_intersection::{LineIntersection, line_intersection};
use geo::algorithm::validation::Validation;
use geo::{Coord, Length, Line, LineString, MultiLineString, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, GeometryValue};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const FIXTURE: &[u8] = include_bytes!("../fixtures/geometry-proof.json");
const GEOJSON_FIXTURE: &[u8] = include_bytes!("../fixtures/geometry-proof-input.geojson");
const CARGO_TOML: &[u8] = include_bytes!("../Cargo.toml");
const CARGO_LOCK: &[u8] = include_bytes!("../Cargo.lock");
const MAIN_SOURCE: &[u8] = include_bytes!("main.rs");
const LIB_SOURCE: &[u8] = include_bytes!("lib.rs");
const IMPORT_SOURCE: &[u8] = include_bytes!("import.rs");
const SHAPELY_SOURCE: &[u8] = include_bytes!("../shapely_proof.py");
const BENCHMARK_SOURCE: &[u8] = include_bytes!("../benchmark.py");
const WASM_BEHAVIOR_SOURCE: &[u8] = include_bytes!("../wasm_behavior.js");
const PYTHON_REQUIREMENTS: &[u8] = include_bytes!("../shapely-requirements.txt");

fn sha256(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn source_hashes() -> Value {
    json!({
        "research/geometry/fixtures/geometry-proof.json": sha256(FIXTURE),
        "research/geometry/fixtures/geometry-proof-input.geojson": sha256(GEOJSON_FIXTURE),
        "research/geometry/Cargo.toml": sha256(CARGO_TOML),
        "research/geometry/Cargo.lock": sha256(CARGO_LOCK),
        "research/geometry/src/main.rs": sha256(MAIN_SOURCE),
        "research/geometry/src/lib.rs": sha256(LIB_SOURCE),
        "research/geometry/src/import.rs": sha256(IMPORT_SOURCE),
        "research/geometry/shapely_proof.py": sha256(SHAPELY_SOURCE),
        "research/geometry/benchmark.py": sha256(BENCHMARK_SOURCE),
        "research/geometry/wasm_behavior.js": sha256(WASM_BEHAVIOR_SOURCE),
        "research/geometry/shapely-requirements.txt": sha256(PYTHON_REQUIREMENTS)
    })
}

fn source_revision() -> String {
    env::var("KYBERIA_GEOMETRY_SOURCE_REVISION")
        .unwrap_or_else(|_| "UNCOMMITTED_WORKTREE_CONTENT_HASH_BOUND".to_string())
}

fn coord(pair: &[f64; 2]) -> Coord<f64> {
    Coord {
        x: pair[0],
        y: pair[1],
    }
}

fn line(points: &[[f64; 2]; 2]) -> Line<f64> {
    Line::new(coord(&points[0]), coord(&points[1]))
}

fn point_from_fixture(value: &Value) -> [f64; 2] {
    let coordinates = value.as_array().expect("fixture point array");
    assert_eq!(coordinates.len(), 2, "fixture probe points must be 2-D");
    [
        coordinates[0].as_f64().expect("fixture X coordinate"),
        coordinates[1].as_f64().expect("fixture Y coordinate"),
    ]
}

fn line_from_fixture(fixture: &Value, key: &str) -> Line<f64> {
    let points = fixture["lines"][key]
        .as_array()
        .unwrap_or_else(|| panic!("fixture line {key}"));
    assert_eq!(
        points.len(),
        2,
        "fixture line {key} must contain two points"
    );
    let endpoints = [
        point_from_fixture(&points[0]),
        point_from_fixture(&points[1]),
    ];
    line(&endpoints)
}

fn linestring_from_fixture(fixture: &Value, key: &str) -> LineString<f64> {
    fixture["lines"][key]
        .as_array()
        .unwrap_or_else(|| panic!("fixture line string {key}"))
        .iter()
        .map(point_from_fixture)
        .collect::<Vec<_>>()
        .into()
}

fn parse_wkt_polygon(text: &str) -> Polygon<f64> {
    let parsed: wkt::Wkt<f64> = text.parse().expect("fixture WKT must parse");
    let geometry: geo::Geometry<f64> = parsed.try_into().expect("WKT geometry conversion");
    match geometry {
        geo::Geometry::Polygon(value) => value,
        other => panic!("expected polygon from WKT, got {other:?}"),
    }
}

fn point_pair(value: &Coord<f64>) -> [f64; 2] {
    [value.x, value.y]
}

fn canonical_segment(line: &Line<f64>) -> [[f64; 2]; 2] {
    let first = [line.start.x, line.start.y];
    let second = [line.end.x, line.end.y];
    if first <= second {
        [first, second]
    } else {
        [second, first]
    }
}

fn floor_name(feature: &Feature) -> Option<&str> {
    feature
        .properties
        .as_ref()
        .and_then(|properties| properties.get("floor"))
        .and_then(Value::as_str)
}

fn geometry_from_feature(feature: &Feature) -> geo::Geometry<f64> {
    feature
        .geometry
        .as_ref()
        .expect("bounded import rejects null feature geometry")
        .clone()
        .try_into()
        .expect("bounded GeoJSON conversion")
}

fn select_floor_geometry(
    collection: &FeatureCollection,
    floor: &str,
    geometry_kind: &str,
) -> geo::Geometry<f64> {
    collection
        .features
        .iter()
        .filter(|feature| floor_name(feature) == Some(floor))
        .find_map(|feature| {
            let geometry = feature.geometry.as_ref()?;
            let matches_kind = matches!(
                (&geometry.value, geometry_kind),
                (GeometryValue::Polygon { .. }, "Polygon")
                    | (GeometryValue::LineString { .. }, "LineString")
            );
            matches_kind.then(|| geometry_from_feature(feature))
        })
        .unwrap_or_else(|| panic!("no {geometry_kind} geometry on floor {floor}"))
}

fn ensure_valid_polygon(polygon: &Polygon<f64>, name: &str) {
    assert!(
        polygon.is_valid(),
        "{name} must be validated before operations: {:?}",
        polygon.validation_errors()
    );
}

fn write_json(path: &str, value: &Value) {
    let parent = Path::new(path).parent().expect("result path parent");
    fs::create_dir_all(parent).expect("result directory");
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize result"),
    )
    .expect("write result");
}

fn run_bounded_import(input: &str, output: &str) -> Result<(), String> {
    let parsed = import::read_bounded_file(Path::new(input))?;
    let result = json!({
        "status": "PASS",
        "format": "GeoJSON",
        "source_path": input,
        "source_sha256": sha256(&parsed.source_bytes),
        "source_revision": source_revision(),
        "limits": {
            "max_input_bytes": import::MAX_INPUT_BYTES,
            "max_coordinates": import::MAX_COORDINATES,
            "max_features": import::MAX_FEATURES,
            "max_geometries": import::MAX_GEOMETRIES,
            "max_depth": import::MAX_DEPTH
        },
        "observed": {"coordinates": parsed.coordinate_count, "geometries": parsed.geometry_count},
        "source_hashes": source_hashes(),
        "document_type": match parsed.document { GeoJson::FeatureCollection(_) => "FeatureCollection", GeoJson::Feature(_) => "Feature", GeoJson::Geometry(_) => "Geometry" }
    });
    write_json(output, &result);
    Ok(())
}

fn run_comparison(output: &str) {
    let start = Instant::now();
    let fixture: Value = serde_json::from_slice(FIXTURE).expect("fixture JSON must parse");
    let imported = import::parse_bounded_bytes(GEOJSON_FIXTURE)
        .expect("fixture GeoJSON must satisfy bounded import");
    let geojson = match imported.document {
        GeoJson::FeatureCollection(collection) => collection,
        other => panic!("expected feature collection, got {other:?}"),
    };
    let ground = match select_floor_geometry(&geojson, "ground", "Polygon") {
        geo::Geometry::Polygon(value) => value,
        _ => unreachable!(),
    };
    let wall = match select_floor_geometry(&geojson, "ground", "LineString") {
        geo::Geometry::LineString(value) => value,
        _ => unreachable!(),
    };
    let upper_wall = select_floor_geometry(&geojson, "upper", "LineString");
    let ground_wall_count = geojson
        .features
        .iter()
        .filter(|feature| floor_name(feature) == Some("ground"))
        .filter(|feature| {
            feature
                .geometry
                .as_ref()
                .map(|geometry| matches!(geometry.value, GeometryValue::LineString { .. }))
                .unwrap_or(false)
        })
        .count();
    let upper_wall_count = geojson
        .features
        .iter()
        .filter(|feature| floor_name(feature) == Some("upper"))
        .filter(|feature| {
            feature
                .geometry
                .as_ref()
                .map(|geometry| matches!(geometry.value, GeometryValue::LineString { .. }))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(ground_wall_count, 1);
    assert_eq!(upper_wall_count, 1);
    assert!(matches!(upper_wall, geo::Geometry::LineString(_)));
    let upper_same_xy = upper_wall == geo::Geometry::LineString(wall.clone());
    assert!(
        upper_same_xy,
        "upper wall probe must be same XY as selected ground wall"
    );

    let a = parse_wkt_polygon(fixture["wkt"]["polygon_a"].as_str().unwrap());
    let b = parse_wkt_polygon(fixture["wkt"]["polygon_b"].as_str().unwrap());
    let square = parse_wkt_polygon(fixture["wkt"]["buffer_square"].as_str().unwrap());
    let invalid_text = fixture["wkt"]["invalid_bowtie"].as_str().unwrap();
    let invalid = parse_wkt_polygon(invalid_text);
    ensure_valid_polygon(&ground, "selected ground polygon");
    ensure_valid_polygon(&a, "polygon A");
    ensure_valid_polygon(&b, "polygon B");
    ensure_valid_polygon(&square, "buffer square");
    let invalid_errors = invalid.validation_errors();
    assert!(
        !invalid.is_valid(),
        "invalid fixture must be rejected before operations"
    );
    let buffer = square.buffer(1.0);

    let crossing = line_from_fixture(&fixture, "crossing");
    let touching = line_from_fixture(&fixture, "touch");
    let touch_wall = line_from_fixture(&fixture, "touch_wall");
    let collinear = line_from_fixture(&fixture, "collinear");
    let collinear_overlap = line_from_fixture(&fixture, "collinear_overlap");
    let wall_segments = wall.lines().count();
    let crossing_results = wall
        .lines()
        .filter_map(|segment| line_intersection(crossing, segment))
        .collect::<Vec<_>>();
    assert_eq!(
        crossing_results.len(),
        1,
        "every wall segment must be processed"
    );
    let crossing_result = crossing_results.into_iter().next().unwrap();
    let touch_result = line_intersection(touching, touch_wall).expect("touch intersection");
    let collinear_result =
        line_intersection(collinear, collinear_overlap).expect("collinear intersection");
    let hole_span = linestring_from_fixture(&fixture, "hole_span");
    let clipped = ground.clip(&MultiLineString::new(vec![hole_span]), false);
    let source_hash = sha256(invalid_text.as_bytes());
    let result = json!({
        "harness": "rust-geo-geojson",
        "library": {"geo": "0.33.1", "geojson": "1.0.0", "wkt": "0.14.0"},
        "coordinate_semantics": "2-D planar floor-local metres; z/floor is application metadata",
        "source_revision": source_revision(),
        "source_hashes": source_hashes(),
        "fixture_sha256": sha256(FIXTURE),
        "geojson_fixture_sha256": sha256(GEOJSON_FIXTURE),
        "bounds": {"max_input_bytes": import::MAX_INPUT_BYTES, "max_coordinates": import::MAX_COORDINATES, "observed_coordinates": imported.coordinate_count, "observed_geometries": imported.geometry_count},
        "import": {"format": "GeoJSON FeatureCollection + WKT", "feature_count": geojson.features.len(), "geometry_count": imported.geometry_count, "wkt_polygon_count": 4},
        "operations": {
            "shell_hole_area": ground.unsigned_area(),
            "polygon_a_area": a.unsigned_area(),
            "polygon_b_area": b.unsigned_area(),
            "intersection_area": a.intersection(&b).unsigned_area(),
            "union_area": a.union(&b).unsigned_area(),
            "difference_area": a.difference(&b).unsigned_area(),
            "buffer_square_area": buffer.unsigned_area(),
            "crossing": match crossing_result { LineIntersection::SinglePoint { intersection, .. } => json!({"kind":"point", "xy":point_pair(&intersection)}), LineIntersection::Collinear { intersection } => json!({"kind":"collinear", "segment":canonical_segment(&intersection)}), },
            "touch": match touch_result { LineIntersection::SinglePoint { intersection, .. } => json!({"kind":"point", "xy":point_pair(&intersection)}), LineIntersection::Collinear { intersection } => json!({"kind":"collinear", "segment":canonical_segment(&intersection)}), },
            "collinear": match collinear_result { LineIntersection::SinglePoint { intersection, .. } => json!({"kind":"point", "xy":point_pair(&intersection)}), LineIntersection::Collinear { intersection } => json!({"kind":"collinear", "segment":canonical_segment(&intersection)}), },
            "hole_span_inside_length": clipped.0.iter().map(|item| geo::Euclidean.length(item)).sum::<f64>(),
            "same_floor_wall_intersects": ground.intersects(&wall),
            "upper_floor_same_xy_is_separate_metadata": upper_same_xy,
            "floor_filter": {"selected_floor": "ground", "selected_wall_features": ground_wall_count, "upper_wall_features": upper_wall_count, "wall_segments_processed": wall_segments, "upper_excluded_before_2d_operation": true},
            "probe_inputs": {"crossing": fixture["lines"]["crossing"], "touch": fixture["lines"]["touch"], "touch_wall": fixture["lines"]["touch_wall"], "collinear": fixture["lines"]["collinear"], "collinear_overlap": fixture["lines"]["collinear_overlap"], "hole_span": fixture["lines"]["hole_span"]}
        },
        "invalid_geometry": {
            "is_valid": false,
            "diagnostic_count": invalid_errors.len(),
            "diagnostics": invalid_errors.iter().map(|error| format!("{error:?}")).collect::<Vec<_>>(),
            "source": {"format": "WKT", "sha256": source_hash, "text": invalid_text},
            "repair": {"status": "NOT_PERFORMED", "method": "explicit reject", "artifact": Value::Null, "tool": "geo", "version": "0.33.1"}
        },
        "determinism": {"repeated_area": a.union(&b).unsigned_area(), "repeated_buffer_area": square.buffer(1.0).unsigned_area()},
        "timing_boundary": "from before embedded fixture decode and bounded import through operations, before result serialization",
        "elapsed_ms": start.elapsed().as_secs_f64() * 1000.0
    });
    write_json(output, &result);
}

fn main() {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("import") => {
            let input = arguments.next().unwrap_or_else(|| {
                eprintln!("usage: geometry-proof import <input.geojson> <output.json>");
                process::exit(2);
            });
            let output = arguments.next().unwrap_or_else(|| {
                eprintln!("usage: geometry-proof import <input.geojson> <output.json>");
                process::exit(2);
            });
            if let Err(error) = run_bounded_import(&input, &output) {
                eprintln!("bounded GeoJSON import rejected: {error}");
                process::exit(2);
            }
        }
        Some(output) => run_comparison(output),
        None => run_comparison("research/geometry/results/rust-desktop.json"),
    }
}
