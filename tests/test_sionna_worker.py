"""Contract/lifecycle tests run without Sionna; real engine proof is separate."""

from copy import deepcopy
import json
import signal
import subprocess
from pathlib import Path
import sys
import threading
import time
import tempfile
import unittest
import venv

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "workers/sionna"))
from rfatlas_sionna.contract import (ContractError, MAX_LOG_BYTES, MAX_REQUEST_BYTES,
                                     canonical_bytes, decode, digest, validate, validate_result)
from rfatlas_sionna.client import run, supervise
from rfatlas_sionna.examples import request


class ContractTests(unittest.TestCase):
    def test_original_requests_and_roundtrip(self):
        for operation in ("validate_scene", "path_query", "radio_map"):
            value = request(operation)
            self.assertEqual(validate(decode(canonical_bytes(value))), value)

    def test_reject_malformed_envelopes(self):
        for payload in (b"null", b"[]", b'{"schema_version":true}', b"{", b'{"a":1,"a":2}',
                        b'{"a":NaN}', b"["*2000, b" "*(MAX_REQUEST_BYTES+1)):
            with self.subTest(payload=payload[:30]):
                with self.assertRaises(ContractError):
                    validate(decode(payload))

    def test_numeric_units_and_unsupported_features(self):
        cases = [("frequency_hz", True), ("frequency_hz", float("nan")),
                 ("frequency_hz", float("inf")), ("frequency_hz", -1),
                 ("bandwidth_hz", 2e10), ("temperature_k", 0),
                 ("scene_sha256", "0"*64), ("operation", "calibration_step"),
                 ("request_id", "../../project"), ("receivers", [])]
        for key, value in cases:
            candidate = request()
            candidate[key] = value
            with self.subTest(key=key, value=value):
                with self.assertRaises(ContractError):
                    validate(candidate)
        for field, value in (("seed", -1), ("seed", True), ("samples", 1000001),
                             ("max_depth", 1), ("backend", "cuda_ad_mono_polarized"),
                             ("loop_mode", "symbolic")):
            candidate = request()
            candidate["solver"][field] = value
            with self.assertRaises(ContractError):
                validate(candidate)

    def test_geometry_grid_and_radio_validation(self):
        candidate = request()
        candidate["scene"]["coordinate_frame"] = "pixels"
        candidate["scene_sha256"] = digest(candidate["scene"])
        with self.assertRaises(ContractError):
            validate(candidate)
        for position in ([0, 0, 4], [100, 0, 4], [1, 2], [True, 2, 4]):
            candidate = request()
            candidate["receivers"][0]["position_m"] = position
            with self.assertRaises(ContractError):
                validate(candidate)
        for key, value in (("cell_size_m", [0.001, 0.001]), ("size_m", [8.5, 8]),
                           ("center_m", [100, 0, 1]), ("center_m", [0, 0, 4])):
            candidate = request("radio_map")
            candidate["grid"][key] = value
            with self.assertRaises(ContractError):
                validate(candidate)

    def test_unknown_fields_and_interactions_fail_closed(self):
        for target in ("scene", "solver", "limits"):
            candidate = request()
            candidate[target]["command"] = "arbitrary"
            with self.assertRaises(ContractError):
                validate(candidate)
        candidate = request()
        candidate["solver"]["interactions"]["refraction"] = True
        with self.assertRaises(ContractError):
            validate(candidate)

    def test_all_input_semantics_affect_request_hash(self):
        baseline = request()
        for key, value in (("frequency_hz", 5e9), ("bandwidth_hz", 40e6),
                           ("temperature_k", 300), ("profile_revision", "iso-v2")):
            candidate = deepcopy(baseline)
            candidate[key] = value
            self.assertNotEqual(digest(candidate), digest(baseline))
        for key, value in (("seed", 41), ("samples", 50000)):
            candidate = deepcopy(baseline)
            candidate["solver"][key] = value
            self.assertNotEqual(digest(candidate), digest(baseline))
        self.assertEqual(digest(dict(reversed(list(baseline.items())))), digest(baseline))


