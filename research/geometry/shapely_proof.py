"""Research-only GEOS/Shapely geometry proof with strict bounded import."""
from __future__ import annotations

import hashlib
import json
import os
import pathlib
import sys
import time

import shapely
from shapely import geometry, make_valid, wkt
from shapely.geometry import shape


ROOT = pathlib.Path(__file__).resolve().parent
FIXTURE = ROOT / "fixtures" / "geometry-proof.json"
GEOJSON_FIXTURE = ROOT / "fixtures" / "geometry-proof-input.geojson"
MAX_INPUT_BYTES = 256 * 1024
MAX_COORDINATES = 10_000
MAX_FEATURES = 1_024
MAX_GEOMETRIES = 4_096
MAX_DEPTH = 64
SOURCE_PATHS = [
    "research/geometry/fixtures/geometry-proof.json",
    "research/geometry/fixtures/geometry-proof-input.geojson",
    "research/geometry/Cargo.toml",
    "research/geometry/Cargo.lock",
    "research/geometry/src/main.rs",
    "research/geometry/src/lib.rs",
    "research/geometry/src/import.rs",
    "research/geometry/shapely_proof.py",
    "research/geometry/benchmark.py",
    "research/geometry/wasm_behavior.js",
    "research/geometry/shapely-requirements.txt",
]


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def source_hashes():
    return {path: sha256_bytes((ROOT.parent.parent / path).read_bytes()) for path in SOURCE_PATHS}


def source_revision():
    return os.environ.get("KYBERIA_GEOMETRY_SOURCE_REVISION", "UNCOMMITTED_WORKTREE_CONTENT_HASH_BOUND")


