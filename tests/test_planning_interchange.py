"""Independent proof tests for the neutral OpenRFPlan V1 interchange."""

from decimal import Decimal
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest
import uuid


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "research" / "interchange" / "interchange.py"
FIXTURE_PATH = ROOT / "research" / "interchange" / "fixtures" / "openrfplan-v1.json"
SEED_PATH = ROOT / "research" / "interchange" / "fixtures" / "openrfplan-seed-v1.txt"


def load_module():
    spec = importlib.util.spec_from_file_location("kyberia_openrfplan_interchange", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


INTERCHANGE = load_module()


class PlanningInterchangeProof(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.raw = FIXTURE_PATH.read_bytes()
        cls.document = INTERCHANGE.decode_document(cls.raw)

    def test_original_seeded_fixture_is_canonical_and_source_bound(self):
        self.assertLessEqual(len(self.raw), INTERCHANGE.MAX_DOCUMENT_BYTES)
        self.assertEqual(self.raw, INTERCHANGE.canonical_bytes(self.document))
        self.assertEqual(self.document["schema"], {"name": "openrfplan", "version": 1})
        source = self.document["provenance"]["sources"][0]
        self.assertEqual(source["sha256"], hashlib.sha256(SEED_PATH.read_bytes()).hexdigest())
        self.assertEqual(len(self.raw), 3017)
        self.assertEqual(
            hashlib.sha256(self.raw).hexdigest(),
            "c03c223e49f867e86446c1cbec08fcd50b427ca73c260cd649ec6556524ba580",
        )
        self.assertEqual(self.document["seeds"][0]["value"], 42)
        self.assertEqual(self.document["extensions"]["example.vendor"]["status"], "unknown")

    def test_collection_permutations_have_one_canonical_identity(self):
        permuted = copy.deepcopy(self.document)
        for key in [
            "coordinate_frames", "floors", "materials", "geometry", "access_points", "radios",
            "channel_constraints", "zones", "requirements", "seeds",
        ]:
            permuted[key].reverse()
        permuted["provenance"]["sources"].reverse()
        self.assertEqual(INTERCHANGE.canonical_bytes(permuted), self.raw)
        self.assertEqual(INTERCHANGE.canonical_sha256(permuted), hashlib.sha256(self.raw).hexdigest())

        with_more_radios = copy.deepcopy(self.document)
        second_radio = copy.deepcopy(with_more_radios["radios"][0])
        second_radio["id"] = "radio-office-alt"
        second_radio["position_m"] = [6, 4, 2]
        with_more_radios["radios"].append(second_radio)
        with_more_radios["access_points"][0]["radio_ids"].append("radio-office-alt")
        canonical = INTERCHANGE.canonical_bytes(with_more_radios)
        with_more_radios["access_points"][0]["radio_ids"].reverse()
        with_more_radios["channel_constraints"][0]["allowed_channels"].reverse()
        self.assertEqual(INTERCHANGE.canonical_bytes(with_more_radios), canonical)

        duplicate_channel = copy.deepcopy(self.document)
        duplicate_channel["channel_constraints"][0]["allowed_channels"].append(
            copy.deepcopy(duplicate_channel["channel_constraints"][0]["allowed_channels"][0])
        )
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.canonical_bytes(duplicate_channel)

    def test_decimal_bounds_reject_values_beyond_exact_limits(self):
        for value in ["10000000.0000000001", "-10000000.0000000001",
                      "10000000.00000000000000000000000001"]:
            document = copy.deepcopy(self.document)
            document["coordinate_frames"][0]["origin_m"][0] = Decimal(value)
            with self.assertRaises(INTERCHANGE.InterchangeError):
                INTERCHANGE.encode_document(document)
        for value in ["360.0000000000000001", "-360.0000000000000001"]:
            document = copy.deepcopy(self.document)
            parent = document["coordinate_frames"][0]
            child = copy.deepcopy(parent)
            child.update(id="exact-angle-child", parent_id=parent["id"])
            child["orientation_deg"] = [Decimal(value), 0, 0]
            document["coordinate_frames"].append(child)
            with self.assertRaises(INTERCHANGE.InterchangeError):
                INTERCHANGE.encode_document(document)

    def test_numeric_spellings_share_one_decimal_identity(self):
        integer = copy.deepcopy(self.document)
        decimal = copy.deepcopy(self.document)
        integer["floors"][0]["elevation_m"]["value"] = 0
        decimal["floors"][0]["elevation_m"]["value"] = 0.0
        self.assertEqual(INTERCHANGE.canonical_bytes(integer), INTERCHANGE.canonical_bytes(decimal))

        decimal_a = copy.deepcopy(self.document)
        decimal_b = copy.deepcopy(self.document)
        decimal_a["requirements"][0]["threshold"]["value"] = 1e-6
        decimal_b["requirements"][0]["threshold"]["value"] = 0.000001
        self.assertEqual(INTERCHANGE.canonical_bytes(decimal_a), INTERCHANGE.canonical_bytes(decimal_b))
        wire_decimal = self.raw.replace(b'"value":0', b'"value":0.0', 1)
        parsed = INTERCHANGE.decode_document(wire_decimal)
        self.assertEqual(INTERCHANGE.canonical_bytes(parsed), self.raw)
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.decode_canonical_document(wire_decimal)

    def test_unknown_values_and_extensions_survive_import_export(self):
        changed = copy.deepcopy(self.document)
        unknown = changed["radios"][0]["channel"]["band"]
        self.assertEqual(unknown, {"raw": "vendor-band-token", "reason": "unsupported", "status": "unknown", "unit": "band"})
        changed["extensions"]["vendor.example"] = {"opaque": [True, None, "kept"]}
        round_tripped = INTERCHANGE.decode_document(INTERCHANGE.canonical_bytes(changed))
        self.assertEqual(round_tripped["radios"][0]["channel"]["band"], unknown)
        self.assertEqual(round_tripped["extensions"]["vendor.example"], {"opaque": [True, None, "kept"]})

    def test_cli_performs_real_import_and_canonical_export(self):
        retained = ROOT / ".trash" / "test-runs" / f"planning-interchange-{uuid.uuid4().hex}"
        retained.mkdir(parents=True, exist_ok=True)
        output = retained / "exported-openrfplan-v1.json"
        subprocess.run(
            [sys.executable, str(MODULE_PATH), "canonicalize", str(FIXTURE_PATH), str(output)],
            cwd=ROOT,
            check=True,
        )
        self.assertEqual(output.read_bytes(), self.raw)
        result = subprocess.run(
            [sys.executable, str(MODULE_PATH), "validate", str(output)],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        summary = json.loads(result.stdout)
        self.assertEqual(summary["schema"], "openrfplan/1")
        self.assertEqual(summary["canonical_sha256"], hashlib.sha256(self.raw).hexdigest())
        self.assertEqual(summary["counts"]["geometry"], 3)

    def test_strict_canonical_decoder_distinguishes_valid_from_canonical(self):
        INTERCHANGE.decode_canonical_document(self.raw)
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.decode_canonical_document(b"  " + self.raw + b"\n")

    def test_duplicate_keys_invalid_numbers_and_future_version_are_rejected(self):
        with self.assertRaises(INTERCHANGE.DuplicateKeyError):
            INTERCHANGE.decode_document(b'{"schema":1,"schema":2}')
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.decode_document(b"NaN")
        future = self.raw.replace(b'"version":1', b'"version":2', 1)
        with self.assertRaises(INTERCHANGE.UnsupportedVersionError):
            INTERCHANGE.decode_document(future)

        for invalid in [True, 1.0, "1", {"value": 1}, [1]]:
            direct = copy.deepcopy(self.document)
            direct["schema"]["version"] = invalid
            with self.subTest(direct=invalid), self.assertRaises(INTERCHANGE.UnsupportedVersionError):
                INTERCHANGE.canonical_bytes(direct)
        for wire_value in [b"true", b"1.0", b'"1"', b'{"value":1}', b"[1]"]:
            wire = self.raw.replace(b'"version":1', b'"version":' + wire_value, 1)
            with self.subTest(wire=wire_value), self.assertRaises(INTERCHANGE.UnsupportedVersionError):
                INTERCHANGE.decode_document(wire)

    def test_references_duplicate_ids_and_typed_unknowns_are_checked(self):
        missing_reference = copy.deepcopy(self.document)
        missing_reference["zones"][0]["boundary_geometry_id"] = "missing-boundary"
        with self.assertRaises(INTERCHANGE.ReferenceError):
            INTERCHANGE.canonical_bytes(missing_reference)

        duplicate_id = copy.deepcopy(self.document)
        duplicate_id["geometry"][1]["id"] = duplicate_id["geometry"][0]["id"]
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.canonical_bytes(duplicate_id)

        bad_unknown = copy.deepcopy(self.document)
        bad_unknown["radios"][0]["channel"]["band"]["reason"] = "not-a-reason"
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.canonical_bytes(bad_unknown)

    def test_geometry_frame_units_and_cross_object_shape_are_not_defaults(self):
        for field, expected in INTERCHANGE._UNITS.items():
            self.assertEqual(self.document["units"][field], expected)
        malformed = copy.deepcopy(self.document)
        malformed["radios"][0]["frame_id"] = "floor-ground"
        with self.assertRaises(INTERCHANGE.ReferenceError):
            INTERCHANGE.canonical_bytes(malformed)
        malformed = copy.deepcopy(self.document)
        malformed["radios"][0]["floor_id"] = "floor-ground"
        malformed["radios"][0]["frame_id"] = "frame-ground"
        malformed["access_points"][0]["floor_id"] = "floor-ground"
        malformed["access_points"][0]["frame_id"] = "frame-ground"
        # The IDs still resolve; the association check must reject a radio
        # placed on a different valid floor/frame than its access point.
        malformed["coordinate_frames"].append({
            "id": "frame-alt", "parent_id": "frame-ground", "handedness": "right",
            "axis_order": ["x", "y", "z"], "origin_m": [0, 0, 0], "orientation_deg": [0, 0, 0],
        })
        malformed["floors"].append({"id": "floor-alt", "frame_id": "frame-alt", "elevation_m": {"status": "known", "unit": "m", "value": 3.0}})
        malformed["radios"][0]["floor_id"] = "floor-alt"
        malformed["radios"][0]["frame_id"] = "frame-alt"
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.canonical_bytes(malformed)
        malformed = copy.deepcopy(self.document)
        malformed["geometry"][0]["closed"] = False
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.canonical_bytes(malformed)

    def test_world_root_is_unrotated_but_child_rotation_is_explicitly_supported(self):
        rotated_root = copy.deepcopy(self.document)
        rotated_root["coordinate_frames"][0]["orientation_deg"] = [10, 0, 0]
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.canonical_bytes(rotated_root)
        rotated_child = copy.deepcopy(self.document)
        rotated_child["coordinate_frames"].append({
            "id": "frame-child", "parent_id": "frame-ground", "handedness": "right",
            "axis_order": ["x", "y", "z"], "origin_m": [2, 3, 0], "orientation_deg": [10, 20, 30],
        })
        INTERCHANGE.canonical_bytes(rotated_child)

    def test_resource_bounds_apply_to_raw_json_and_extension_values(self):
        with self.assertRaises(INTERCHANGE.ResourceLimitError):
            INTERCHANGE.decode_document(b"x" * (INTERCHANGE.MAX_DOCUMENT_BYTES + 1))
        retained = ROOT / ".trash" / "test-runs" / f"planning-interchange-oversize-{uuid.uuid4().hex}"
        retained.mkdir(parents=True, exist_ok=True)
        oversized_file = retained / "oversized.json"
        oversized_file.write_bytes(b"{" + b" " * INTERCHANGE.MAX_DOCUMENT_BYTES + b"}")
        with self.assertRaises(INTERCHANGE.ResourceLimitError):
            INTERCHANGE.read_document(oversized_file)
        deep = b"[" * (INTERCHANGE.MAX_NESTING + 1) + b"0" + b"]" * (INTERCHANGE.MAX_NESTING + 1)
        with self.assertRaises(INTERCHANGE.ResourceLimitError):
            INTERCHANGE.decode_document(deep)
        oversized = copy.deepcopy(self.document)
        oversized["extensions"]["example.vendor"] = list(range(INTERCHANGE.MAX_ARRAY_ITEMS + 1))
        with self.assertRaises(INTERCHANGE.ResourceLimitError):
            INTERCHANGE.canonical_bytes(oversized)
        oversized_integer = self.raw.replace(b'"value":42', b'"value":123456789012345678901', 1)
        with self.assertRaises(INTERCHANGE.ResourceLimitError):
            INTERCHANGE.decode_document(oversized_integer)

        cyclic = copy.deepcopy(self.document)
        cyclic["extensions"]["example.vendor"] = cyclic["extensions"]
        with self.assertRaises(INTERCHANGE.InterchangeError):
            INTERCHANGE.canonical_bytes(cyclic)

    def test_no_observations_scores_or_foreign_runtime_are_promoted(self):
        self.assertNotIn("observations", self.document)
        self.assertNotIn("scores", self.document)
        self.assertNotIn("optimizer", self.document)
        source = MODULE_PATH.read_text()
        self.assertNotIn("import deconflict", source)
        self.assertNotIn("import rf_atlas", source)


if __name__ == "__main__":
    unittest.main()
