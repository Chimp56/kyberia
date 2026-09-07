"""Deterministic parser tests and explicitly fake child-process lifecycle tests."""
from dataclasses import asdict
import json
import os
import signal
from pathlib import Path
import sys
import threading
import time
import unittest
from unittest import mock
from types import SimpleNamespace
import uuid

from research.active.contract import Invalid, MAX_JSON, Request, parse_result, strict_json
from research.active.process import MAX_STDERR, execute, run
from research.active.acceptance import loopback, server_summary

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "research/active/fixtures"


def request(**changes):
    fields = dict(schema_version="1", request_id="unit-test", target_ip="127.0.0.1", target_class="loopback",
                  port=55291, protocol="tcp", direction="upload", duration_s=1,
                  offered_rate_bps=2000000, streams=1, authorization="owned_loopback")
    fields.update(changes)
    return Request(**fields)


def fixture(protocol="tcp", direction="upload"):
    return (FIXTURES / (protocol + "-" + direction + ".json")).read_bytes()


class ContractTests(unittest.TestCase):
    def test_zombie_group_permission_error_requires_post_reap_absence(self):
        from research.active.process import _stop
        child = mock.Mock(pid=12345)
        with mock.patch("research.active.process.os.killpg", side_effect=[PermissionError("zombie"), ProcessLookupError()]) as kill:
            _stop(child)
        self.assertEqual(kill.call_args_list, [mock.call(12345, signal.SIGKILL), mock.call(12345, 0)])
        child.wait.assert_called_with(timeout=2)
        for probe in (None, PermissionError("still denied")):
            with mock.patch("research.active.process.os.killpg", side_effect=[PermissionError("denied"), probe]):
                with self.assertRaises(PermissionError): _stop(child)

    def test_acceptance_fails_on_server_thread_or_cleanup_error(self):
        for effect in (PermissionError("synthetic thread failure"),
                       SimpleNamespace(cleanup_error="synthetic cleanup failure")):
            with mock.patch("research.active.acceptance.socket.socket") as socket_type, \
                    mock.patch("research.active.acceptance.time.sleep"), \
                    mock.patch("research.active.acceptance.run", return_value={"status": "cancelled"}), \
                    mock.patch("research.active.acceptance.execute") as execute_mock:
                socket_type.return_value.__enter__.return_value.getsockname.return_value = ("127.0.0.1", 55291)
                if isinstance(effect, Exception): execute_mock.side_effect = effect
                else: execute_mock.return_value = effect
                with self.assertRaises(RuntimeError): loopback("unused", request())

    def test_non_posix_runtime_is_explicitly_unsupported(self):
        with mock.patch("research.active.process.os", SimpleNamespace(name="nt")):
            result = run(request(), "/does/not/exist")
        self.assertEqual(result["status"], "unsupported")
        self.assertEqual(result["reason"], "posix_only")
        self.assertIsNone(result["measurement"])

    def test_connection_hosts_require_strings(self):
        for field in ("local_host", "remote_host"):
            for bad in (2130706433, True, None, ["127.0.0.1"], {"ip": "127.0.0.1"}):
                data = json.loads(fixture())
                data["start"]["connected"][0][field] = bad
                with self.subTest(field=field, bad=bad), self.assertRaises(Invalid):
                    parse_result(json.dumps(data).encode(), request())

    def test_real_fixtures_preserve_units_endpoint_roles_and_udp_unknowns(self):
        for protocol in ("tcp", "udp"):
            for direction in ("upload", "download"):
                with self.subTest(protocol=protocol, direction=direction):
                    value = parse_result(fixture(protocol, direction), request(protocol=protocol, direction=direction))
                    sender, receiver = value.transfers
                    self.assertEqual(sender.endpoint, "client" if direction == "upload" else "server")
                    self.assertNotEqual(sender.endpoint, receiver.endpoint)
                    self.assertEqual(sender.bits_per_second / 1000000, sender.megabits_per_second)
                    self.assertEqual(sender.bytes, receiver.bytes)
                    self.assertIsNone(sender.jitter_ms)
                    if protocol == "udp":
                        self.assertEqual(receiver.lost_packets, 0)
                        self.assertGreater(receiver.jitter_ms, 0)
                        self.assertIsNone(sender.lost_percent)

    def test_request_validation_and_no_argv_injection(self):
        for field, bad in (("schema_version", 1), ("duration_s", True), ("duration_s", 3),
                           ("port", 0), ("port", 65536), ("streams", 0), ("streams", 5),
                           ("offered_rate_bps", 10000001), ("offered_rate_bps", 0),
                           ("target_ip", "localhost; touch nope"), ("target_ip", "--server"),
                           ("target_ip", 1), ("target_ip", True), ("target_ip", "255.255.255.255"),
                           ("target_ip", "0.0.0.0"), ("target_ip", "224.0.0.1"),
                           ("target_class", "remote"), ("authorization", "implicit"),
                           ("request_id", "../bad")):
            with self.subTest(field=field, bad=bad), self.assertRaises(Invalid): request(**{field: bad})
        with self.assertRaises(Invalid): Request.decode(b'{"schema_version":"1","argv":["--server"]}')
        with self.assertRaises(Invalid): Request.decode(b" " * 4097)
        args = request().argv("/trusted/iperf3")
        self.assertEqual(args[args.index("--bitrate") + 1], "2000000")
        self.assertNotIn("--server", args)
        self.assertEqual(Request.decode(json.dumps(asdict(request())).encode()), request())

    def test_unsupported_topology_modes_never_execute(self):
        cases = [request(direction="bidirectional"), request(protocol="quic"), request(target_ip="::1"), request(streams=4),
                 request(target_ip="192.168.1.10", target_class="lan", authorization="authenticated_agent_required"),
                 request(target_ip="8.8.8.8", target_class="remote", authorization="authenticated_agent_required")]
        for req in cases:
            result = run(req, "/does/not/exist")
            self.assertEqual(result["status"], "unsupported")
            self.assertIsNone(result["measurement"])
            self.assertIsNone(result["process_window"])
            self.assertEqual(result["wifi_attribution"], "not_established")

    def test_malformed_duplicate_nonfinite_overflow_and_resource_limits(self):
        for raw in (b"{", b"[]", b"{}{}", b'\xff', b'{"a":1,"a":2}',
                    b'{"a":{"x":1,"x":2}}', b'{"a":NaN}', b'{"a":Infinity}',
                    b'{"a":1e999}', b'{"a":9007199254740992}',
                    b'{"a":' + b'1' * 5000 + b'}', b'{"a":' + b'[' * 100 + b'0' + b']' * 100 + b'}',
                    b" " * (MAX_JSON + 1)):
            with self.subTest(raw=raw[:50]), self.assertRaises(Invalid): strict_json(raw)

    def test_missing_partial_source_versions_and_forged_attribution(self):
        mutations = [("start", "version", "iperf 3.21"), ("start", "system_info", {}),
                     ("start", "test_start", {}), ("start", "connected", []),
                     ("end", "sum_sent", {}), ("end", "sum_received", None)]
        for section, key, value in mutations:
            raw = json.loads(fixture()); raw[section][key] = value
            with self.subTest(key=key), self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request())
        for field, value in (("protocol", "UDP"), ("num_streams", 2), ("duration", 2), ("omit", 1),
                             ("reverse", 1), ("target_bitrate", 1), ("bidir", 1), ("bytes", 10)):
            raw = json.loads(fixture()); raw["start"]["test_start"][field] = value
            with self.subTest(field=field), self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request())
        for field, value in (("remote_host", "192.168.1.1"), ("local_host", "192.168.1.2"), ("remote_port", 1)):
            raw = json.loads(fixture()); raw["start"]["connected"][0][field] = value
            with self.subTest(field=field), self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request())
        raw = json.loads(fixture()); raw["error"] = "interrupted"
        with self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request())
        raw = json.loads(fixture()); del raw["end"]["sum_received"]
        with self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request())

    def test_impossible_counters_and_incomplete_windows(self):
        for field, value in (("seconds", 0), ("seconds", .1), ("bytes", -1), ("bytes", True),
                             ("bits_per_second", 0), ("sender", False), ("end", 3), ("start", .1)):
            raw = json.loads(fixture()); raw["end"]["sum_sent"][field] = value
            with self.subTest(field=field), self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request())
        raw = json.loads(fixture("udp")); raw["end"]["sum_received"]["lost_packets"] = 999
        with self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request(protocol="udp"))

    def test_udp_gap_estimator_uses_sender_denominator_and_preserves_semantics(self):
        # Synthetic: 210 sent, highest sequence 200, 10 internal gaps, 190 arrivals.
        # The last 10 missing datagrams do not enter iperf's sequence-gap estimator.
        for direction in ("upload", "download"):
            raw = json.loads(fixture("udp", direction))
            row = raw["end"]["sum_received"]
            row.update(packets=200, lost_packets=10, lost_percent=100 * 10 / 210,
                       bytes=190 * 1200, bits_per_second=190 * 1200 * 8 / row["seconds"])
            sender, receiver = parse_result(json.dumps(raw).encode(), request(protocol="udp", direction=direction)).transfers
            self.assertEqual(receiver.packets, 200)
            self.assertEqual(receiver.packet_count_semantics, "highest_sequence_seen")
            self.assertEqual(sender.packet_count_semantics, "sent_datagrams")
            self.assertEqual(receiver.loss_denominator_packets, 210)
            self.assertEqual(receiver.loss_semantics, "iperf_sequence_gap_estimate")
            self.assertAlmostEqual(receiver.lost_percent, 100 * 10 / 210)
            self.assertEqual(receiver.received_datagrams, 190)
            self.assertEqual(receiver.source_lost_packets, 10)

    def test_udp_duplicates_and_absent_timing_evidence(self):
        raw = json.loads(fixture("udp"))
        row = raw["end"]["sum_received"]
        # Duplicate arrivals are included in bytes, and may exceed packets sent.
        row.update(bytes=211 * 1200, bits_per_second=211 * 1200 * 8 / row["seconds"])
        receiver = parse_result(json.dumps(raw).encode(), request(protocol="udp")).transfers[1]
        self.assertEqual(receiver.received_datagrams, 211)
        for arrivals in (0, 1):
            row.update(bytes=arrivals * 1200, bits_per_second=arrivals * 1200 * 8 / row["seconds"],
                       packets=arrivals, lost_packets=0, lost_percent=0, jitter_ms=0)
            receiver = parse_result(json.dumps(raw).encode(), request(protocol="udp")).transfers[1]
            self.assertIsNone(receiver.jitter_ms)
            self.assertEqual(receiver.source_jitter_ms, 0)
            if arrivals == 0:
                self.assertIsNone(receiver.lost_percent)
                self.assertIsNone(receiver.lost_packets)
                self.assertEqual(receiver.source_lost_percent, 0)

    def test_udp_rejects_wrong_payload_and_contradictory_counters(self):
        for value in (1, 1200.0, True, None):
            raw = json.loads(fixture("udp")); raw["start"]["test_start"]["blksize"] = value
            with self.subTest(blksize=value), self.assertRaises(Invalid):
                parse_result(json.dumps(raw).encode(), request(protocol="udp"))
        cases = [("sum_sent", {"packets": 209}),
                 ("sum_sent", {"lost_packets": 1, "lost_percent": 100 / 210}),
                 ("sum_sent", {"jitter_ms": 1}),
                 ("sum_received", {"bytes": 0, "packets": 1}),
                 ("sum_received", {"bytes": 1201}),
                 ("sum_received", {"bytes": 1200, "packets": 0}),
                 ("sum_received", {"bytes": 1200, "packets": 200, "lost_packets": 10, "lost_percent": 100 * 10 / 210}),
                 ("sum_received", {"packets": 211}),
                 ("sum_received", {"packets": 10, "lost_packets": 90, "lost_percent": 100 * 90 / 210}),
                 ("sum_received", {"bytes": 1200, "packets": 1, "jitter_ms": 1})]
        for key, changes in cases:
            raw = json.loads(fixture("udp")); row = raw["end"][key]; row.update(changes)
            row["bits_per_second"] = row["bytes"] * 8 / row["seconds"]
            with self.subTest(key=key, changes=changes), self.assertRaises(Invalid):
                parse_result(json.dumps(raw).encode(), request(protocol="udp"))

    def test_evidence_only_server_duplicate_exception_is_narrow(self):
        raw = b'{"start":{"target_bitrate":2000000,"target_bitrate":2000000}}'
        with self.assertRaises(Invalid): strict_json(raw)
        self.assertEqual(server_summary(raw, request())["start"]["target_bitrate"], 2000000)
        for bad in (b'{"start":{"target_bitrate":2000000,"target_bitrate":2000001}}',
                    b'{"end":{"target_bitrate":2000000,"target_bitrate":2000000}}',
                    b'{"start":{"target_bitrate":2000000,"target_bitrate":2000000,"target_bitrate":2000000}}',
                    b'{"start":{"target_bitrate":1,"target_bitrate":1}}',
                    b'{"start":{"version":"3.20","version":"3.20"}}'):
            with self.subTest(raw=bad), self.assertRaises(Invalid): server_summary(bad, request())
        raw = json.loads(fixture("udp")); raw["end"]["sum_received"]["lost_percent"] = 99
        with self.assertRaises(Invalid): parse_result(json.dumps(raw).encode(), request(protocol="udp"))


