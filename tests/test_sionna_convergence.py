"""Synthetic algorithm tests; these do not execute or validate Sionna RT."""

from copy import deepcopy
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "workers/sionna"))

from rfatlas_sionna import (AUDITED_REVISION, CAPABILITY_SCHEMA_VERSION,
                            ENGINE_PINS, WORKER_VERSION)
from rfatlas_sionna.contract import digest
from rfatlas_sionna.convergence import ConvergenceError, run_sweep
from rfatlas_sionna.examples import request as example_request


def _synthetic_envelope(request, gains, elapsed, *, version_change=False, malformed=False):
    grid = request["grid"]
    nx = round(grid["size_m"][0] / grid["cell_size_m"][0])
    ny = round(grid["size_m"][1] / grid["cell_size_m"][1])
    centers = []
    for y in range(ny):
        row = []
        for x in range(nx):
            row.append([
                grid["center_m"][0] - grid["size_m"][0] / 2 + (x + .5) * grid["cell_size_m"][0],
                grid["center_m"][1] - grid["size_m"][1] / 2 + (y + .5) * grid["cell_size_m"][1],
                grid["center_m"][2],
            ])
        centers.append(row)
    shape = [len(request["transmitters"]), ny, nx]
    data = {
        "kind": "planar_radio_map",
        "transmitter_ids": [tx["id"] for tx in request["transmitters"]],
        "shape": shape,
        "units": "linear_power_ratio",
        "path_gain": gains,
        "cell_centers_m": centers,
        "no_data_mask": [[[False for _ in range(nx)] for _ in range(ny)] for _ in range(shape[0])],
        "precision": "float32",
        "grid": deepcopy(grid),
        "orientation_rad": [0, 0, 0],
        "axis_order": ["transmitter", "y", "x"],
        "combination": "monte_carlo_cell_average_power",
    }
    if malformed:
        data["axis_order"] = ["x", "y", "transmitter"]
    versions = dict(ENGINE_PINS)
    versions.update({
        "python": "3.12.0",
        "os": "synthetic-test-runtime",
        "machine": "synthetic-cpu",
        "worker": WORKER_VERSION,
        "backend": "llvm_ad_mono_polarized",
        "audited_source_revision": AUDITED_REVISION,
        "audited_python_source_verified": True,
        "source_pin_sha256": digest(json.loads(
            (ROOT / "workers/sionna/rfatlas_sionna/source_pin.json").read_text())),
        "llvm_library_sha256": "0" * 64,
    })
    if version_change:
        versions["machine"] = "different-synthetic-cpu"
    capabilities = {
        "cpu_llvm": True,
        "operations": ["validate_scene", "path_query", "radio_map"],
        "scene_kinds": ["empty_space"],
        "product_tier_supported": False,
    }
    result = {
        "schema_version": 1,
        "status": "completed",
        "request_id": request["request_id"],
        "request_sha256": digest(request),
        "capability_schema_version": CAPABILITY_SCHEMA_VERSION,
        "versions": versions,
        "capabilities": capabilities,
        "data": data,
        "data_sha256": digest(data),
        "scene_sha256": request["scene_sha256"],
        "profile_revision": request["profile_revision"],
        "solver": deepcopy(request["solver"]),
        "frequency_hz": request["frequency_hz"],
        "bandwidth_hz": request["bandwidth_hz"],
        "temperature_k": request["temperature_k"],
        "noise_model": "not_used_for_path_gain",
        "elapsed_engine_s": elapsed / 2,
        "warnings": [],
    }
    now = datetime.now(timezone.utc).isoformat()
    return {
        "schema_version": 1,
        "request_id": request["request_id"],
        "request_sha256": digest(request),
        "started_utc": now,
        "ended_utc": now,
        "elapsed_s": elapsed,
        "returncode": 0,
        "log_sha256": "a" * 64,
        "log": "",
        "cleanup_error": None,
        "cancel_error": None,
        "resource_limits": {"wall_timeout_s": request["limits"]["timeout_s"]},
        "result": result,
        "status": "completed",
    }