class LifecycleTests(unittest.TestCase):
    def supervise(self, program, timeout=2, cancel=None, payload=b"{}"):
        return supervise([sys.executable, "-I", "-c", program], payload, timeout, cancel)

    def test_actual_subprocess_roundtrip_and_logs(self):
        result = self.supervise("import sys; data=sys.stdin.buffer.read(); sys.stdout.buffer.write(data); print('diagnostic', file=sys.stderr)")
        self.assertEqual(result["state"], "exited")
        self.assertEqual(result["returncode"], 0)
        self.assertEqual(result["stdout"], b"{}")
        self.assertIn(b"diagnostic", result["stderr"])

    def test_crash_and_next_process_recovery(self):
        result = self.supervise("import os; os._exit(37)")
        self.assertEqual(result["returncode"], 37)
        self.assertEqual(self.supervise("print('next-job')")["returncode"], 0)

    def test_timeout_and_closed_pipe_hang(self):
        for program in ("import time; time.sleep(10)",
                        "import os,time; os.close(1); os.close(2); time.sleep(10)"):
            result = self.supervise(program, timeout=0.15)
            self.assertEqual(result["state"], "timed_out")
            self.assertLess(result["elapsed_s"], 2)
            self.assertNotEqual(result["returncode"], 0)

    def test_cancel_running_process(self):
        event = threading.Event()
        timer = threading.Timer(0.1, event.set)
        timer.start()
        try:
            result = self.supervise("import time; time.sleep(10)", cancel=event)
        finally:
            timer.join()
        self.assertEqual(result["state"], "cancelled")
        self.assertLess(result["elapsed_s"], 2)

    def test_backpressure_on_stdin_remains_cancellable(self):
        result = self.supervise("import time; time.sleep(10)", timeout=0.1,
                                payload=b" " * MAX_REQUEST_BYTES)
        self.assertEqual(result["state"], "timed_out")

    def test_stdout_and_stderr_resource_limits(self):
        for fd in (1, 2):
            result = self.supervise("import os\nwhile True: os.write(%d,b'x'*8192)" % fd)
            self.assertEqual(result["state"], "output_limit")
            self.assertLessEqual(len(result["stderr"]), MAX_LOG_BYTES)

    def test_engine_absence_explicit_no_fallback(self):
        # A fresh stdlib-only environment guarantees absence on any supported POSIX host.
        # Preserve its tiny ignored directory; do not recursively clean test artifacts.
        tools_dir = ROOT / ".tools"
        tools_dir.mkdir(exist_ok=True)
        directory = Path(tempfile.mkdtemp(prefix="sionna-absent-", dir=tools_dir))
        venv.EnvBuilder(with_pip=False).create(directory)
        result = run(request(), directory / "bin/python")
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error"], "engine_unavailable")
        self.assertNotIn("data", result.get("result", {}))

    def test_cli_cancellation_while_input_is_incomplete(self):
        with subprocess.Popen([sys.executable, str(ROOT / "workers/sionna/worker.py")],
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE) as process:
            try:
                process.stdin.write(b"{")
                process.stdin.flush()
                time.sleep(0.2)
                process.send_signal(signal.SIGTERM)
                process.wait(timeout=2)  # Keep stdin open: cancellation must end the read.
                stdout, stderr = process.communicate(timeout=2)
            finally:
                if process.poll() is None:
                    process.kill()
                process.wait()
        self.assertEqual(process.returncode, 2, stderr)
        result = json.loads(stdout)
        self.assertIn("cancelled", result["detail"])


