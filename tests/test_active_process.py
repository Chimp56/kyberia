"""Deterministic parser tests and explicitly fake child-process lifecycle tests."""
import ctypes
from dataclasses import asdict
import io
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
from research.active.process import Execution, MAX_STDERR, execute, run
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
        if os.name == "nt":
            child.poll.return_value = None
            # Mock creates unknown attributes on access.  An actual Popen
            # instance has no Kyberia job attribute until the supervisor has
            # attached one, so model the direct-kill fallback explicitly.
            child._kyberia_windows_job = None
            _stop(child)
            child.kill.assert_called_once_with()
            child.wait.assert_called_once_with(timeout=2)
            return
        with mock.patch("research.active.process.os.killpg", side_effect=[PermissionError("zombie"), ProcessLookupError()]) as kill:
            _stop(child)
        self.assertEqual(kill.call_args_list, [mock.call(12345, signal.SIGKILL), mock.call(12345, 0)])
        child.wait.assert_called_with(timeout=2)
        for probe in (None, PermissionError("still denied")):
            with mock.patch("research.active.process.os.killpg", side_effect=[PermissionError("denied"), probe]):
                with self.assertRaises(PermissionError): _stop(child)

    def test_windows_stop_terminates_the_owned_job_tree(self):
        from research.active import process
        child = mock.Mock()
        child.poll.return_value = None
        job = mock.Mock()
        child._kyberia_windows_job = job
        with mock.patch.object(process.os, "name", "nt"):
            process._stop(child)
        job.terminate.assert_called_once_with()
        child.wait.assert_called_once_with(timeout=2)
        child.kill.assert_not_called()

    def test_windows_stop_terminates_descendants_after_parent_exit(self):
        from research.active import process

        child = mock.Mock()
        child.poll.return_value = 0
        job = mock.Mock()
        child._kyberia_windows_job = job
        with mock.patch.object(process.os, "name", "nt"):
            process._stop(child)
        job.terminate.assert_called_once_with()
        child.wait.assert_called_once_with(timeout=2)
        child.kill.assert_not_called()

    def test_windows_hostile_stdout_cannot_starve_stderr_or_terminal_events(self):
        """Each data stream is bounded independently from terminal signalling."""
        from research.active import process

        events = {"stdout": process._PipeEvents(process._WINDOW_PIPE_STDOUT_EVENTS),
                  "stderr": process._PipeEvents(process._WINDOW_PIPE_STDERR_EVENTS)}
        stop_event = threading.Event()
        streams = ((b"x" * (MAX_JSON + 2 * MAX_STDERR), "stdout"),
                   (b"x" * MAX_STDERR, "stderr"))
        threads = []
        try:
            for payload, label in streams:
                reader = threading.Thread(target=process._pipe_reader,
                                           args=(io.BytesIO(payload), label, events[label], stop_event),
                                           daemon=True)
                reader.start()
                threads.append(reader)
            threads[1].join(timeout=1)
            self.assertFalse(threads[1].is_alive())
            self.assertTrue(threads[0].is_alive(),
                            "hostile stdout should block only on its own data quota")
        finally:
            stop_event.set()
            for thread in threads:
                thread.join(timeout=1)
        output, eof = {"stdout": bytearray(), "stderr": bytearray()}, set()
        process._consume_pipe_events(events, output, eof, MAX_JSON)
        self.assertEqual(len(output["stdout"]), MAX_JSON)
        self.assertEqual(len(output["stderr"]), MAX_STDERR)
        self.assertIn("stderr", eof)

    def test_windows_stop_escalates_after_job_and_reap_failures(self):
        from research.active import process
        child = mock.Mock()
        child.poll.side_effect = [None, None, None]
        child.wait.side_effect = [TimeoutError("initial wait failed"),
                                  TimeoutError("retry wait failed")]
        child.kill.side_effect = OSError("direct kill failed")
        job = mock.Mock()
        job.terminate.side_effect = OSError("job terminate failed")
        child._kyberia_windows_job = job
        with mock.patch.object(process.os, "name", "nt"), \
                self.assertRaisesRegex(OSError, "containment unknown"):
            process._stop(child)
        job.terminate.assert_called_once_with()
        child.kill.assert_called_once_with()
        self.assertEqual(child.wait.call_args_list,
                         [mock.call(timeout=2), mock.call(timeout=2)])

    def test_windows_stop_retries_direct_kill_and_reports_prior_failure(self):
        from research.active import process
        child = mock.Mock()
        order = []
        polls = iter((None, None, 0))
        waits = iter((TimeoutError("initial wait failed"), None))

        def poll():
            order.append("poll")
            return next(polls)

        def wait(timeout=None):
            order.append("wait")
            result = next(waits)
            if isinstance(result, BaseException):
                raise result

        def kill():
            order.append("kill")

        child.poll.side_effect = poll
        child.wait.side_effect = wait
        child.kill.side_effect = kill
        job = mock.Mock()
        def terminate():
            order.append("job.terminate")
            raise OSError("job terminate failed")
        job.terminate.side_effect = terminate
        child._kyberia_windows_job = job
        with mock.patch.object(process.os, "name", "nt"), \
                self.assertRaisesRegex(OSError, "termination/reap failed") as raised:
            process._stop(child)
        job.terminate.assert_called_once_with()
        child.kill.assert_called_once_with()
        self.assertEqual(child.wait.call_args_list,
                         [mock.call(timeout=2), mock.call(timeout=2)])
        self.assertEqual(order, ["poll", "job.terminate", "wait", "poll", "kill", "wait", "poll"])
        self.assertNotIn("containment unknown", str(raised.exception))

    def test_windows_job_close_retry_distinguishes_proven_containment(self):
        from research.active import process
        job = mock.Mock(_closed=False)
        calls = [0]

        def close_once():
            calls[0] += 1
            if calls[0] == 1:
                raise OSError("transient close failure")
            job._closed = True

        job.close.side_effect = close_once
        errors = []
        process._close_windows_job(job, errors)
        self.assertEqual(job.close.call_count, 2)
        self.assertEqual(len(errors), 1)
        self.assertNotIn("containment unknown", str(errors[0]))

        job = mock.Mock(_closed=False, _handle=None, _owned_handles={"thread": 33})
        job.close.side_effect = OSError("persistent close failure")
        errors = []
        process._close_windows_job(job, errors)
        self.assertEqual(job.close.call_count, 2)
        self.assertTrue(any("auxiliary native handle cleanup unresolved" in str(error)
                            and "thread" in str(error)
                            for error in errors))
        self.assertFalse(any("Job Object handle remains open" in str(error)
                             for error in errors))

    @staticmethod
    def fake_windows_job(kernel):
        from research.active import process
        job = object.__new__(process._WindowsJobObject)
        job._kernel32 = kernel
        job._handle = 99
        job._closed = False
        job._process_assigned = False
        job._owned_handles = {}
        return job

    @staticmethod
    def fake_thread_entry(process, thread_id, process_id):
        def fill(_snapshot, pointer):
            entry = ctypes.cast(pointer, ctypes.POINTER(process._WindowsThreadEntry32)).contents
            entry.th32ThreadID = thread_id
            entry.th32OwnerProcessID = process_id
            return 1
        return fill

    def test_windows_thread_discovery_rejects_missing_multiple_and_unopenable(self):
        from research.active import process
        for mode in ("missing", "multiple", "unopenable"):
            kernel = mock.Mock()
            kernel.CreateToolhelp32Snapshot.return_value = 10
            kernel.CloseHandle.return_value = 1
            if mode == "missing":
                kernel.Thread32First.return_value = 0
                last_error = [18]
            else:
                kernel.Thread32First.side_effect = self.fake_thread_entry(process, 22, 700)
                if mode == "multiple":
                    seen = [False]
                    def next_entry(_snapshot, pointer):
                        if not seen[0]:
                            seen[0] = True
                            return self.fake_thread_entry(process, 23, 700)(_snapshot, pointer)
                        return 0
                    kernel.Thread32Next.side_effect = next_entry
                else:
                    kernel.Thread32Next.return_value = 0
                kernel.OpenThread.return_value = 0 if mode == "unopenable" else 33
                last_error = [18, 5] if mode == "unopenable" else [18]
            job = self.fake_windows_job(kernel)
            expected_error = {"missing": "no thread", "multiple": "multiple", "unopenable": "OpenThread"}[mode]
            with self.subTest(mode=mode), mock.patch.object(
                    process.ctypes, "get_last_error", side_effect=last_error, create=True):
                with self.assertRaisesRegex(OSError, expected_error):
                    job._open_suspended_thread(700)
            kernel.CloseHandle.assert_called_once_with(10)

    def test_windows_auxiliary_handle_close_failures_retry_without_reuse(self):
        from research.active import process

        kernel = mock.Mock()
        kernel.CreateToolhelp32Snapshot.return_value = 10
        kernel.Thread32First.side_effect = self.fake_thread_entry(process, 22, 700)
        kernel.Thread32Next.return_value = 0
        kernel.CloseHandle.side_effect = [0, 1, 1]
        job = self.fake_windows_job(kernel)
        with mock.patch.object(process.ctypes, "get_last_error", return_value=18, create=True), \
                self.assertRaisesRegex(OSError, "CloseHandle\\(thread snapshot\\)"):
            job._open_suspended_thread(700)
        self.assertEqual(job._owned_handles, {"thread snapshot": 10})
        job.close()
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(10), mock.call(10), mock.call(99)])
        self.assertFalse(job._owned_handles)
        call_count = kernel.CloseHandle.call_count
        job.close()
        self.assertEqual(kernel.CloseHandle.call_count, call_count)

        kernel = mock.Mock()
        kernel.OpenProcess.return_value = 44
        kernel.AssignProcessToJobObject.return_value = 1
        kernel.ResumeThread.return_value = 1
        kernel.CloseHandle.side_effect = [0, 1]
        job = self.fake_windows_job(kernel)
        job._own_handle("thread", 33)
        child = mock.Mock(pid=700)
        with mock.patch.object(job, "_open_suspended_thread", return_value=33), \
                self.assertRaisesRegex(OSError, "handle close"):
            job.assign_and_resume(child)
        self.assertEqual(job._owned_handles, {"process": 44})
        kernel.CloseHandle.side_effect = None
        kernel.CloseHandle.return_value = 1
        job.close()
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(44), mock.call(33), mock.call(44), mock.call(99)])
        self.assertFalse(job._owned_handles)

        kernel = mock.Mock()
        kernel.OpenProcess.return_value = 44
        kernel.AssignProcessToJobObject.return_value = 1
        kernel.ResumeThread.return_value = 1
        kernel.CloseHandle.side_effect = [1, 0, 1, 1]
        job = self.fake_windows_job(kernel)
        job._own_handle("thread", 33)
        with mock.patch.object(job, "_open_suspended_thread", return_value=33), \
                self.assertRaisesRegex(OSError, "handle close"):
            job.assign_and_resume(mock.Mock(pid=700))
        self.assertEqual(job._owned_handles, {"thread": 33})
        job.close()
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(44), mock.call(33), mock.call(33), mock.call(99)])
        self.assertFalse(job._owned_handles)

    def test_windows_resume_failure_and_close_failure_are_structured(self):
        from research.active import process
        kernel = mock.Mock()
        kernel.OpenProcess.return_value = 44
        kernel.AssignProcessToJobObject.return_value = 1
        kernel.ResumeThread.return_value = process._WAIT_FAILED
        kernel.CloseHandle.return_value = 1
        job = self.fake_windows_job(kernel)
        child = mock.Mock(pid=700, _handle=44)
        kernel.AssignProcessToJobObject.return_value = 0
        with mock.patch.object(job, "_open_suspended_thread", return_value=32), \
                mock.patch.object(process.ctypes, "get_last_error", return_value=5, create=True), \
                self.assertRaisesRegex(OSError, "AssignProcessToJobObject"):
            job.assign_and_resume(child)
        self.assertFalse(job._process_assigned)
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(44), mock.call(32)])

        kernel.reset_mock()
        kernel.OpenProcess.return_value = 44
        kernel.AssignProcessToJobObject.return_value = 1
        with mock.patch.object(job, "_open_suspended_thread", return_value=33), \
                mock.patch.object(process.ctypes, "get_last_error", return_value=5, create=True), \
                self.assertRaisesRegex(OSError, "ResumeThread"):
            job.assign_and_resume(child)
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(44), mock.call(33)])

        kernel.CloseHandle.return_value = 0
        with mock.patch.object(job, "_open_suspended_thread", return_value=34), \
                mock.patch.object(process.ctypes, "get_last_error", return_value=6, create=True), \
                self.assertRaisesRegex(OSError, "handle close"):
            job.assign_and_resume(child)

    def test_windows_job_close_failure_does_not_mark_closed(self):
        from research.active import process
        kernel = mock.Mock()
        kernel.CloseHandle.return_value = 0
        job = self.fake_windows_job(kernel)
        with mock.patch.object(process.ctypes, "get_last_error", return_value=5, create=True), \
                self.assertRaises(OSError):
            job.close()
        self.assertFalse(job._closed)
        self.assertEqual(job._handle, 99)

    def test_windows_job_constructor_retains_handle_after_close_failure(self):
        from research.active import process
        kernel = mock.Mock()
        kernel.CreateJobObjectW.return_value = 77
        kernel.SetInformationJobObject.return_value = 0
        kernel.CloseHandle.return_value = 0
        with mock.patch.object(process.os, "name", "nt"), \
                mock.patch.object(process.ctypes, "WinDLL", return_value=kernel, create=True), \
                mock.patch.object(process.ctypes, "get_last_error", return_value=5, create=True):
            job = process._WindowsJobObject()
        self.assertEqual(job._handle, 77)
        self.assertFalse(job._closed)
        self.assertIn("SetInformationJobObject", str(job._initialization_error))
        self.assertIn("job handle close failed", str(job._initialization_error))
        kernel.CloseHandle.return_value = 1
        job.close()
        self.assertTrue(job._closed)
        self.assertIsNone(job._handle)
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(77), mock.call(77)])

    def test_windows_attach_cleanup_failures_are_not_suppressed(self):
        from research.active import process
        child = mock.Mock()
        job = mock.Mock()
        job.assign_and_resume.side_effect = OSError("resume setup failed")
        job.terminate.side_effect = OSError("terminate failed")
        job.close.side_effect = OSError("close failed")
        child.kill.side_effect = OSError("kill failed")
        child.wait.side_effect = TimeoutError("wait failed")
        with mock.patch.object(process, "_WindowsJobObject", return_value=job), \
                self.assertRaisesRegex(OSError, "resume setup failed.*cleanup failures"):
            process._attach_windows_job(child)
        job.terminate.assert_called_once_with()
        self.assertEqual(job.close.call_count, 2)
        self.assertEqual(child.kill.call_count, 2)
        self.assertEqual(child.wait.call_count, 2)

    def test_windows_output_consumer_limits_exact_and_over_bound(self):
        from research.active import process
        for label, bound in (("stdout", MAX_JSON), ("stderr", MAX_STDERR)):
            for size in (bound, bound + 1):
                events = {"stdout": process._PipeEvents(process._WINDOW_PIPE_STDOUT_EVENTS),
                          "stderr": process._PipeEvents(process._WINDOW_PIPE_STDERR_EVENTS)}
                events[label].data.put(("data", label, b"x" * size))
                output = {"stdout": bytearray(), "stderr": bytearray()}
                status = process._consume_pipe_events(events, output, set(), MAX_JSON)
                self.assertEqual(status, "output_limit")
                self.assertEqual(len(output[label]), bound)

    def test_windows_blocked_read_and_write_are_joined_after_close(self):
        from research.active import process
        released = threading.Event()

        class BlockingRead:
            def read(self, _size):
                released.wait(2)
                return b""

            def close(self):
                released.set()

        stop_event = threading.Event()
        events = {"stdout": process._PipeEvents(process._WINDOW_PIPE_STDOUT_EVENTS),
                  "stderr": process._PipeEvents(process._WINDOW_PIPE_STDERR_EVENTS)}
        reader = threading.Thread(target=process._pipe_reader,
                                   args=(BlockingRead(), "stdout", events["stdout"], stop_event), daemon=True)
        reader2 = threading.Thread(target=process._pipe_reader,
                                    args=(BlockingRead(), "stderr", events["stderr"], stop_event), daemon=True)
        reader.start(); reader2.start()
        errors = process._join_pipe_threads([("stdout", reader), ("stderr", reader2)])
        self.assertEqual(len(errors), 2)
        stop_event.set()
        released.set()
        self.assertFalse(process._join_pipe_threads([("stdout", reader), ("stderr", reader2)]))

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
        cls.directory = ROOT / ".trash/test-runs" / ("active-process-" + str(uuid.uuid4()))
        cls.directory.mkdir(parents=True)

    def fake(self, body):
        path = self.directory / (str(uuid.uuid4()) + ".py")
        path.write_text("#!" + sys.executable + "\n" + body)
        path.chmod(0o700)
        return path

    @staticmethod
    def execution_message(value):
        return (
            f"status={value.status!r}, returncode={value.returncode!r}, "
            f"stdout_bytes={len(value.stdout)}, stderr_bytes={len(value.stderr)}, "
            f"cleanup_error={value.cleanup_error!r}"
        )

    @staticmethod
    def result_message(value):
        return f"result={value!r}"

    @staticmethod
    def completed_execution(stdout):
        now = time.monotonic_ns()
        return Execution("completed", 0, stdout, b"", now, now + 1, time.time_ns())

    def stable_version_probe(self, argv, timeout_s, cancel=None, stdout_limit=MAX_JSON):
        if argv[-1] == "--version":
            return self.completed_execution(b"iperf 3.20 (cJSON 1.7.15)\nFake test executable\n")
        return execute(argv, timeout_s, cancel, stdout_limit)

    def test_exit_crash_and_unavailable_are_not_throughput(self):
        for code in ("raise SystemExit(7)", "import os,signal; os.kill(os.getpid(),signal.SIGKILL)"):
            value = execute([sys.executable, "-c", code], 1)
            self.assertEqual(value.status, "process_error", msg=self.execution_message(value))
            self.assertNotEqual(value.returncode, 0, msg=self.execution_message(value))
        value = run(request(), "/does/not/exist")
        self.assertEqual(value["status"], "unavailable", msg=self.result_message(value))

    def test_cleanup_failure_is_structured_and_cannot_report_success(self):
        from research.active.process import _stop
        def failed_cleanup(child):
            _stop(child)  # Reap the real test child before injecting the failure.
            raise PermissionError("synthetic group cleanup denial")
        with mock.patch("research.active.process._stop", side_effect=failed_cleanup):
            value = execute([sys.executable, "-c", "print('ok')"], 1)
        self.assertEqual(value.status, "process_error", msg=self.execution_message(value))
        self.assertIn("synthetic group cleanup denial", value.cleanup_error, msg=self.execution_message(value))

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
            self.assertEqual(result.status, "completed", msg=self.execution_message(result))
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
        value = execute(child, .1)
        self.assertEqual(value.status, "timeout", msg=self.execution_message(value))
        event = threading.Event(); timer = threading.Timer(.1, event.set); timer.start()
        try:
            value = execute(child, 2, event)
            self.assertEqual(value.status, "cancelled", msg=self.execution_message(value))
        finally: timer.join()
        self.assertLess(time.monotonic() - began, 2)
        value = execute([sys.executable, "-c", "print('ok')"], 1)
        self.assertEqual(value.stdout, b"ok\n", msg=self.execution_message(value))
        value = execute(child, 1, event)
        self.assertEqual(value.status, "cancelled", msg=self.execution_message(value))

    def test_bounded_stdout_stderr_and_inherited_pipe(self):
        for fd, limit in ((1, MAX_JSON), (2, MAX_STDERR)):
            value = execute([sys.executable, "-c", "import os; os.write(" + str(fd) + ",b'x'*1000000)"], 1)
            self.assertEqual(value.status, "output_limit", msg=self.execution_message(value))
            self.assertLessEqual(len(value.stdout if fd == 1 else value.stderr), limit, msg=self.execution_message(value))

    def test_posix_exact_output_limits_are_terminal(self):
        for fd, bound in ((1, MAX_JSON), (2, MAX_STDERR)):
            value = execute([sys.executable, "-c", "import os; os.write(%d,b'x'*%d)" % (fd, bound)], 1)
            self.assertEqual(value.status, "output_limit", msg=self.execution_message(value))
            self.assertEqual(len(value.stdout if fd == 1 else value.stderr), bound,
                             msg=self.execution_message(value))
        child = "import os,time; pid=os.fork(); time.sleep(5) if pid==0 else os._exit(0)"
        value = execute([sys.executable, "-c", child], .15)
        self.assertEqual(value.status, "timeout", msg=self.execution_message(value))

    def test_version_probe_and_partial_output_failures(self):
        wrong = self.fake("print('iperf 3.99 (cJSON 1.7.15)')\n")
        value = run(request(), wrong)
        self.assertEqual(value["status"], "unsupported", msg=self.result_message(value))
        partial = self.fake("")
        with mock.patch("research.active.process.execute", side_effect=[
            self.completed_execution(b"iperf 3.20 (cJSON 1.7.15)\nFake test executable\n"),
            self.completed_execution(b"{}\n"),
        ]) as executor:
            value = run(request(), partial)
        self.assertEqual(executor.call_count, 2, msg=self.result_message(value))
        self.assertEqual(value["status"], "invalid_output", msg=self.result_message(value))
        self.assertIsNone(value["measurement"], msg=self.result_message(value))
        self.assertGreaterEqual(value["process_window"]["end_ns"], value["process_window"]["start_ns"], msg=self.result_message(value))
        failure = self.fake("import sys\nif '--version' in sys.argv: print('iperf 3.20 (cJSON 1.7.15)\\nFake test executable')\nelse: raise SystemExit(2)\n")
        with mock.patch("research.active.process.execute", side_effect=self.stable_version_probe):
            value = run(request(), failure)
        self.assertEqual(value["status"], "process_error", msg=self.result_message(value))
        self.assertIsNone(value["measurement"], msg=self.result_message(value))

    def test_real_client_malformed_output_remains_covered(self):
        partial = self.fake("import sys\nif '--version' in sys.argv: print('iperf 3.20 (cJSON 1.7.15)\\nFake test executable')\nelse: print('{}')\n")
        with mock.patch("research.active.process.execute", side_effect=self.stable_version_probe):
            value = run(request(), partial)
        self.assertEqual(value["status"], "invalid_output", msg=self.result_message(value))
        self.assertIsNone(value["measurement"], msg=self.result_message(value))

    def test_version_probe_timeout_is_reported_at_probe_boundary(self):
        slow = self.fake(
            "import sys,time\n"
            "if '--version' in sys.argv: time.sleep(2.2); print('iperf 3.20 (cJSON 1.7.15)')\n"
            "else: print('{}')\n"
        )
        value = run(request(), slow)
        self.assertEqual(value["status"], "timeout", msg=self.result_message(value))
        self.assertEqual(value["reason"], "version_probe_failed", msg=self.result_message(value))
        self.assertIsNone(value["measurement"], msg=self.result_message(value))


@unittest.skipUnless(os.name == "nt", "Windows job-object descendant contract")
class WindowsExecutionTests(unittest.TestCase):
    def test_windows_job_object_kills_descendants_after_parent_exit(self):
        directory = ROOT / ".trash/test-runs" / ("active-windows-job-" + str(time.time_ns()))
        directory.mkdir(parents=True)
        marker = directory / "escaped.txt"
        descendant = ("from pathlib import Path; import time; time.sleep(.4); "
                      "Path(" + repr(str(marker)) + ").write_text('descendant escaped')")
        program = ("import subprocess,sys; subprocess.Popen([sys.executable,'-c',"
                   + repr(descendant) + "]); print('parent-exited')")
        result = execute([sys.executable, "-I", "-c", program], 1)
        self.assertEqual(result.status, "completed", result)
        self.assertEqual(result.stdout, b"parent-exited\n")
        time.sleep(.7)
        self.assertFalse(marker.exists(), "Windows job close failed to terminate descendant")


if __name__ == "__main__": unittest.main()