class ConvergenceSweepTests(unittest.TestCase):
    def setUp(self):
        self.base = example_request("radio_map")
        self.calls = []

    def runner(self, request, _python_executable):
        self.calls.append(deepcopy(request))
        budget, seed = request["solver"]["samples"], request["solver"]["seed"]
        ntx = len(request["transmitters"])
        ny = round(request["grid"]["size_m"][1] / request["grid"]["cell_size_m"][1])
        nx = round(request["grid"]["size_m"][0] / request["grid"]["cell_size_m"][0])
        gains = [[[1.0 + seed / 10 + 1 / budget + tx + y / 100 + x / 1000
                   for x in range(nx)] for y in range(ny)] for tx in range(ntx)]
        return _synthetic_envelope(request, gains, budget / 1000 + seed / 10000)

    def test_bounded_sweep_summarizes_cells_runtime_and_provenance(self):
        report = run_sweep(self.base, [10, 100], [1, 3], "unused-for-synthetic-test",
                           runner=self.runner)
        self.assertEqual(report["kind"], "sionna_cpu_convergence_diagnostic")
        self.assertEqual(report["claim_scope"], "support diagnostic only; not product acceptance")
        self.assertEqual(len(self.calls), 4)
        self.assertEqual(report["base_request_sha256"], digest(self.base))
        self.assertEqual(report["shape"], [1, 4, 4])
        lower = report["summary_by_budget"][0]
        upper = report["summary_by_budget"][1]
        expected = (1 + .1 + 1 / 10 + 1 + .3 + 1 / 10) / 2
        self.assertAlmostEqual(lower["per_transmitter"][0]["mean_path_gain"][0][0], expected)
        self.assertAlmostEqual(lower["per_transmitter"][0]["standard_error_path_gain"][0][0], .1)
        self.assertGreater(lower["runtime"]["max"], lower["runtime"]["min"])
        self.assertEqual(report["adjacent_budget_changes"][0]["from_samples_per_run"], 10)
        self.assertEqual(report["adjacent_budget_changes"][0]["to_samples_per_run"], 100)
        self.assertAlmostEqual(
            report["adjacent_budget_changes"][0]["mean_delta_path_gain"][0][0][0], -.09)
        self.assertAlmostEqual(
            report["adjacent_budget_changes"][0]["symmetric_relative_change_fraction"][0][0][0],
            .09 / 1.3)
        self.assertRegex(report["runs"][0]["result_provenance"]["data_sha256"],
                         re.compile(r"^[0-9a-f]{64}$"))
        self.assertEqual(report["runs"][0]["worker_status"], "completed")
        self.assertEqual(report["runs"][0]["returncode"], 0)
        self.assertEqual(report["runs"][0]["request"]["solver"]["samples"], 10)
        self.assertEqual(report["runs"][0]["request"]["solver"]["seed"], 1)
        self.assertAlmostEqual(upper["per_transmitter"][0]["mean_path_gain"][0][0],
                               (1 + .1 + .01 + 1 + .3 + .01) / 2)

    def test_only_request_id_seed_and_sample_count_change(self):
        run_sweep(self.base, [10, 20], [2, 4], "unused", runner=self.runner)
        for actual in self.calls:
            expected = deepcopy(self.base)
            expected.pop("request_id")
            actual_compare = deepcopy(actual)
            actual_compare.pop("request_id")
            expected["solver"].pop("samples")
            expected["solver"].pop("seed")
            actual_compare["solver"].pop("samples")
            actual_compare["solver"].pop("seed")
            self.assertEqual(actual_compare, expected)

    def test_rejects_bad_options_before_running(self):
        for budgets, seeds in (([10], [1, 2]), ([20, 10], [1, 2]),
                               ([10, 20], [1, 1]), ([10, 20], [True, 2]),
                               ([10, 20], [1])):
            with self.subTest(budgets=budgets, seeds=seeds):
                with self.assertRaises(ConvergenceError):
                    run_sweep(self.base, budgets, seeds, "unused", runner=self.runner)
        self.assertEqual(self.calls, [])

    def test_rejects_malformed_map_and_unavailable_cells(self):
        def malformed_runner(request, _python):
            gains = [[[1.0] * 4 for _ in range(4)]]
            return _synthetic_envelope(request, gains, .1, malformed=True)
        with self.assertRaises(ConvergenceError):
            run_sweep(self.base, [10, 20], [1, 2], "unused", runner=malformed_runner)

        def unavailable_runner(request, _python):
            gains = [[[1.0] * 4 for _ in range(4)]]
            gains[0][0][0] = 0
            envelope = _synthetic_envelope(request, gains, .1)
            envelope["result"]["data"]["no_data_mask"][0][0][0] = True
            envelope["result"]["data_sha256"] = digest(envelope["result"]["data"])
            return envelope
        with self.assertRaises(ConvergenceError):
            run_sweep(self.base, [10, 20], [1, 2], "unused", runner=unavailable_runner)

    def test_rejects_runtime_provenance_changes(self):
        calls = 0
        def changing_runner(request, python):
            nonlocal calls
            calls += 1
            gains = [[[1.0] * 4 for _ in range(4)]]
            return _synthetic_envelope(request, gains, .1, version_change=(calls == 2))
        with self.assertRaises(ConvergenceError):
            run_sweep(self.base, [10, 20], [1, 2], "unused", runner=changing_runner)

    def test_enforces_aggregate_sample_and_summary_value_bounds(self):
        high_cost = deepcopy(self.base)
        high_cost["transmitters"] = [
            {"id": f"tx{index}", "position_m": [index * 2, 0, 4]}
            for index in range(4)
        ]
        with self.assertRaises(ConvergenceError):
            run_sweep(high_cost, [900_000, 1_000_000], list(range(8)), "unused",
                      runner=self.runner)

        large_map = deepcopy(high_cost)
        large_map["scene"]["bounds_m"] = [[-100, -100, 0], [100, 100, 10]]
        large_map["scene_sha256"] = digest(large_map["scene"])
        large_map["grid"] = {"center_m": [0, 0, 1.5], "size_m": [64, 64],
                              "cell_size_m": [1, 1]}
        with self.assertRaises(ConvergenceError):
            run_sweep(large_map, [10, 20, 30], [1, 2], "unused", runner=self.runner)
        self.assertEqual(self.calls, [])

    def test_imports_without_loading_the_engine(self):
        self.assertNotIn("sionna.rt", sys.modules)

    def test_default_path_dispatches_each_run_to_existing_client(self):
        with mock.patch("rfatlas_sionna.convergence.run_worker",
                        side_effect=self.runner) as client_run:
            run_sweep(self.base, [10, 20], [1, 2], "unused")
        self.assertEqual(client_run.call_count, 4)

    def test_enforces_serialized_output_limit(self):
        with mock.patch("rfatlas_sionna.convergence.MAX_OUTPUT_BYTES", 1):
            with self.assertRaises(ConvergenceError):
                run_sweep(self.base, [10, 20], [1, 2], "unused", runner=self.runner)


if __name__ == "__main__":
    unittest.main()