class RecordedResultTests(unittest.TestCase):
    def test_correlated_but_corrupt_results_rejected(self):
        # Recorded real runtime artifact supplies shape/units; this is a contract test only.
        report = json.loads((ROOT / "workers/sionna/evidence/cpu-proof.json").read_text())
        job = next(x for x in report["jobs"] if x["request"]["operation"] == "radio_map")
        original = job["response"]["result"]
        validate_result(original, job["request"])
        for field, value in (("shape", [1, 3, 5]), ("units", "dBm"),
                             ("path_gain", [[[float("inf")]]]),
                             ("transmitter_ids", ["wrong-transmitter"]),
                             ("no_data_mask", [[[True]]]), ("cell_centers_m", [[[0, 0, 0]]]),
                             ("combination", "coherent"), ("precision", "float64"),
                             ("orientation_rad", [0, 1, 0])):
            result = deepcopy(original)
            result["data"][field] = value
            with self.subTest(field=field):
                with self.assertRaises((ContractError, ValueError)):
                    result["data_sha256"] = digest(result["data"])
                    validate_result(result, job["request"])
        for key, value in (("schema_version", True), ("versions", []),
                           ("request_id", "wrong-job"), ("data_sha256", "0"*64),
                           ("frequency_hz", 5e9), ("bandwidth_hz", 40e6), ("temperature_k", 300),
                           ("profile_revision", "other-profile"), ("noise_model", "made-up")):
            result = deepcopy(original)
            result[key] = value
            with self.assertRaises(ContractError):
                validate_result(result, job["request"])
        for key, value in (("audited_source_revision", "0"*40), ("source_pin_sha256", "0"*64)):
            result = deepcopy(original)
            result["versions"][key] = value
            with self.assertRaises(ContractError):
                validate_result(result, job["request"])

    def test_negative_delays_and_inconsistent_path_power(self):
        report = json.loads((ROOT / "workers/sionna/evidence/cpu-proof.json").read_text())
        job = next(x for x in report["jobs"] if x["request"]["operation"] == "path_query")
        original = job["response"]["result"]
        validate_result(original, job["request"])
        for mutate in (lambda d: d["delays_s"][0][0].__setitem__(0, -1),
                       lambda d: d["delays_s"][0][0].__setitem__(0, 0),
                       lambda d: d["delays_s"][0][0].__setitem__(0, 1),
                       lambda d: d["delays_s"][0][0].__setitem__(0, 1e-20),
                       lambda d: d["path_gain"][0][0][0].__setitem__(0, 1),
                       lambda d: d["coefficients_real"][0][0][0][0][0].__setitem__(0, 1)):
            result = deepcopy(original)
            mutate(result["data"])
            result["data_sha256"] = digest(result["data"])
            with self.assertRaises(ContractError):
                validate_result(result, job["request"])

    def test_completed_results_require_typed_runtime_provenance(self):
        report = json.loads((ROOT / "workers/sionna/evidence/cpu-proof.json").read_text())
        jobs = [next(x for x in report["jobs"] if x["request"]["operation"] == operation)
                for operation in ("capabilities", "path_query", "radio_map")]
        invalid = {"python": ({}, None, "", "3.9.6", "unknown", "3.12"),
                   "os": ({}, None, "", " ", "macOS\nforged"),
                   "machine": ({}, None, "", " ", "arm64\x00"),
                   "llvm_library_sha256": ({}, None, "", "unknown", "g"*64, "a"*63)}
        for job in jobs:
            original = job["response"]["result"]
            validate_result(original, job["request"])
            for field, values in invalid.items():
                result = deepcopy(original)
                del result["versions"][field]
                with self.subTest(operation=job["request"]["operation"], field=field, value="missing"):
                    with self.assertRaises(ContractError):
                        validate_result(result, job["request"])
                for value in values:
                    result = deepcopy(original)
                    result["versions"][field] = value
                    with self.subTest(operation=job["request"]["operation"], field=field, value=value):
                        with self.assertRaises(ContractError):
                            validate_result(result, job["request"])


if __name__ == "__main__":
    unittest.main()