def finite_number(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return False
    try:
        converted = float(value)
    except (OverflowError, ValueError):
        return False
    return converted == converted and abs(converted) != float("inf")


def validate_bbox(bbox):
    if bbox is not None and (not isinstance(bbox, list) or len(bbox) != 4 or any(not finite_number(value) for value in bbox)):
        raise ValueError("only finite 2-D four-value bounding boxes are supported")


def enforce_json_nesting(value, depth=0):
    if depth > MAX_DEPTH:
        raise ValueError(f"JSON nesting exceeds {MAX_DEPTH} levels")
    if isinstance(value, dict):
        for child in value.values():
            enforce_json_nesting(child, depth + 1)
    elif isinstance(value, list):
        for child in value:
            enforce_json_nesting(child, depth + 1)


def reject_crs(value):
    if isinstance(value, dict):
        if "crs" in value:
            raise ValueError("deprecated/unsupported GeoJSON CRS member")
        for child in value.values():
            reject_crs(child)
    elif isinstance(value, list):
        for child in value:
            reject_crs(child)


def walk_position(position, state):
    if not isinstance(position, list) or len(position) != 2 or any(not finite_number(item) for item in position):
        raise ValueError("only finite 2-D positions are supported")
    state["coordinates"] += 1
    if state["coordinates"] > MAX_COORDINATES:
        raise ValueError("coordinate limit exceeded")


def walk_geometry(value, depth, state):
    if depth > MAX_DEPTH:
        raise ValueError("geometry nesting limit exceeded")
    if not isinstance(value, dict) or value.get("type") not in {"Point", "MultiPoint", "LineString", "MultiLineString", "Polygon", "MultiPolygon", "GeometryCollection"}:
        raise ValueError("unsupported geometry type")
    state["geometries"] += 1
    if state["geometries"] > MAX_GEOMETRIES:
        raise ValueError("geometry limit exceeded")
    kind = value["type"]
    validate_bbox(value.get("bbox"))
    if kind == "GeometryCollection":
        children = value.get("geometries")
        if not isinstance(children, list) or not children:
            raise ValueError("empty GeometryCollection")
        for child in children:
            walk_geometry(child, depth + 1, state)
        return
    coordinates = value.get("coordinates")
    if kind == "Point":
        walk_position(coordinates, state)
    elif kind == "MultiPoint":
        if not isinstance(coordinates, list) or not coordinates:
            raise ValueError("empty MultiPoint")
        for position in coordinates:
            walk_position(position, state)
    elif kind == "LineString":
        if not isinstance(coordinates, list) or len(coordinates) < 2:
            raise ValueError("LineString requires two positions")
        for position in coordinates:
            walk_position(position, state)
    elif kind == "MultiLineString":
        if not isinstance(coordinates, list) or not coordinates:
            raise ValueError("empty MultiLineString")
        for line in coordinates:
            if not isinstance(line, list) or len(line) < 2:
                raise ValueError("MultiLineString component requires two positions")
            for position in line:
                walk_position(position, state)
    elif kind in {"Polygon", "MultiPolygon"}:
        polygons = coordinates if kind == "MultiPolygon" else [coordinates]
        if not isinstance(polygons, list) or not polygons:
            raise ValueError("empty polygon collection")
        for polygon in polygons:
            if not isinstance(polygon, list) or not polygon:
                raise ValueError("Polygon requires an exterior ring")
            for ring in polygon:
                if not isinstance(ring, list) or len(ring) < 4 or ring[0] != ring[-1]:
                    raise ValueError("Polygon rings must be closed and contain four positions")
                for position in ring:
                    walk_position(position, state)


def bounded_geojson_import(document):
    enforce_json_nesting(document)
    reject_crs(document)
    if not isinstance(document, dict) or document.get("type") not in {"FeatureCollection", "Feature", "Point", "MultiPoint", "LineString", "MultiLineString", "Polygon", "MultiPolygon", "GeometryCollection"}:
        raise ValueError("unsupported GeoJSON document type")
    state = {"coordinates": 0, "geometries": 0}
    kind = document["type"]
    if kind == "FeatureCollection":
        validate_bbox(document.get("bbox"))
        features = document.get("features")
        if not isinstance(features, list) or not features or len(features) > MAX_FEATURES:
            raise ValueError("invalid or oversized FeatureCollection")
        for feature in features:
            if not isinstance(feature, dict) or feature.get("type") != "Feature" or not isinstance(feature.get("geometry"), dict):
                raise ValueError("Feature must contain a geometry")
            validate_bbox(feature.get("bbox"))
            walk_geometry(feature["geometry"], 0, state)
    elif kind == "Feature":
        validate_bbox(document.get("bbox"))
        if not isinstance(document.get("geometry"), dict):
            raise ValueError("Feature must contain a geometry")
        walk_geometry(document["geometry"], 0, state)
    else:
        walk_geometry(document, 0, state)
    return state


def pt_pair(point):
    return [float(point.x), float(point.y)]


def canonical_segment(line):
    points = [[float(x), float(y)] for x, y, *_ in line.coords]
    return sorted(points)[:2]


def main(output):
    start = time.perf_counter()
    fixture_bytes = FIXTURE.read_bytes()
    if len(fixture_bytes) > MAX_INPUT_BYTES:
        raise ValueError("fixture exceeds bounded import size")
    fixture = json.loads(fixture_bytes)
    enforce_json_nesting(fixture)
    geojson_bytes = GEOJSON_FIXTURE.read_bytes()
    if len(geojson_bytes) > MAX_INPUT_BYTES:
        raise ValueError("raw GeoJSON exceeds bounded import size")
    geojson_document = json.loads(geojson_bytes)
    import_info = bounded_geojson_import(geojson_document)
    floor_features = {floor: [feature for feature in geojson_document["features"] if feature["properties"].get("floor") == floor] for floor in ["ground", "upper"]}
    ground_feature = next(feature for feature in floor_features["ground"] if feature["geometry"]["type"] == "Polygon")
    ground_wall_feature = next(feature for feature in floor_features["ground"] if feature["geometry"]["type"] == "LineString")
    upper_wall_feature = next(feature for feature in floor_features["upper"] if feature["geometry"]["type"] == "LineString")
    ground = shape(ground_feature["geometry"])
    wall = shape(ground_wall_feature["geometry"])
    upper_wall = shape(upper_wall_feature["geometry"])
    upper_same_xy = upper_wall.equals(wall)
    if not upper_same_xy or upper_wall_feature["properties"].get("floor") == ground_wall_feature["properties"].get("floor"):
        raise ValueError("upper-floor same-XY exclusion fixture lost its floor distinction")
    a = wkt.loads(fixture["wkt"]["polygon_a"])
    b = wkt.loads(fixture["wkt"]["polygon_b"])
    square = wkt.loads(fixture["wkt"]["buffer_square"])
    invalid_text = fixture["wkt"]["invalid_bowtie"]
    invalid = wkt.loads(invalid_text)
    for name, candidate in [("ground", ground), ("polygon_a", a), ("polygon_b", b), ("buffer_square", square)]:
        if not candidate.is_valid:
            raise ValueError(f"{name} invalid before operations: {shapely.is_valid_reason(candidate)}")
    if invalid.is_valid:
        raise ValueError("invalid fixture unexpectedly valid")

    crossing = geometry.LineString(fixture["lines"]["crossing"])
    touching = geometry.LineString(fixture["lines"]["touch"])
    collinear = geometry.LineString(fixture["lines"]["collinear"])
    hole_span = geometry.LineString(fixture["lines"]["hole_span"])
    crossing_result = crossing.intersection(wall)
    touch_result = touching.intersection(geometry.LineString(fixture["lines"]["touch_wall"]))
    collinear_result = collinear.intersection(geometry.LineString(fixture["lines"]["collinear_overlap"]))
    clipped = hole_span.intersection(ground)
    repaired = make_valid(invalid)
    repaired_wkt = shapely.to_wkt(repaired, rounding_precision=16, trim=False)
    ground_wall_count = sum(feature["geometry"]["type"] == "LineString" for feature in floor_features["ground"])
    upper_wall_count = sum(feature["geometry"]["type"] == "LineString" for feature in floor_features["upper"])
    wall_segments = len(ground_wall_feature["geometry"]["coordinates"]) - 1
    result = {
        "harness": "geos-shapely",
        "library": {"shapely": shapely.__version__, "geos": shapely.geos_version_string, "geos_capi": shapely.geos_capi_version_string},
        "coordinate_semantics": "2-D planar floor-local metres; z/floor is application metadata",
        "source_revision": source_revision(),
        "source_hashes": source_hashes(),
        "fixture_sha256": sha256_bytes(fixture_bytes),
        "geojson_fixture_sha256": sha256_bytes(geojson_bytes),
        "bounds": {"max_input_bytes": MAX_INPUT_BYTES, "max_coordinates": MAX_COORDINATES, "observed_coordinates": import_info["coordinates"], "observed_geometries": import_info["geometries"]},
        "import": {"format": "GeoJSON FeatureCollection + WKT", "feature_count": len(fixture["geojson"]["features"]), "geometry_count": import_info["geometries"], "wkt_polygon_count": 4},
        "operations": {
            "shell_hole_area": ground.area,
            "polygon_a_area": a.area,
            "polygon_b_area": b.area,
            "intersection_area": a.intersection(b).area,
            "union_area": a.union(b).area,
            "difference_area": a.difference(b).area,
            "buffer_square_area": square.buffer(1.0, quad_segs=8).area,
            "crossing": {"kind": crossing_result.geom_type.lower(), "xy": pt_pair(crossing_result)},
            "touch": {"kind": touch_result.geom_type.lower(), "xy": pt_pair(touch_result)},
            "collinear": {"kind": collinear_result.geom_type.lower(), "segment": canonical_segment(collinear_result)},
            "hole_span_inside_length": clipped.length,
            "same_floor_wall_intersects": ground.intersects(wall),
            "upper_floor_same_xy_is_separate_metadata": upper_same_xy and upper_wall_feature["properties"]["floor"] != ground_wall_feature["properties"]["floor"],
            "floor_filter": {"selected_floor": "ground", "selected_wall_features": ground_wall_count, "upper_wall_features": upper_wall_count, "wall_segments_processed": wall_segments, "upper_excluded_before_2d_operation": True},
            "probe_inputs": {"crossing": fixture["lines"]["crossing"], "touch": fixture["lines"]["touch"], "touch_wall": fixture["lines"]["touch_wall"], "collinear": fixture["lines"]["collinear"], "collinear_overlap": fixture["lines"]["collinear_overlap"], "hole_span": fixture["lines"]["hole_span"]},
        },
        "invalid_geometry": {
            "is_valid": False,
            "diagnostic": shapely.is_valid_reason(invalid),
            "source": {"format": "WKT", "sha256": sha256_bytes(invalid_text.encode()), "text": invalid_text},
            "repair": {"explicit": True, "method": "make_valid", "result_type": repaired.geom_type, "result_is_valid": repaired.is_valid, "artifact_wkt": repaired_wkt, "artifact_sha256": sha256_bytes(repaired_wkt.encode()), "source_sha256": sha256_bytes(invalid_text.encode()), "tool": "shapely", "shapely_version": shapely.__version__, "geos_version": shapely.geos_version_string},
        },
        "determinism": {"repeated_area": a.union(b).area, "repeated_buffer_area": square.buffer(1.0, quad_segs=8).area, "normalized_union_wkt": shapely.normalize(a.union(b)).wkt},
        "timing_boundary": "from before fixture/raw GeoJSON file reads and decode through operations, before result serialization",
        "elapsed_ms": (time.perf_counter() - start) * 1000.0,
    }
    pathlib.Path(output).parent.mkdir(parents=True, exist_ok=True)
    pathlib.Path(output).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else str(ROOT / "results" / "shapely.json"))
