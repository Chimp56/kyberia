"""Independent contract attacks and real Swift lifecycle tests (no simulated RF)."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import random
import signal
import subprocess
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "collectors/macos"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


contract = module("macos_contract", ADAPTER / "contract.py")
fixtures = module("macos_fixtures", ADAPTER / "fixtures/generate.py")
with patch.dict(sys.modules, {"contract": contract}):
    runtime = module("macos_runtime_check", ADAPTER / "runtime_check.py")


class MacOSContractTests(unittest.TestCase):
    def valid(self):
        return copy.deepcopy(fixtures.assets()["valid.ndjson"])

    def rejected(self, events):
        with self.assertRaises(contract.ContractError):
            contract.decode_stream(fixtures.encode(events))

    def test_fixtures_are_deterministic_and_explicitly_synthetic(self):
        for name, events in fixtures.assets().items():
            with self.subTest(name=name):
                data = fixtures.encode(events)
                self.assertEqual(data, (ADAPTER / "fixtures" / name).read_bytes())
                parsed = contract.decode_stream(data)
                self.assertEqual(parsed[0]["evidence_origin"], "synthetic_fixture")
                self.assertEqual(parsed[-1]["status"], events[-1]["status"])

    def test_optional_source_measurements_remain_unknown(self):
        obs = contract.decode_stream(fixtures.encode(self.valid()))[3]
        for key in ["noise_dbm", "dwell_seconds", "result_age_seconds", "position", "phy"]:
            self.assertEqual(obs[key]["state"], "unknown")
        self.assertEqual(obs["channel"]["value"]["raw_width_enum"], 99)
        self.assertEqual(obs["channel"]["value"]["width_mhz"]["state"], "unknown")

    def test_partial_evidence_is_not_success(self):
        parsed = contract.decode_stream((ADAPTER / "fixtures/partial.ndjson").read_bytes())
        self.assertTrue(parsed[-1]["partial"])
        self.assertEqual(parsed[-1]["observation_count"], 1)
        self.assertEqual(parsed[-1]["status"], "partial")

    def test_forbidden_scan_permission_and_interface_states(self):
        for status in ["denied", "restricted", "not_determined", "unknown"]:
            events = self.valid()
            events[1]["location_authorization"] = status
            self.rejected(events)
        for mutation in [lambda e: e[1].update(location_services_enabled=False),
                         lambda e: e[1]["sources"][0].update(power_on=False),
                         lambda e: e[1].update(sources=[])]:
            events = self.valid()
            mutation(events)
            self.rejected(events)

    def test_invented_metrics_and_unit_errors_rejected(self):
        for key in ["noise_dbm", "rssi_dbm"]:
            for value in [0, 10, -201, True, "-61", -61.5]:
                events = self.valid()
                events[3][key] = fixtures.known(value)
                self.rejected(events)
        for key in ["dwell_seconds", "result_age_seconds", "position", "phy"]:
            events = self.valid()
            events[3][key] = fixtures.known(0)
            self.rejected(events)
        events = self.valid()
        events[3]["channel"]["value"]["width_mhz"] = fixtures.known(320)
        self.rejected(events)

    def test_channel_geometry_not_inferred_from_unknown_enum(self):
        for key, value in [("band", "5_ghz"), ("width_mhz", 80), ("frequency_hz", 5180000000)]:
            events = self.valid()
            events[3]["channel"]["value"][key] = fixtures.known(value)
            self.rejected(events)

    def test_required_source_provenance(self):
        for key in ["collector_build", "collector_version", "source_schema", "interface_name", "os_version", "source_id"]:
            events = self.valid()
            events[3]["source"][key] = "other"
            self.rejected(events)
        events = self.valid()
        events[3]["source"]["physical_radio_id"] = fixtures.known("guessed-radio")
        self.rejected(events)

    def test_privacy_and_identifier_validation(self):
        events = self.valid()
        events[0]["identifier_policy"] = "redacted"
        self.rejected(events)
        for value in ["00:00:00:00:00:00", "ff:ff:ff:ff:ff:ff", "03:00:00:00:00:01", "02::00:00:00:00:01", "nope"]:
            events = self.valid()
            events[3]["bssid"] = fixtures.known(value)
            self.rejected(events)
        for value in ["", "!!", "YQ==" * 20]:
            events = self.valid()
            events[3]["ssid_octets_base64"] = fixtures.known(value)
            self.rejected(events)

    def test_loss_duplication_reordering_and_mixed_sessions(self):
        events = self.valid()
        for index in range(len(events)):
            self.rejected(events[:index] + events[index + 1:])
        self.rejected(events + [events[-1]])
        events = self.valid()
        events[3]["session_id"] = "00000000-0000-4000-8000-000000000099"
        self.rejected(events)
        events = self.valid()
        events[-1]["observation_count"] = 0
        self.rejected(events)

    def test_monotonic_time_and_capture_time_semantics(self):
        for value in ["-1", "01", str(2 ** 64), 5000, "1e3"]:
            events = self.valid()
            events[3]["time"]["receipt_monotonic_ns"] = value
            self.rejected(events)
        for start, end in [("3000", "3500"), ("2900", "5000"), ("2900", "1000")]:
            events = self.valid()
            events[3]["api_window"] = {"start_monotonic_ns": start, "end_monotonic_ns": end}
            self.rejected(events)
        events = self.valid()
        events[3]["time"]["capture_time"] = fixtures.known("2026-09-07T00:00:00Z")
        self.rejected(events)

    def test_resource_bounds_and_truncation(self):
        data = fixtures.encode(self.valid())
        for malformed in [data[:-1], b"\n", b"{}\n" * (contract.MAX_RECORDS + 1),
                          b" " * contract.MAX_RECORD_BYTES + data, b"\xff\n{}\n"]:
            with self.assertRaises(contract.ContractError):
                contract.decode_stream(malformed)
        events = self.valid()
        events[0]["max_observations"] = 0
        self.rejected(events)

    def test_duplicate_keys_nonfinite_numbers_and_malformed_shapes(self):
        for payload in [b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}', b'{"x":1e999}', b'{"x":-1e999}']:
            with self.assertRaises(contract.ContractError):
                contract._json(payload)
        self.assertEqual(contract._json(b'{"x":1e100}')["x"], 1e100)
        rng = random.Random(937)
        bad = [None, [], {}, True, -44, "", ["nested"]]
        for _ in range(250):
            events = self.valid()
            index = rng.randrange(len(events))
            key = rng.choice(["protocol", "sequence", "session_id", "time", "kind"])
            events[index][key] = rng.choice(bad)
            self.rejected(events)

    def test_authorization_lifecycle_contract(self):
        for enabled in [False, True]:
            for state in ["authorized", "not_determined", "denied", "restricted", "unknown"]:
                events = fixtures.assets()["probe.ndjson"]
                events[0]["command"] = "authorize"
                stamp = {key: events[1][key] for key in ["protocol", "session_id", "sequence", "time"]}
                events[1] = dict(stamp, kind="authorization", state=state, requested_by_operator=True,
                                 location_services_enabled=enabled, prompt_requested=enabled and state == "not_determined")
                events[-1].update(status="ok" if enabled and state == "authorized" else "permission_required",
                                  reason="location_authorized" if enabled and state == "authorized" else "location_unavailable",
                                  final_authorization=state, final_location_services_enabled=enabled)
                contract.decode_stream(fixtures.encode(events))
                events[1]["prompt_requested"] = not events[1]["prompt_requested"]
                self.rejected(events)

    def test_authorization_success_requires_possible_transition_and_final_evidence(self):
        for initial in ["authorized", "not_determined", "denied", "restricted", "unknown"]:
            events = fixtures.assets()["probe.ndjson"]
            events[0]["command"] = "authorize"
            stamp = {key: events[1][key] for key in ["protocol", "session_id", "sequence", "time"]}
            events[1] = dict(stamp, kind="authorization", state=initial, requested_by_operator=True,
                            location_services_enabled=True, prompt_requested=initial == "not_determined")
            events[-1].update(status="ok", reason="location_authorized", final_authorization="authorized",
                              final_location_services_enabled=True)
            if initial in {"authorized", "not_determined"}:
                contract.decode_stream(fixtures.encode(events))
                for key, value in [("final_authorization", "denied"), ("final_location_services_enabled", False)]:
                    changed = copy.deepcopy(events)
                    changed[-1][key] = value
                    self.rejected(changed)
            else:
                self.rejected(events)

    def test_scan_cannot_use_disabled_source_when_another_radio_is_powered(self):
        events = self.valid()
        other = copy.deepcopy(events[1]["sources"][0])
        other["interface_name"] = "en1"
        other["source_id"] = events[0]["session_id"] + ":en1"
        events[1]["sources"].append(other)
        events[1]["sources"][0]["power_on"] = False
        self.rejected(events)

    def test_runtime_exit_code_validation_distinguishes_termination_signals(self):
        for reason, correct, wrong in [("sigint", 130, 143), ("sigterm", 143, 130)]:
            complete = {"status": "cancelled", "reason": reason}
            runtime.validate_exit(complete, correct)
            with self.assertRaises(ValueError):
                runtime.validate_exit(complete, wrong)
        with self.assertRaises(ValueError):
            runtime.validate_exit({"status": "cancelled", "reason": "unknown"}, 130)


@unittest.skipUnless(sys.platform == "darwin" and os.environ.get("KYBERIA_MACOS_NATIVE_TESTS") == "1",
                     "native Swift checks enabled with KYBERIA_MACOS_NATIVE_TESTS=1 on macOS")
class NativeMacOSLifecycleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = ADAPTER / ".build"
        cls.build.mkdir(exist_ok=True)
        for name, sources in {"process-tests": ["Sources/Wire.swift", "Tests/ProcessHarness.swift"],
                              "permission-tests": ["Sources/Permission.swift", "Tests/PermissionTests.swift"]}.items():
            subprocess.run(["xcrun", "swiftc", "-swift-version", "5", "-warnings-as-errors", "-O",
                            "-module-cache-path", str(cls.build / "module-cache"),
                            *[str(ADAPTER / path) for path in sources], "-o", str(cls.build / name)],
                           check=True, capture_output=True, timeout=120)

    def test_permission_decisions_run_actual_swift(self):
        result = subprocess.run([str(self.build / "permission-tests")], capture_output=True, timeout=3, check=True)
        self.assertIn(b"ten permission/service combinations", result.stdout)

    def test_actual_timeout_has_terminal_event(self):
        result = subprocess.run([str(self.build / "process-tests")], capture_output=True, timeout=4)
        self.assertEqual(result.returncode, 124)
        events = contract.decode_stream(result.stdout)
        self.assertEqual(events[-1]["status"], "timeout")
        self.assertEqual(events[-1]["observation_count"], 0)

    def test_main_runloop_delivers_foundation_callbacks(self):
        result = subprocess.run([str(self.build / "process-tests"), "--runloop-callback"], capture_output=True, timeout=3)
        self.assertEqual(result.returncode, 70)
        self.assertEqual(contract.decode_stream(result.stdout)[-1]["reason"], "test_runloop_callback_delivered")

    def test_signals_produce_one_terminal_event(self):
        for sig, expected in [(signal.SIGINT, 130), (signal.SIGTERM, 143)]:
            child = subprocess.Popen([str(self.build / "process-tests")], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                first = child.stdout.readline()
                self.assertEqual(json.loads(first)["kind"], "hello")
                child.send_signal(sig)
                rest, errors = child.communicate(timeout=3)
                events = contract.decode_stream(first + rest)
                self.assertEqual(child.returncode, expected)
                self.assertEqual(events[-1]["status"], "cancelled")
                self.assertEqual(len(events), 2)
                self.assertFalse(errors)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.communicate(timeout=3)

    def test_disconnected_output_consumer_exits_bounded(self):
        child = subprocess.Popen([str(self.build / "process-tests")], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        child.stdout.close()
        try:
            child.wait(timeout=3)
            self.assertEqual(child.returncode, 74)
            self.assertIn(b"incomplete stream", child.stderr.read())
        finally:
            child.stderr.close()
            if child.poll() is None:
                child.kill()
                child.wait(timeout=3)


if __name__ == "__main__":
    unittest.main()