@unittest.skipUnless(os.name == "posix" and hasattr(os, "fork"), "POSIX supervisor lifecycle tests")
class FakeProcessTests(unittest.TestCase):
    """Fake executables exercise supervision, never live network validation."""
    @classmethod
    def setUpClass(cls):
        cls.directory = ROOT / ".tools/active-tests" / str(uuid.uuid4())
        cls.directory.mkdir(parents=True)

    def fake(self, body):
        path = self.directory / (str(uuid.uuid4()) + ".py")
        path.write_text("#!" + sys.executable + "\n" + body)
        path.chmod(0o700)
        return path

    def test_exit_crash_and_unavailable_are_not_throughput(self):
        for code in ("raise SystemExit(7)", "import os,signal; os.kill(os.getpid(),signal.SIGKILL)"):
            value = execute([sys.executable, "-c", code], 1)
            self.assertEqual(value.status, "process_error")
            self.assertNotEqual(value.returncode, 0)
        self.assertEqual(run(request(), "/does/not/exist")["status"], "unavailable")

    def test_cleanup_failure_is_structured_and_cannot_report_success(self):
        from research.active.process import _stop
        def failed_cleanup(child):
            _stop(child)  # Reap the real test child before injecting the failure.
            raise PermissionError("synthetic group cleanup denial")
        with mock.patch("research.active.process._stop", side_effect=failed_cleanup):
            value = execute([sys.executable, "-c", "print('ok')"], 1)
        self.assertEqual(value.status, "process_error")
        self.assertIn("synthetic group cleanup denial", value.cleanup_error)

    def test_successful_parent_cannot_leave_a_background_descendant(self):
        pidfile = self.directory / (str(uuid.uuid4()) + ".pid")
        escaped = self.directory / (str(uuid.uuid4()) + ".escaped")
        code = ("import os,time\nfrom pathlib import Path\n"
                "ready_read,ready_write=os.pipe()\npid=os.fork()\n"
                "if pid==0:\n"
                "    os.close(ready_read)\n"
                "    os.close(0);os.close(1);os.close(2)\n"
                "    os.write(ready_write,b'1');os.close(ready_write)\n"
                "    time.sleep(0.2)\n"
                "    Path(" + repr(str(escaped)) + ").write_text('descendant outlived supervisor')\n"
                "    time.sleep(5)\n    os._exit(0)\n"
                "os.close(ready_write);os.read(ready_read,1);os.close(ready_read)\n"
                "Path(" + repr(str(pidfile)) + ").write_text(str(pid))\nos._exit(0)\n")
        try:
            result = execute([sys.executable, "-c", code], 1)
            self.assertEqual(result.status, "completed")
            time.sleep(0.5)
            self.assertFalse(escaped.exists(), "background descendant executed after terminal success")
        finally:
            # Only the PID created by this fixture; retain every test artifact.
            if pidfile.exists():
                try: os.kill(int(pidfile.read_text()), signal.SIGKILL)
                except ProcessLookupError: pass

    def test_timeout_cancel_and_recovery(self):
        child = [sys.executable, "-c", "import time; time.sleep(10)"]
        began = time.monotonic()
        self.assertEqual(execute(child, .1).status, "timeout")
        event = threading.Event(); timer = threading.Timer(.1, event.set); timer.start()
        try: self.assertEqual(execute(child, 2, event).status, "cancelled")
        finally: timer.join()
        self.assertLess(time.monotonic() - began, 2)
        self.assertEqual(execute([sys.executable, "-c", "print('ok')"], 1).stdout, b"ok\n")
        self.assertEqual(execute(child, 1, event).status, "cancelled")

    def test_bounded_stdout_stderr_and_inherited_pipe(self):
        for fd, limit in ((1, MAX_JSON), (2, MAX_STDERR)):
            value = execute([sys.executable, "-c", "import os; os.write(" + str(fd) + ",b'x'*1000000)"], 1)
            self.assertEqual(value.status, "output_limit")
            self.assertLessEqual(len(value.stdout if fd == 1 else value.stderr), limit)
        child = "import os,time; pid=os.fork(); time.sleep(5) if pid==0 else os._exit(0)"
        self.assertEqual(execute([sys.executable, "-c", child], .15).status, "timeout")

    def test_version_probe_and_partial_output_failures(self):
        wrong = self.fake("print('iperf 3.99 (cJSON 1.7.15)')\n")
        self.assertEqual(run(request(), wrong)["status"], "unsupported")
        partial = self.fake("import sys\nif '--version' in sys.argv: print('iperf 3.20 (cJSON 1.7.15)\\nFake test executable')\nelse: print('{}')\n")
        value = run(request(), partial)
        self.assertEqual(value["status"], "invalid_output")
        self.assertIsNone(value["measurement"])
        self.assertGreaterEqual(value["process_window"]["end_ns"], value["process_window"]["start_ns"])
        failure = self.fake("import sys\nif '--version' in sys.argv: print('iperf 3.20 (cJSON 1.7.15)\\nFake test executable')\nelse: raise SystemExit(2)\n")
        value = run(request(), failure)
        self.assertEqual(value["status"], "process_error")
        self.assertIsNone(value["measurement"])


if __name__ == "__main__": unittest.main()
