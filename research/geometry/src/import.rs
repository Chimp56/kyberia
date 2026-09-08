//! Bounded, strict GeoJSON import used only by the Gate E research harness.
//!
//! GeoJSON is parsed into the upstream representation, then checked before
//! any conversion to computational geometry. The checker deliberately accepts
//! only finite 2-D positions and rejects CRS/Z dimensions, null feature
//! geometries, empty geometry collections, malformed ring/line cardinality,
//! excessive nesting, and oversized collections.

use geojson::{Feature, GeoJson, Geometry, GeometryValue, Position};
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const MAX_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_COORDINATES: usize = 10_000;
pub const MAX_FEATURES: usize = 1_024;
pub const MAX_GEOMETRIES: usize = 4_096;
pub const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug)]
pub struct BoundedGeoJson {
    pub document: GeoJson,
    pub coordinate_count: usize,
    pub geometry_count: usize,
    /// The exact bounded bytes that were parsed; callers must hash these bytes
    /// rather than reading the source path a second time.
    pub source_bytes: Vec<u8>,
}

pub fn read_bounded_file(path: &Path) -> Result<BoundedGeoJson, String> {
    let mut file = File::open(path).map_err(|error| format!("cannot open GeoJSON: {error}"))?;
    let mut bytes = Vec::with_capacity(MAX_INPUT_BYTES);
    let mut chunk = [0_u8; 8192];
    while bytes.len() < MAX_INPUT_BYTES {
        let remaining = MAX_INPUT_BYTES - bytes.len();
        let read_size = remaining.min(chunk.len());
        let count = file
            .read(&mut chunk[..read_size])
            .map_err(|error| format!("cannot read GeoJSON: {error}"))?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.len() == MAX_INPUT_BYTES {
        let mut sentinel = [0_u8; 1];
        let count = file
            .read(&mut sentinel)
            .map_err(|error| format!("cannot read GeoJSON size sentinel: {error}"))?;
        if count != 0 {
            return Err(format!("GeoJSON exceeds {MAX_INPUT_BYTES} byte limit"));
        }
    }
    parse_bounded_bytes(&bytes)
}

pub fn parse_bounded_bytes(bytes: &[u8]) -> Result<BoundedGeoJson, String> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(format!("GeoJSON exceeds {MAX_INPUT_BYTES} byte limit"));
    }
    check_json_nesting(bytes)?;
    let raw: Value =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid JSON: {error}"))?;
    reject_crs_members(&raw)?;
    let document: GeoJson =
        serde_json::from_value(raw).map_err(|error| format!("invalid GeoJSON: {error}"))?;
    let mut state = WalkState::default();
    match &document {
        GeoJson::FeatureCollection(collection) => {
            if collection.features.is_empty() {
                return Err("empty FeatureCollection is unsupported".to_string());
            }
            validate_bbox(collection.bbox.as_ref())?;
            if collection.features.len() > MAX_FEATURES {
                return Err(format!(
                    "FeatureCollection exceeds {MAX_FEATURES} feature limit"
                ));
            }
            for feature in &collection.features {
                walk_feature(feature, 0, &mut state)?;
            }
        }
        GeoJson::Feature(feature) => walk_feature(feature, 0, &mut state)?,
        GeoJson::Geometry(geometry) => walk_geometry(geometry, 0, &mut state)?,
    }
    Ok(BoundedGeoJson {
        document,
        coordinate_count: state.coordinate_count,
        geometry_count: state.geometry_count,
        source_bytes: bytes.to_vec(),
    })
}

#[derive(Default)]
struct WalkState {
    coordinate_count: usize,
    geometry_count: usize,
}

fn walk_feature(feature: &Feature, depth: usize, state: &mut WalkState) -> Result<(), String> {
    validate_bbox(feature.bbox.as_ref())?;
    let geometry = feature
        .geometry
        .as_ref()
        .ok_or_else(|| "null Feature.geometry is unsupported".to_string())?;
    walk_geometry(geometry, depth, state)
}

fn walk_geometry(geometry: &Geometry, depth: usize, state: &mut WalkState) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "GeoJSON geometry nesting exceeds {MAX_DEPTH} levels"
        ));
    }
    validate_bbox(geometry.bbox.as_ref())?;
    state.geometry_count += 1;
    if state.geometry_count > MAX_GEOMETRIES {
        return Err(format!("GeoJSON exceeds {MAX_GEOMETRIES} geometry limit"));
    }
    match &geometry.value {
        GeometryValue::Point { coordinates } => walk_position(coordinates, state),
        GeometryValue::MultiPoint { coordinates } => {
            if coordinates.is_empty() {
                return Err("empty MultiPoint is unsupported".to_string());
            }
            for position in coordinates {
                walk_position(position, state)?;
            }
            Ok(())
        }
        GeometryValue::LineString { coordinates } => walk_line(coordinates, state),
        GeometryValue::MultiLineString { coordinates } => {
            if coordinates.is_empty() {
                return Err("empty MultiLineString is unsupported".to_string());
            }
            for line in coordinates {
                walk_line(line, state)?;
            }
            Ok(())
        }
        GeometryValue::Polygon { coordinates } => walk_polygon(coordinates, state),
        GeometryValue::MultiPolygon { coordinates } => {
            if coordinates.is_empty() {
                return Err("empty MultiPolygon is unsupported".to_string());
            }
            for polygon in coordinates {
                walk_polygon(polygon, state)?;
            }
            Ok(())
        }
        GeometryValue::GeometryCollection { geometries } => {
            if geometries.is_empty() {
                return Err("empty GeometryCollection is unsupported".to_string());
            }
            for child in geometries {
                walk_geometry(child, depth + 1, state)?;
            }
            Ok(())
        }
    }
}

