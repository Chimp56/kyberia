"""Acceptance tests for the isolated Phase 0 geometry-kernel proof."""
import hashlib
import json
from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
GEOMETRY = ROOT / "research" / "geometry"
FIXTURE = GEOMETRY / "fixtures" / "geometry-proof.json"
GEOJSON_FIXTURE = GEOMETRY / "fixtures" / "geometry-proof-input.geojson"
RUST_RESULT = GEOMETRY / "results" / "rust-desktop.json"
SHAPELY_RESULT = GEOMETRY / "results" / "shapely.json"
IMPORT_RESULT = GEOMETRY / "results" / "import-valid.json"
BENCHMARK = GEOMETRY / "results" / "benchmark.json"
WASM_BUILD = GEOMETRY / "results" / "wasm-build.json"
WASM_BEHAVIOR = GEOMETRY / "results" / "wasm-behavior.json"
LICENSES = ROOT / "docs" / "licenses" / "geometry-research-sources.json"
PYTHON = ROOT / ".tools" / "geometry-venv" / "bin" / "python"
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


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def expected_source_hashes():
    return {path: sha256(ROOT / path) for path in SOURCE_PATHS}


class GeometryResearchProof(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = json.loads(FIXTURE.read_text())
        cls.rust = json.loads(RUST_RESULT.read_text())
        cls.shapely = json.loads(SHAPELY_RESULT.read_text())
        cls.import_result = json.loads(IMPORT_RESULT.read_text())
        cls.wasm = json.loads(WASM_BEHAVIOR.read_text())
        cls.source_hashes = expected_source_hashes()

    def test_fixture_and_all_retained_results_bind_exact_sources(self):
        self.assertLessEqual(FIXTURE.stat().st_size, 256 * 1024)
        self.assertLessEqual(GEOJSON_FIXTURE.stat().st_size, 256 * 1024)
        self.assertEqual(self.rust["fixture_sha256"], sha256(FIXTURE))
        self.assertEqual(self.shapely["fixture_sha256"], sha256(FIXTURE))
        self.assertEqual(self.rust["geojson_fixture_sha256"], sha256(GEOJSON_FIXTURE))
        self.assertEqual(self.shapely["geojson_fixture_sha256"], sha256(GEOJSON_FIXTURE))
        build = json.loads(WASM_BUILD.read_text())
        for result in [self.rust, self.shapely, self.import_result, self.wasm, build]:
            self.assertEqual(result["source_hashes"], self.source_hashes)
            self.assertEqual(result["source_revision"], "UNCOMMITTED_WORKTREE_CONTENT_HASH_BOUND")
        benchmark = json.loads(BENCHMARK.read_text())
        self.assertEqual(benchmark["fixture_sha256"], sha256(FIXTURE))
        self.assertEqual(benchmark["source_hashes"], self.source_hashes)
        self.assertEqual(benchmark["source_revision"], "UNCOMMITTED_WORKTREE_CONTENT_HASH_BOUND")
        self.assertEqual(self.fixture["schema_version"], 1)

    def test_bounded_import_path_records_raw_geojson_and_counts(self):
        self.assertEqual(self.import_result["status"], "PASS")
        self.assertEqual(self.import_result["document_type"], "FeatureCollection")
        self.assertEqual(self.import_result["source_path"], "research/geometry/fixtures/geometry-proof-input.geojson")
        self.assertEqual(self.import_result["source_sha256"], sha256(GEOJSON_FIXTURE))
        self.assertEqual(self.import_result["observed"], {"coordinates": 16, "geometries": 3})
        self.assertEqual(self.import_result["limits"]["max_input_bytes"], 256 * 1024)
        self.assertEqual(self.import_result["limits"]["max_coordinates"], 10_000)

    def test_python_import_rejects_bbox_variants_and_huge_numbers_cleanly(self):
        code = """
import sys
sys.path.insert(0, 'research/geometry')
from shapely_proof import bounded_geojson_import
cases = [
    {'type': 'Point', 'coordinates': [1, 2], 'bbox': [0, 1, 2]},
    {'type': 'Point', 'coordinates': [1, 2], 'bbox': [0, 1, 2, 3, 4]},
    {'type': 'Point', 'coordinates': [10 ** 1000, 2]},
]
nested = {'leaf': 1}
for _ in range(70):
    nested = {'nested': nested}
cases.append({'type': 'Feature', 'properties': nested, 'geometry': {'type': 'Point', 'coordinates': [1, 2]}})
for value in cases:
    try:
        bounded_geojson_import(value)
    except ValueError:
        pass
    except Exception as error:
        raise AssertionError(f'unexpected exception type: {type(error).__name__}: {error}')
    else:
        raise AssertionError('adversarial value was accepted')
"""
        subprocess.run([str(PYTHON), "-c", code], cwd=ROOT, check=True)

    def test_both_backends_import_same_formats_and_agree_on_boolean_areas(self):
        expected = self.fixture["expected"]
        for result in [self.rust, self.shapely]:
            self.assertEqual(result["import"], {"format": "GeoJSON FeatureCollection + WKT", "feature_count": 3, "geometry_count": 3, "wkt_polygon_count": 4})
            self.assertEqual(result["bounds"]["observed_coordinates"], 16)
            self.assertEqual(result["bounds"]["observed_geometries"], 3)
            self.assertEqual(result["coordinate_semantics"], "2-D planar floor-local metres; z/floor is application metadata")
            operations = result["operations"]
            for key in ["shell_hole_area", "polygon_a_area", "polygon_b_area", "intersection_area", "union_area", "difference_area"]:
                self.assertAlmostEqual(operations[key], expected[key], places=9)
            self.assertAlmostEqual(operations["buffer_square_area"], expected["buffer_square_area"], delta=0.03)
        for key in ["shell_hole_area", "intersection_area", "union_area", "difference_area", "buffer_square_area"]:
            self.assertAlmostEqual(self.rust["operations"][key], self.shapely["operations"][key], delta=1e-7)

    def test_floor_filter_precedes_operations_and_excludes_same_xy_upper_wall(self):
        for result in [self.rust, self.shapely]:
            operations = result["operations"]
            floor_filter = operations["floor_filter"]
            self.assertEqual(floor_filter["selected_floor"], "ground")
            self.assertEqual(floor_filter["selected_wall_features"], 1)
            self.assertEqual(floor_filter["upper_wall_features"], 1)
            self.assertEqual(floor_filter["wall_segments_processed"], self.fixture["expected"]["wall_segments_processed"])
            self.assertTrue(floor_filter["upper_excluded_before_2d_operation"])
            self.assertTrue(operations["upper_floor_same_xy_is_separate_metadata"])
            self.assertTrue(operations["same_floor_wall_intersects"])

    def test_line_intersections_cover_cross_touch_collinear_and_hole(self):
        expected = self.fixture["expected"]
        for result in [self.rust, self.shapely]:
            operations = result["operations"]
            self.assertEqual(operations["crossing"]["kind"], "point")
            self.assertEqual(operations["crossing"]["xy"], expected["crossing_point"])
            self.assertEqual(operations["touch"]["kind"], "point")
            self.assertEqual(operations["touch"]["xy"], expected["touch_point"])
            self.assertIn(operations["collinear"]["kind"], ["collinear", "linestring"])
            self.assertEqual(operations["collinear"]["segment"], expected["collinear_segment"])
            self.assertAlmostEqual(operations["hole_span_inside_length"], expected["hole_span_inside_length"], places=9)
            for key in ["crossing", "touch", "touch_wall", "collinear", "collinear_overlap", "hole_span"]:
                self.assertEqual(operations["probe_inputs"][key], self.fixture["lines"][key])

    def test_invalid_geometry_is_diagnosed_and_repair_is_explicit_and_hashed(self):
        self.assertTrue(self.fixture["expected"]["invalid_geometry"])
        self.assertFalse(self.rust["invalid_geometry"]["is_valid"])
        self.assertGreater(self.rust["invalid_geometry"]["diagnostic_count"], 0)
        self.assertEqual(self.rust["invalid_geometry"]["repair"]["status"], "NOT_PERFORMED")
        repair = self.shapely["invalid_geometry"]["repair"]
        self.assertFalse(self.shapely["invalid_geometry"]["is_valid"])
        self.assertTrue(repair["explicit"])
        self.assertTrue(repair["result_is_valid"])
        self.assertEqual(repair["result_type"], "MultiPolygon")
        self.assertEqual(repair["artifact_sha256"], hashlib.sha256(repair["artifact_wkt"].encode()).hexdigest())
        self.assertEqual(repair["source_sha256"], self.rust["invalid_geometry"]["source"]["sha256"])
        self.assertTrue(repair["tool"] and repair["shapely_version"] and repair["geos_version"])

    def test_repeated_operations_are_deterministic_for_each_backend(self):
        for result in [self.rust, self.shapely]:
            self.assertEqual(result["operations"]["union_area"], result["determinism"]["repeated_area"])
            self.assertEqual(result["operations"]["buffer_square_area"], result["determinism"]["repeated_buffer_area"])
        self.assertEqual(self.rust["operations"]["union_area"], self.shapely["operations"]["union_area"])

    def test_wasm_is_behaviorally_executed_and_matches_desktop_semantics(self):
        build = json.loads(WASM_BUILD.read_text())
        self.assertEqual(build["status"], "PASS")
        self.assertEqual(build["target"], "wasm32-unknown-unknown")
        self.assertEqual(build["profile"], "release")
        self.assertEqual(build["behavior_status"], "PASS")
        self.assertEqual(build["behavior_runtime"], "node-webassembly")
        self.assertGreater(build["binary_bytes"], 0)
        binary = ROOT / build["binary_path"]
        self.assertTrue(binary.is_file(), "normal Gate E validation requires the rebuilt WASM artifact")
        self.assertEqual(build["binary_sha256"], sha256(binary))
        self.assertEqual(build["behavior_sha256"], sha256(WASM_BEHAVIOR))
        self.assertEqual(self.wasm["wasm_sha256"], build["binary_sha256"])
        self.assertEqual(self.wasm["semantics_version"], 1)
        desktop = self.rust["operations"]
        wasm = self.wasm["operations"]
        for wasm_key, desktop_key in [("union_area", "union_area"), ("intersection_area", "intersection_area"), ("difference_area", "difference_area"), ("buffer_area", "buffer_square_area"), ("hole_span_length", "hole_span_inside_length")]:
            self.assertAlmostEqual(wasm[wasm_key], desktop[desktop_key], delta=1e-9)
        self.assertEqual(wasm["crossing_x"], desktop["crossing"]["xy"][0])

    def test_same_workload_benchmark_is_retained(self):
        benchmark = json.loads(BENCHMARK.read_text())
        self.assertEqual(benchmark["harness"], "geometry-proof-benchmark")
        self.assertEqual(benchmark["iterations"], 20)
        self.assertIn("bounded 2-D GeoJSON import", benchmark["workload"])
        self.assertIn("Process timings include startup", benchmark["comparison_note"])
        self.assertEqual(benchmark["internal_timing_comparison"]["status"], "DESCRIPTIVE_ONLY")
        self.assertEqual(benchmark["rust_binary_sha256"], self.rust_binary_sha256())
        self.assertIn("embedded fixture decode", benchmark["implementations"]["rust_geo"]["internal_timing_boundary"])
        self.assertIn("fixture/raw GeoJSON file reads", benchmark["implementations"]["geos_shapely"]["internal_timing_boundary"])
        self.assertEqual(set(benchmark["implementations"]), {"rust_geo", "geos_shapely"})
        for implementation in benchmark["implementations"].values():
            self.assertGreater(implementation["internal_elapsed_ms"]["median"], 0)
            self.assertGreater(implementation["process_elapsed_ms"]["median"], 0)

    @staticmethod
    def rust_binary_sha256():
        binary = GEOMETRY / "target" / "release" / "kyberia-geometry-proof"
        if binary.exists():
            return sha256(binary)
        return json.loads(BENCHMARK.read_text())["rust_binary_sha256"]

    def test_license_record_freezes_inputs_and_states_transitive_scope(self):
        ledger = json.loads(LICENSES.read_text())
        self.assertEqual(ledger["schema_version"], 1)
        ids = {record["id"] for record in ledger["sources"]}
        self.assertTrue({"geo", "geojson", "wkt", "serde", "serde_json", "sha2", "shapely", "geos", "numpy"} <= ids)
        for record in ledger["sources"]:
            for key in ["id", "version", "license", "source", "redistribution", "provenance", "update_procedure"]:
                self.assertTrue(record[key])
        fixture_record = next(item for item in ledger["fixtures"] if item["path"] == "research/geometry/fixtures/geometry-proof.json")
        self.assertEqual(fixture_record["sha256"], sha256(FIXTURE))
        geojson_record = next(item for item in ledger["fixtures"] if item["path"] == "research/geometry/fixtures/geometry-proof-input.geojson")
        self.assertEqual(geojson_record["sha256"], sha256(GEOJSON_FIXTURE))
        self.assertEqual(ledger["execution"]["cargo_lock_sha256"], sha256(GEOMETRY / "Cargo.lock"))
        self.assertEqual(ledger["execution"]["python_lock_sha256"], sha256(GEOMETRY / "shapely-requirements.txt"))
        self.assertEqual(ledger["transitive_scope"]["cargo_lock_package_count"], 77)
        self.assertIn("not an SBOM", ledger["transitive_scope"]["license_review"])


if __name__ == "__main__":
    unittest.main()
