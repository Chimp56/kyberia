"""Execute real CPU jobs and preserve numerical evidence. Never substitute a fixture engine."""

import argparse
from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import sys
import threading

from rfatlas_sionna.client import run
from rfatlas_sionna.contract import digest
from rfatlas_sionna.examples import request


def flatten(value):
    if isinstance(value, list):
        return [x for child in value for x in flatten(child)]
    return [value]


def mean_cell_gain(center, cell_size, tx, frequency, subdivisions=100):
    # Independent midpoint quadrature of Friis power over the actual cell area.
    # This compares a cell average, not point power evaluated only at its center.
    total = 0.0
    for y in range(subdivisions):
        for x in range(subdivisions):
            point = [center[0] + ((x+.5)/subdivisions-.5)*cell_size[0],
                     center[1] + ((y+.5)/subdivisions-.5)*cell_size[1], center[2]]
            total += (299792458 / (4*math.pi*frequency*math.dist(point, tx)))**2
    return total / subdivisions**2


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = {"schema_version": 1, "evidence_kind": "runtime", "scope": "empty_space_cpu_proof",
              "created_utc": datetime.now(timezone.utc).isoformat(), "command": sys.argv,
              "status": "FAIL", "jobs": [], "checks": [], "gate_i_passed": False,
              "tolerances": {"path_relative_error": 1e-5, "delay_relative_error": 1e-5,
                             "same_seed_relative_error": 1e-5, "map_cell_relative_error": 0.05},
              "open_gates": ["full_upstream_suite", "materials", "refraction", "reflection",
                             "diffraction", "scattering", "antenna_transforms", "multi_floor",
                             "oom", "hard_memory_quota", "remote_artifacts", "field_holdouts",
                             "desktop_without_worker", "production_performance_envelope"]}

    def job(value, cancellation=None):
        result = run(value, args.python, cancellation)
        report["jobs"].append({"request": value, "response": result})
        return result

    def check(name, condition, **evidence):
        report["checks"].append({"name": name, "passed": bool(condition), **evidence})
        if not condition:
            raise AssertionError(name)

    try:
        capabilities = job({"schema_version": 1, "request_id": "cpu-probe", "operation": "capabilities"})
        check("pinned_cpu_backend", capabilities["status"] == "completed")
        report["runtime"] = capabilities["result"]["versions"]
        report["cuda"] = capabilities["result"]["capabilities"]["cuda"]
        for frequency in (2.4e9, 5.2e9, 6.5e9):
            value = request(frequency_hz=frequency)
            value["receivers"] = [{"id": "rx%d" % distance, "position_m": [distance, 0, 4]}
                                  for distance in (1, 5, 10)]
            result = job(value)
            check("path_execution_%d" % frequency, result["status"] == "completed")
            data = result["result"]["data"]
            measured, delays = flatten(data["path_gain"]), flatten(data["delays_s"])
            analytic = [(299792458/(4*math.pi*frequency*d))**2 for d in (1, 5, 10)]
            errors = [abs(a-b)/b for a, b in zip(measured, analytic)]
            check("friis_%d" % frequency, len(measured) == 3 and max(errors) < 1e-5,
                  max_relative_error=max(errors), expected=analytic, actual=measured)
            expected_delays = [d/299792458 for d in (1, 5, 10)]
            check("absolute_delays_%d" % frequency, len(delays) == 3 and all(
                abs(a-b)/b < 1e-5 for a, b in zip(delays, expected_delays)))
        # Multiple transmitters remain separately addressable.
        value = request()
        value["transmitters"].append({"id": "tx1", "position_m": [5, 0, 4]})
        value["receivers"].append({"id": "rx1", "position_m": [15, 0, 4]})
        result = job(value)
        check("two_transmitter_path_execution", result["status"] == "completed")
        gains = flatten(result["result"]["data"]["path_gain"])
        expected = [(299792458/(4*math.pi*2.4e9*distance))**2 for distance in (10, 5, 15, 10)]
        check("two_by_two_transmitter_receiver_axes", len(gains) == 4 and all(
            abs(a-b)/b < 1e-5 for a,b in zip(gains, expected)), path_gain=gains, expected=expected)
        delays = flatten(result["result"]["data"]["delays_s"])
        check("two_by_two_delay_axes", len(delays) == 4 and all(
            abs(a-distance/299792458)/(distance/299792458) < 1e-5
            for a,distance in zip(delays, (10,5,15,10))))
        value = request()
        value["scene"]["bounds_m"] = [[9990, 9990, 0], [10000, 10000, 10]]
        value["scene_sha256"] = digest(value["scene"])
        value["transmitters"][0]["position_m"] = [9998.125, 9998.25, 4]
        value["receivers"][0]["position_m"] = [9999.2501, 9998.9, 4.1]
        result = job(value)
        check("large_coordinate_los_execution", result["status"] == "completed")
        distance = math.dist(value["transmitters"][0]["position_m"], value["receivers"][0]["position_m"])
        delay = flatten(result["result"]["data"]["delays_s"])[0]
        check("large_coordinate_los_delay", delay > 0 and math.isclose(delay, distance/299792458,
              rel_tol=1e-5, abs_tol=0.002/299792458), absolute_distance_error_m=abs(delay*299792458-distance))
        values_by_budget = {}
        reference = None
        for samples, seed in ((10000, 42), (100000, 42), (100000, 43), (100000, 44), (100000, 42)):
            value = request("radio_map", seed=seed)
            value["solver"]["samples"] = samples
            result = job(value)
            check("map_execution_%d_%d" % (samples, seed), result["status"] == "completed")
            data = result["result"]["data"]
            actual = flatten(data["path_gain"])
            expected = [mean_cell_gain(center, [2, 2], [0, 0, 4], 2.4e9)
                        for row in data["cell_centers_m"] for center in row]
            errors = [abs(a-b)/b for a, b in zip(actual, expected)]
            check("map_cell_average_%d_%d" % (samples, seed), data["shape"] == [1, 4, 4]
                  and len(actual) == 16 and (samples < 100000 or max(errors) < 0.05),
                  accuracy_threshold_applies=samples >= 100000,
                  note="10k is a diagnostic undersampling case; 100k is the tested acceptance budget.",
                  max_relative_error=max(errors), rms_relative_error=math.sqrt(sum(e*e for e in errors)/16))
            check("map_mask_and_integrity", not any(flatten(data["no_data_mask"])) and
                  result["result"]["data_sha256"] == digest(data))
            values_by_budget.setdefault(str(samples), []).append(actual)
            if samples == 100000 and seed == 42:
                if reference is not None:
                    check("fixed_seed_repeatability", all(abs(a-b)/b < 1e-5 for a, b in zip(actual, reference)))
                reference = actual
        report["map_samples"] = values_by_budget
        value = request()
        value["limits"]["timeout_s"] = 0.1
        check("actual_worker_timeout", job(value)["error"] == "timed_out")
        event = threading.Event()
        timer = threading.Timer(0.1, event.set)
        timer.start()
        try:
            cancelled = job(request(), event)
        finally:
            timer.join()
        check("actual_worker_cancellation", cancelled.get("error") == "cancelled")
        check("recovery_after_cancel", job(request())["status"] == "completed")
        report["status"] = "PASS"
    except Exception as error:
        report["failure"] = type(error).__name__ + ": " + str(error)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, allow_nan=False)+"\n")
    print(json.dumps({"status": report["status"], "checks": len(report["checks"]),
                      "output": str(args.output), "gate_i_passed": False,
                      "failure": report.get("failure")}))
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
