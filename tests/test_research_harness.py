"""Independent numerical and adversarial acceptance tests for research infrastructure."""
import hashlib
import importlib.util
import itertools
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "tools/validation" / (name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


fixtures = module("fixtures")
gates = module("runtime_gates")
catalog_generator = module("generate_catalog")


class NumericalFixtures(unittest.TestCase):
    def setUp(self):
        self.scenes = {scene["id"]: scene for scene in fixtures.canonical_scenes()["scenes"]}

    def test_checked_in_artifacts_are_reproducible(self):
        self.assertEqual(fixtures.check(), [])
        self.assertEqual(gates.load_json(gates.CATALOG), catalog_generator.build())
        self.assertEqual(hashlib.sha256(fixtures.canonical_bytes(fixtures.survey_fixture())).hexdigest(),
                         "a3188519f4a57687634ff02283e671f52e767df36cde3c15c2b26b5444c068cf")

    def test_friis_expected_values_and_wall_deltas(self):
        for frequency in [2400000000, 5000000000, 6000000000]:
            # Independent wavelength-domain computation, rather than the generator's stored constants.
            wavelength = 299792458 / frequency
            received_mw = 100 * (wavelength / (4 * math.pi * 10)) ** 2
            free_rss = 10 * math.log10(received_mw)
            for name, penalty in [("open", 0), ("one-wall", 3), ("two-wall", 10)]:
                expected = self.scenes["%s-%s" % (name, frequency)]["expected"]["rss"]
                self.assertAlmostEqual(expected["value"], free_rss - penalty, delta=expected["absolute_tolerance"])

    def test_frequency_material_and_floor_semantics(self):
        item = self.scenes["material-frequency-comparison"]["expected"]
        for a, b in zip(item["A_rss_dbm"], item["B_rss_dbm"]):
            self.assertEqual(b - a, -4)
        self.assertGreater(item["A_rss_dbm"][0], item["A_rss_dbm"][1])
        item = self.scenes["slab-versus-opening"]["expected"]
        self.assertEqual(item["open_slab_rss"]["value"] - item["closed_slab_rss"]["value"], 12)

    def test_antenna_local_axes_are_not_swapped(self):
        scene = self.scenes["directional-antenna-rotation"]
        for yaw, rss in zip(scene["inputs"]["yaw_degrees_ccw_about_z"], scene["expected"]["rss_dbm"]):
            world_to_local_x = math.cos(math.radians(-yaw))
            self.assertAlmostEqual(-60 + 3 * world_to_local_x, rss)

    def test_off_axis_antenna_rejects_reversed_yaw(self):
        scene = self.scenes["directional-antenna-rotation"]
        case = scene["inputs"]["off_axis_case"]
        expected = scene["expected"]["off_axis_rss_dbm"]
        x, y, z = case["world_receiver_direction"]
        self.assertAlmostEqual(x * x + y * y + z * z, 1)
        # A 45-degree boresight aligns with this receiver; -45 degrees is orthogonal.
        self.assertEqual(case["yaw_degrees_ccw_about_z"], [45, -45])
        self.assertEqual(expected, [-57, -60])
        for yaw, rss in zip(case["yaw_degrees_ccw_about_z"], expected):
            angle = math.radians(yaw)
            local_x = math.cos(angle) * x + math.sin(angle) * y
            reversed_x = math.cos(angle) * x - math.sin(angle) * y
            self.assertAlmostEqual(-60 + 3 * local_x, rss)
            self.assertGreater(abs(-60 + 3 * reversed_x - rss), 2.99)

    def test_power_coupling_and_unknown_noise(self):
        for name in ["co-channel-pair", "adjacent-channel-pair"]:
            item = self.scenes[name]
            inp, expected = item["inputs"], item["expected"]
            coupled_mw = 10 ** (inp["interferer_dbm"] / 10) * inp["coupling"] * inp["interferer_activity"]
            self.assertAlmostEqual(10 * math.log10(coupled_mw), expected["effective_interference"]["value"])
            self.assertIsNone(expected["sinr"])

    def test_hidden_node_is_receiver_specific(self):
        item = self.scenes["hidden-node-like-topology"]
        powers = item["inputs"]["rx_power_dbm"]
        self.assertLess(powers["A_to_B"], item["inputs"]["scenario_cca_threshold_dbm"])
        self.assertEqual(powers["A_to_R"], powers["B_to_R"])
        self.assertFalse(item["expected"]["A_senses_B"])
        self.assertIsNone(item["expected"]["collision_probability"])

    def test_high_rssi_does_not_imply_high_throughput(self):
        weak = self.scenes["low-rssi-high-throughput"]
        strong = self.scenes["high-rssi-congested"]
        self.assertLess(weak["inputs"]["synthetic_rssi_dbm"], strong["inputs"]["synthetic_rssi_dbm"])
        self.assertGreater(weak["expected"]["preserve_lan_mbps"], strong["expected"]["preserve_lan_mbps"])
        for scene in [weak, strong, self.scenes["healthy-lan-slow-wan"]]:
            self.assertNotEqual(scene["inputs"]["lan_endpoint"]["tier"], scene["inputs"]["internet_endpoint"]["tier"])
            self.assertIsNone(scene["expected"]["rssi_to_goodput_function"])
            self.assertFalse(scene["expected"]["may_assert_proven_root_cause"])

    def test_bidirectional_coverage_uses_weaker_direction(self):
        item = self.scenes["uplink-limited-client"]
        i, e = item["inputs"], item["expected"]
        down, up = i["ap_tx_dbm"] - i["path_loss_db"], i["client_tx_dbm"] - i["path_loss_db"]
        self.assertEqual(e["downlink"]["value"], down)
        self.assertEqual(e["uplink"]["value"], up)
        self.assertLess(min(down, up), i["scenario_required_bidirectional_dbm"])
        self.assertEqual(e["compliance"], "FAIL")

    def test_roam_hysteresis_does_not_invent_exact_time(self):
        scene = self.scenes["roaming-boundary"]
        inputs, outputs = scene["inputs"], scene["expected"]
        association = "A"
        for index, (a, b) in enumerate(zip(inputs["ap_A_dbm"], inputs["ap_B_dbm"])):
            if association == "A" and b - a >= inputs["hysteresis_db"]:
                association = "B"
            self.assertEqual(outputs["association"][index], association)
        self.assertEqual(outputs["transition_time_interval_seconds"], [2, 3])
        self.assertIsNone(outputs["authentication_duration_ms"])

    def test_exact_optimum_is_proved_by_enumeration_and_permutation(self):
        scene = self.scenes["multi-floor-candidate-optimum"]
        candidates = scene["inputs"]["candidates"]
        for permutation in itertools.permutations(candidates):
            solutions = []
            for count in range(len(candidates) + 1):
                for subset in itertools.combinations(permutation, count):
                    covered = {cell for c in subset for cell in c["covers"]}
                    score = (2 - len(covered), sum(c["cost"] for c in subset), len(subset),
                             sorted(c["id"] for c in subset))
                    solutions.append(score)
            self.assertEqual(min(solutions), (0, 4, 2, ["A", "B"]))
            feasible = {tuple(s[3]) for s in solutions if s[0] == 0}
            self.assertEqual(feasible, {tuple(s) for s in scene["expected"]["all_feasible_subsets"]})

    def test_high_fidelity_geometry_does_not_invent_field_baselines(self):
        scene = self.scenes["single-reflection-geometry"]
        expected = scene["expected"]["reflected_path_length"]
        self.assertAlmostEqual(expected["value"], 2 * math.hypot(2, 1), delta=expected["absolute_tolerance"])
        for name in ["single-reflection-geometry", "knife-edge-geometry"]:
            self.assertIsNone(self.scenes[name]["expected"]["path_coefficient"])
            self.assertIsNone(self.scenes[name]["expected"]["radio_map_rss"])

    def test_seed_and_blocked_folds(self):
        a, b = fixtures.survey_fixture(42), fixtures.survey_fixture(43)
        self.assertEqual(a, fixtures.survey_fixture(42))
        self.assertNotEqual(a["samples"], b["samples"])
        layout = a["position_covariance_layout"]
        self.assertEqual(layout["shape"], [2, 2])
        self.assertEqual(layout["axes"], ["x", "y"])
        self.assertEqual(layout["order"], "row-major")
        self.assertIn("z is fixed at 1.5 m", layout["vertical_assumption"])
        for sample in a["samples"]:
            self.assertEqual(len(sample[layout["field"]]), 4)
            self.assertEqual(sample["position_m"][2], 1.5)
            self.assertEqual(sample["fold"], "room-east" if sample["position_m"][0] >= 3 else "room-west")
            self.assertIsNone(sample["noise_dbm"])
            self.assertEqual(sample["evidence_class"], "synthetic")
        for invalid in [-1, 2 ** 32, True, 1.5]:
            with self.assertRaises(ValueError):
                fixtures.survey_fixture(invalid)

    def test_assets_are_not_measured_evidence_or_vendor_defaults(self):
        for asset in fixtures.assets().values():
            self.assertEqual(asset["evidence_class"], "synthetic")
        for scene in self.scenes.values():
            self.assertEqual(scene["evidence_class"], "synthetic")
            self.assertTrue(scene["assumptions"])
            self.assertTrue(scene["requirements"])

    def test_source_ledger_has_provenance_and_matching_hashes(self):
        ledger = gates.load_json(ROOT / "docs/licenses/fixture-sources.json")
        covered = set()
        for record in ledger["records"]:
            for key in ["source", "version", "license", "redistribution_status", "transformation", "provenance", "update_procedure"]:
                self.assertTrue(record[key])
            for artifact in record["artifacts"]:
                covered.add(artifact["path"])
                self.assertEqual(artifact["sha256"], hashlib.sha256((ROOT / artifact["path"]).read_bytes()).hexdigest())
        self.assertEqual(covered, set(fixtures.assets()))


class CleanRoomTin(unittest.TestCase):
    def test_numeric_golden_and_convex_hull(self):
        oracle = fixtures.tin_fixture()
        values = [max(values) for values in oracle["selected_bss_values"]]
        self.assertEqual(values, oracle["aggregated_values"])
        for query in oracle["queries"]:
            actual = fixtures.triangle_value(oracle["vertices"], values, query["xy"])
            if query["value"] is None:
                self.assertIsNone(actual)
            else:
                self.assertAlmostEqual(actual, query["value"], delta=oracle["absolute_tolerance"])
        self.assertEqual(oracle["upstream_execution"], "NOT_RUN")
        self.assertFalse(oracle["source_code_copied"])

    def test_affine_field_and_vertex_permutation(self):
        vertices = [[0, 0], [4, 0], [0, 4]]
        for permutation in itertools.permutations(vertices):
            values = [-40 - 5 * x - 10 * y for x, y in permutation]
            for x in range(5):
                for y in range(5 - x):
                    self.assertAlmostEqual(fixtures.triangle_value(list(permutation), values, [x, y]),
                                           -40 - 5 * x - 10 * y)

    def test_invalid_triangle_rejected(self):
        for vertices in [[[0, 0], [1, 1], [2, 2]], [[0, 0], [1, 1], [math.nan, 2]]]:
            with self.assertRaises(ValueError):
                fixtures.triangle_value(vertices, [-40, -50, -60], [0, 0])


class EvidenceGateTests(unittest.TestCase):
    def setUp(self):
        self.base = Path(tempfile.mkdtemp(prefix="kyberia-gate-test-"))
        self.addCleanup(self.cleanup_files)
        self.gate = gates.catalog()["sionna-cuda"]
        self.log = self.base / "acceptance.log"
        self.log.write_text("unit-test-only fabricated report; never runtime evidence\n")
        self.ref = {"path": self.log.name, "sha256": hashlib.sha256(self.log.read_bytes()).hexdigest()}

    def cleanup_files(self):
        # Only flat files this test created; no recursive directory deletion.
        for child in self.base.iterdir():
            if child.is_file() or child.is_symlink():
                child.unlink()
        self.base.rmdir()

    def pass_document(self):
        # A structure-validation fixture. It is never persisted as a real gate result.
        doc = gates.template(self.gate)
        doc.update(status="PASS", executed_at_utc="2026-09-06T12:00:00Z", operator="test-only",
                   command_argv=["test-only-driver", "--all"], exit_code=0)
        doc["versions"] = {key: "test-version" for key in self.gate["required_versions"]}
        doc["versions"].update(self.gate["pinned_versions"])
        doc["hardware"] = {key: "test-only-device" for key in self.gate["required_hardware"]}
        doc["checks"] = {key: {"status": "PASS", "evidence": [self.ref], "notes": "test-only"}
                         for key in self.gate["checks"]}
        return doc

    def test_all_catalog_templates_are_incomplete_not_pass(self):
        for gate in gates.catalog().values():
            doc = gates.template(gate)
            self.assertEqual(gates.validate(doc, gate, self.base), [])
            self.assertEqual(doc["status"], "NOT_RUN")
            self.assertTrue(gate["procedure"])
        self.assertEqual(len(gates.catalog()), 20)

    def test_pass_requires_complete_evidence(self):
        doc = gates.template(self.gate)
        doc["status"] = "PASS"
        self.assertTrue(gates.validate(doc, self.gate, self.base))
        self.assertEqual(gates.validate(self.pass_document(), self.gate, self.base), [])

    def test_missing_hardware_version_or_exact_pin_rejected(self):
        for section, key in [("hardware", "gpu_model"), ("versions", "cuda"), ("versions", "sionna_rt")]:
            doc = self.pass_document()
            doc[section][key] = "unknown"
            self.assertTrue(gates.validate(doc, self.gate, self.base))
        doc = self.pass_document()
        doc["versions"]["sionna_source_revision"] = "f" * 40
        self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_missing_check_failed_check_and_missing_artifacts_rejected(self):
        for mutation in ["delete", "fail", "empty", "hash"]:
            doc = self.pass_document()
            name = self.gate["checks"][0]
            if mutation == "delete":
                del doc["checks"][name]
            elif mutation == "fail":
                doc["checks"][name]["status"] = "FAIL"
            elif mutation == "empty":
                doc["checks"][name]["evidence"] = []
            else:
                doc["checks"][name]["evidence"] = [{"path": "acceptance.log", "sha256": "0" * 64}]
            self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_mock_or_contract_proof_cannot_satisfy_hardware(self):
        for kind in ["synthetic", "synthetic_contract", "runtime", "measured_field"]:
            doc = self.pass_document()
            doc["evidence_kind"] = kind
            self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_corrupt_or_escaping_evidence_rejected(self):
        for path in ["../acceptance.log", "/tmp/acceptance.log", "missing.log"]:
            with self.assertRaises(ValueError):
                gates.verify_reference(dict(self.ref, path=path), self.base)
        (self.base / "linked.log").symlink_to(self.log)
        with self.assertRaises(ValueError):
            gates.verify_reference(dict(self.ref, path="linked.log"), self.base)
        self.log.write_text("changed")
        with self.assertRaises(ValueError):
            gates.verify_reference(self.ref, self.base)

    def test_oversized_evidence_rejected_before_read(self):
        with self.log.open("wb") as handle:
            handle.truncate(gates.MAX_EVIDENCE_BYTES + 1)
        with self.assertRaises(ValueError):
            gates.verify_reference(self.ref, self.base)

    def test_blocker_needs_external_reason_and_hashed_support(self):
        doc = gates.template(self.gate)
        doc["status"] = "BLOCKED_EXTERNAL"
        self.assertTrue(gates.validate(doc, self.gate, self.base))
        doc["external_blocker"] = {"category": "hardware", "dependency": "compatible CUDA GPU",
                                   "reason": "test-only absent hardware", "requirement": "sionna-cuda",
                                   "resume_procedure": "run pinned GPU acceptance suite", "evidence": self.ref}
        self.assertEqual(gates.validate(doc, self.gate, self.base), [])
        doc["external_blocker"]["category"] = "implementation_missing"
        self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_two_revision_gate_requires_distinct_exact_revisions(self):
        self.gate = gates.catalog()["kismet-file-parity"]
        doc = self.pass_document()
        self.assertTrue(gates.validate(doc, self.gate, self.base))
        doc["compatibility_revisions"] = [self.gate["pinned_versions"]["kismet_source_revision"], "b" * 40]
        self.assertEqual(gates.validate(doc, self.gate, self.base), [])
        doc["compatibility_revisions"] = ["a" * 40, "a" * 40]
        self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_summary_cannot_hide_execution_or_forge_schema(self):
        doc = self.pass_document()
        for status in ["NOT_RUN", "FAIL"]:
            doc["status"] = status
            self.assertTrue(gates.validate(doc, self.gate, self.base))
        doc = self.pass_document()
        doc["schema_version"] = True
        self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_invalid_types_fail_without_becoming_pass(self):
        for value in [None, True, {}, [], 12]:
            doc = gates.template(self.gate)
            doc["status"] = value
            self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_whitespace_unknown_and_date_only_utc_rejected(self):
        doc = self.pass_document()
        doc["versions"]["cuda"] = " unknown "
        self.assertTrue(gates.validate(doc, self.gate, self.base))
        doc = self.pass_document()
        doc["executed_at_utc"] = "2026-09-06Z"
        self.assertTrue(gates.validate(doc, self.gate, self.base))

    def test_duplicate_and_nonfinite_json_rejected(self):
        path = self.base / "bad.json"
        for value in ['{"status":"PASS","status":"FAIL"}', '{"value":NaN}', '{"value":Infinity}',
                      '{"value":1e999}', '{"value":-1e999}']:
            path.write_text(value)
            with self.assertRaises(ValueError):
                gates.load_json(path)
        path.write_text('{"value":1.25e2,"negative":-0.125,"large":1e308}')
        self.assertEqual(gates.load_json(path), {"value": 125.0, "negative": -0.125, "large": 1e308})

    def test_cli_never_returns_success_for_not_run_or_blocked(self):
        path = self.base / "not-run.json"
        path.write_text(json.dumps(gates.template(self.gate)))
        result = subprocess.run([sys.executable, str(ROOT / "tools/validation/runtime_gates.py"), "check", str(path)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "NOT_RUN")


if __name__ == "__main__":
    unittest.main()