fn walk_position(position: &Position, state: &mut WalkState) -> Result<(), String> {
    if position.len() != 2 {
        return Err(
            "only finite 2-D GeoJSON positions are supported; Z/M coordinates are rejected"
                .to_string(),
        );
    }
    if position.as_slice().iter().any(|value| !value.is_finite()) {
        return Err("non-finite GeoJSON coordinate is rejected".to_string());
    }
    state.coordinate_count += 1;
    if state.coordinate_count > MAX_COORDINATES {
        return Err(format!(
            "GeoJSON exceeds {MAX_COORDINATES} coordinate limit"
        ));
    }
    Ok(())
}

fn walk_line(line: &[Position], state: &mut WalkState) -> Result<(), String> {
    if line.len() < 2 {
        return Err("LineString must contain at least two positions".to_string());
    }
    for position in line {
        walk_position(position, state)?;
    }
    Ok(())
}

fn walk_polygon(polygon: &[Vec<Position>], state: &mut WalkState) -> Result<(), String> {
    if polygon.is_empty() {
        return Err("Polygon must contain an exterior ring".to_string());
    }
    for ring in polygon {
        if ring.len() < 4 {
            return Err("Polygon rings must contain at least four positions".to_string());
        }
        if ring.first() != ring.last() {
            return Err("Polygon rings must be closed".to_string());
        }
        for position in ring {
            walk_position(position, state)?;
        }
    }
    Ok(())
}

fn validate_bbox(bbox: Option<&Vec<f64>>) -> Result<(), String> {
    if let Some(bbox) = bbox
        && (bbox.len() != 4 || bbox.iter().any(|value| !value.is_finite()))
    {
        return Err("only finite 2-D four-value bounding boxes are supported".to_string());
    }
    Ok(())
}

fn reject_crs_members(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if map.contains_key("crs") {
                return Err("deprecated/unsupported GeoJSON CRS member is rejected; use an explicit project CRS".to_string());
            }
            for child in map.values() {
                reject_crs_members(child)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                reject_crs_members(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_json_nesting(bytes: &[u8]) -> Result<(), String> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(format!("JSON nesting exceeds {MAX_DEPTH} levels"));
                }
            }
            b'}' | b']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "unbalanced JSON delimiters".to_string())?
            }
            _ => {}
        }
    }
    if in_string || depth != 0 {
        return Err("unterminated string or unbalanced JSON delimiters".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_all_supported_geometry_variants_and_counts_coordinates() {
        let input = br#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,2]},{"type":"MultiPoint","coordinates":[[1,2],[3,4]]},{"type":"LineString","coordinates":[[0,0],[1,1]]},{"type":"MultiLineString","coordinates":[[[0,0],[1,1]]]},{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]},{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]}]}"#;
        let parsed = parse_bounded_bytes(input).expect("supported 2-D variants");
        assert_eq!(parsed.coordinate_count, 1 + 2 + 2 + 2 + 4 + 4);
        assert_eq!(parsed.geometry_count, 7);
        assert_eq!(parsed.source_bytes, input);
    }

    #[test]
    fn rejects_z_crs_null_empty_unclosed_and_deep_inputs() {
        for input in [
            br#"{"type":"Point","coordinates":[1,2,3]}"#.as_slice(),
            br#"{"type":"Point","coordinates":[1,2],"crs":{"type":"name"}}"#.as_slice(),
            br#"{"type":"Feature","geometry":null,"properties":{}}"#.as_slice(),
            br#"{"type":"GeometryCollection","geometries":[]}"#.as_slice(),
            br#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1]]] }"#.as_slice(),
        ] {
            assert!(
                parse_bounded_bytes(input).is_err(),
                "input should be rejected"
            );
        }
        let deeply_nested = format!(
            "{}1{}",
            "[".repeat(MAX_DEPTH + 1),
            "]".repeat(MAX_DEPTH + 1)
        );
        assert!(parse_bounded_bytes(deeply_nested.as_bytes()).is_err());
    }

    #[test]
    fn rejects_oversized_coordinate_count_before_conversion() {
        let points = (0..=MAX_COORDINATES)
            .map(|index| format!("[{},0]", index))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!(r#"{{"type":"LineString","coordinates":[{}]}}"#, points);
        assert!(parse_bounded_bytes(input.as_bytes()).is_err());
    }

    #[test]
    fn rejects_unsupported_malformed_oversized_and_nonfinite_inputs() {
        for input in [
            br#"{"type":"CircularString","coordinates":[[0,0],[1,1]]}"#.as_slice(),
            br#"{"type":"Point","coordinates":[1,2]"#.as_slice(),
            br#"{"type":"Point","coordinates":[NaN,2]}"#.as_slice(),
            br#"{"type":"FeatureCollection","features":[]}"#.as_slice(),
            br#"{"type":"Point","bbox":[0,1,2]}"#.as_slice(),
        ] {
            assert!(
                parse_bounded_bytes(input).is_err(),
                "adversarial input should be rejected"
            );
        }
        let oversized = vec![b' '; MAX_INPUT_BYTES + 1];
        assert!(parse_bounded_bytes(&oversized).is_err());
    }

    #[test]
    fn bounded_file_reader_rejects_oversized_input_without_stat_sized_allocation() {
        let path = std::env::temp_dir().join(format!(
            "kyberia-geometry-proof-oversized-{}-{}",
            std::process::id(),
            MAX_INPUT_BYTES
        ));
        std::fs::write(&path, vec![b'x'; MAX_INPUT_BYTES + 1])
            .expect("write bounded-reader fixture");
        let error = read_bounded_file(&path).expect_err("oversized file must be rejected");
        assert!(error.contains("exceeds"));
    }
}
