"""Contract/lifecycle tests run without Sionna; real engine proof is separate."""

from copy import deepcopy
import ctypes
import io
import json
import os
import queue
import signal
import subprocess
from pathlib import Path
import sys
import threading
import time
import tempfile
import unittest
import venv
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "workers/sionna"))
from rfatlas_sionna.contract import (ContractError, MAX_LOG_BYTES, MAX_REQUEST_BYTES,
                                     MAX_RESULT_BYTES, canonical_bytes, decode, digest, validate,
                                     validate_result)
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

    def test_windows_pipe_supervisor_contract(self):
        """Windows anonymous pipes use the threaded transport, not select()."""
        from rfatlas_sionna import client

        class Process:
            def __init__(self):
                class Input(io.BytesIO):
                    def close(self):
                        self.closed_by_supervisor = True

                self.stdin = Input()
                self.stdin.closed_by_supervisor = False
                self.stdout = io.BytesIO(b"result")
                self.stderr = io.BytesIO(b"diagnostic")
                self.returncode = 0
                self.killed = False

            def poll(self):
                return self.returncode

            def kill(self):
                self.killed = True

            def wait(self, timeout=None):
                return self.returncode

        process = Process()
        with mock.patch.object(client.os, "name", "nt"):
            result = client._supervise_windows(process, b"{}", 1, None)
        self.assertEqual(result["state"], "exited")
        self.assertEqual(result["stdout"], b"result")
        self.assertEqual(result["stderr"], b"diagnostic")
        self.assertEqual(process.stdin.getvalue(), b"{}")
        self.assertTrue(process.stdin.closed_by_supervisor)
        self.assertFalse(process.killed)

    def test_windows_pipe_drain_handles_early_exit_and_exact_limit(self):
        from rfatlas_sionna import client

        class DelayedStream(io.BytesIO):
            delay = True

            def read(self, size=-1):
                if self.delay:
                    time.sleep(0.01)
                return super().read(size)

        class Process:
            def __init__(self, stdout, stderr, delayed=True):
                self.stdin = io.BytesIO()
                self.stdout = DelayedStream(stdout)
                self.stderr = DelayedStream(stderr)
                self.stdout.delay = delayed
                self.stderr.delay = delayed
                self.returncode = 0

            def poll(self):
                # Exercise the early-poll path before reader threads publish.
                return self.returncode

            def wait(self, timeout=None):
                return self.returncode

        process = Process(b"late-result", b"late-log")
        with mock.patch.object(client.os, "name", "nt"):
            result = client._supervise_windows(process, b"{}", 1, None)
        self.assertEqual(result["state"], "exited")
        self.assertEqual(result["stdout"], b"late-result")
        self.assertEqual(result["stderr"], b"late-log")

        process = Process(b"x" * (MAX_RESULT_BYTES + 1), b"", delayed=False)
        with mock.patch.object(client.os, "name", "nt"):
            result = client._supervise_windows(process, b"{}", 1, None)
        self.assertEqual(result["state"], "output_limit")
        self.assertEqual(len(result["stdout"]), MAX_RESULT_BYTES)

        for label, bound in (("stdout", MAX_RESULT_BYTES), ("stderr", MAX_LOG_BYTES)):
            exact = {"stdout": b"", "stderr": b""}
            exact[label] = b"x" * bound
            process = Process(exact["stdout"], exact["stderr"], delayed=False)
            with mock.patch.object(client.os, "name", "nt"):
                result = client._supervise_windows(process, b"{}", 1, None)
            self.assertEqual(result["state"], "output_limit")
            self.assertEqual(len(result[label]), bound)

            over = {"stdout": b"", "stderr": b""}
            over[label] = b"x" * (bound + 1)
            process = Process(over["stdout"], over["stderr"], delayed=False)
            with mock.patch.object(client.os, "name", "nt"):
                result = client._supervise_windows(process, b"{}", 1, None)
            self.assertEqual(result["state"], "output_limit")
            self.assertEqual(len(result[label]), bound)

    def test_windows_pipe_reader_stops_after_consumer_shutdown(self):
        from rfatlas_sionna import client

        events = queue.Queue(maxsize=1)
        events.put(("data", "stdout", b"queued"))
        stop_event = threading.Event()
        started = threading.Event()

        class ReadOnce:
            def read(self, _size):
                started.set()
                return b"blocked behind full queue"

        reader = threading.Thread(target=client._pipe_reader,
                                  args=(ReadOnce(), "stdout", events, stop_event), daemon=True)
        reader.start()
        self.assertTrue(started.wait(1))
        stop_event.set()
        reader.join(timeout=1)
        self.assertFalse(reader.is_alive())

    def test_windows_cleanup_interrupts_blocked_read_and_write(self):
        from rfatlas_sionna import client

        class BlockingRead:
            def __init__(self):
                self.released = threading.Event()

            def read(self, _size):
                self.released.wait(2)
                return b""

            def close(self):
                self.released.set()

        class BlockingWrite:
            def __init__(self):
                self.released = threading.Event()

            def write(self, _payload):
                self.released.wait(2)
                return 0

            def flush(self):
                return None

            def close(self):
                self.released.set()

        class Process:
            returncode = 0

            def __init__(self):
                self.stdin = BlockingWrite()
                self.stdout = BlockingRead()
                self.stderr = BlockingRead()

            def poll(self):
                return self.returncode

            def wait(self, timeout=None):
                return self.returncode

        process = Process()
        with mock.patch.object(client.os, "name", "nt"):
            result = client._supervise_windows(process, b"{}", 1, None)
        self.assertEqual(result["state"], "exited")
        self.assertIsNone(result["cleanup_error"], result)

    def test_windows_cleanup_reports_lingering_blocked_reader(self):
        from rfatlas_sionna import client
        released = threading.Event()

        class NeverRead:
            def read(self, _size):
                released.wait(2)
                return b""

            def close(self):
                # Simulate a native pipe close that cannot interrupt this read.
                return None

        class Process:
            returncode = 0

            def __init__(self):
                self.stdin = io.BytesIO()
                self.stdout = NeverRead()
                self.stderr = io.BytesIO()

            def poll(self):
                return self.returncode

            def wait(self, timeout=None):
                return self.returncode

        process = Process()
        with mock.patch.object(client.os, "name", "nt"):
            result = client._supervise_windows(process, b"{}", 1, None)
        self.assertEqual(result["state"], "process_error")
        self.assertIn("stdout pipe thread did not stop", result["cleanup_error"])
        released.set()

    def test_windows_stop_terminates_the_owned_job_tree(self):
        from rfatlas_sionna import client

        process = mock.Mock()
        process.poll.return_value = None
        job = mock.Mock()
        process._kyberia_windows_job = job
        with mock.patch.object(client.os, "name", "nt"):
            client._stop(process)
        job.terminate.assert_called_once_with()
        process.wait.assert_called_once_with(timeout=2)
        process.kill.assert_not_called()

    def test_windows_stop_escalates_after_job_and_reap_failures(self):
        from rfatlas_sionna import client

        process = mock.Mock()
        process.poll.side_effect = [None, None, None]
        process.wait.side_effect = [TimeoutError("initial wait failed"),
                                    TimeoutError("retry wait failed")]
        process.kill.side_effect = OSError("direct kill failed")
        job = mock.Mock()
        job.terminate.side_effect = OSError("job terminate failed")
        process._kyberia_windows_job = job
        with mock.patch.object(client.os, "name", "nt"), \
                self.assertRaisesRegex(OSError, "containment unknown"):
            client._stop(process)
        job.terminate.assert_called_once_with()
        process.kill.assert_called_once_with()
        self.assertEqual(process.wait.call_args_list,
                         [mock.call(timeout=2), mock.call(timeout=2)])

    def test_windows_stop_retries_direct_kill_and_reports_prior_failure(self):
        from rfatlas_sionna import client
        process = mock.Mock()
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

        process.poll.side_effect = poll
        process.wait.side_effect = wait
        process.kill.side_effect = kill
        job = mock.Mock()
        def terminate():
            order.append("job.terminate")
            raise OSError("job terminate failed")
        job.terminate.side_effect = terminate
        process._kyberia_windows_job = job
        with mock.patch.object(client.os, "name", "nt"), \
                self.assertRaisesRegex(OSError, "termination/reap failed") as raised:
            client._stop(process)
        job.terminate.assert_called_once_with()
        process.kill.assert_called_once_with()
        self.assertEqual(process.wait.call_args_list,
                         [mock.call(timeout=2), mock.call(timeout=2)])
        self.assertEqual(order, ["poll", "job.terminate", "wait", "poll", "kill", "wait", "poll"])
        self.assertNotIn("containment unknown", str(raised.exception))

    def test_windows_job_close_retry_distinguishes_proven_containment(self):
        from rfatlas_sionna import client
        job = mock.Mock(_closed=False)
        calls = [0]

        def close_once():
            calls[0] += 1
            if calls[0] == 1:
                raise OSError("transient close failure")
            job._closed = True

        job.close.side_effect = close_once
        errors = []
        client._close_windows_job(job, errors)
        self.assertEqual(job.close.call_count, 2)
        self.assertEqual(len(errors), 1)
        self.assertNotIn("containment unknown", str(errors[0]))

        job = mock.Mock(_closed=False)
        job.close.side_effect = OSError("persistent close failure")
        errors = []
        client._close_windows_job(job, errors)
        self.assertEqual(job.close.call_count, 2)
        self.assertTrue(any("containment unknown" in str(error) for error in errors))

    def test_windows_job_attach_failure_reaps_suspended_child(self):
        from rfatlas_sionna import client

        process = mock.Mock()
        with mock.patch.object(client, "_WindowsJobObject", side_effect=OSError("job unavailable")):
            with self.assertRaisesRegex(OSError, "job unavailable"):
                client._attach_windows_job(process)
        process.kill.assert_called_once_with()
        process.wait.assert_called_once_with(timeout=2)

    def test_windows_supervise_does_not_context_wait_after_cleanup_failure(self):
        from rfatlas_sionna import client

        class Process:
            returncode = None

            def __init__(self):
                self.stdin = io.BytesIO()
                self.stdout = io.BytesIO()
                self.stderr = io.BytesIO()
                self.wait = mock.Mock(side_effect=TimeoutError("bounded wait failed"))

            def __enter__(self):
                raise AssertionError("Windows supervision must not enter Popen context manager")

            def __exit__(self, *_):
                raise AssertionError("Windows supervision must not invoke Popen.__exit__")

            def poll(self):
                return None

        process = Process()
        job = mock.Mock()
        job.terminate.side_effect = OSError("job terminate failed")
        job.close.side_effect = OSError("job close failed")
        started = time.monotonic()
        with mock.patch.object(client.os, "name", "nt"), \
                mock.patch.object(client.subprocess, "Popen", return_value=process), \
                mock.patch.object(client, "_attach_windows_job", return_value=job):
            result = client.supervise([sys.executable, "-c", ""], b"{}", .05)
        self.assertLess(time.monotonic() - started, 1)
        self.assertEqual(result["state"], "process_error", result)
        self.assertIn("termination/reap failed", result["cleanup_error"])
        self.assertIn("job close failed", result["cleanup_error"])
        process.wait.assert_called_with(timeout=2)

    @staticmethod
    def fake_windows_job(kernel):
        from rfatlas_sionna import client
        job = object.__new__(client._WindowsJobObject)
        job._kernel32 = kernel
        job._handle = 99
        job._closed = False
        job._process_assigned = False
        job._owned_handles = {}
        return job

    @staticmethod
    def fake_thread_entry(client, thread_id, process_id):
        def fill(_snapshot, pointer):
            entry = ctypes.cast(pointer, ctypes.POINTER(client._WindowsThreadEntry32)).contents
            entry.th32ThreadID = thread_id
            entry.th32OwnerProcessID = process_id
            return 1
        return fill

    def test_windows_thread_discovery_rejects_missing_multiple_and_unopenable(self):
        from rfatlas_sionna import client
        for mode in ("missing", "multiple", "unopenable"):
            kernel = mock.Mock()
            kernel.CreateToolhelp32Snapshot.return_value = 10
            kernel.CloseHandle.return_value = 1
            if mode == "missing":
                kernel.Thread32First.return_value = 0
                last_error = [18]
            else:
                kernel.Thread32First.side_effect = self.fake_thread_entry(client, 22, 700)
                if mode == "multiple":
                    seen = [False]
                    def next_entry(_snapshot, pointer):
                        if not seen[0]:
                            seen[0] = True
                            return self.fake_thread_entry(client, 23, 700)(_snapshot, pointer)
                        return 0
                    kernel.Thread32Next.side_effect = next_entry
                else:
                    kernel.Thread32Next.return_value = 0
                kernel.OpenThread.return_value = 0 if mode == "unopenable" else 33
                last_error = [18, 5] if mode == "unopenable" else [18]
            job = self.fake_windows_job(kernel)
            expected_error = {"missing": "no thread", "multiple": "multiple", "unopenable": "OpenThread"}[mode]
            with self.subTest(mode=mode), mock.patch.object(
                    client.ctypes, "get_last_error", side_effect=last_error, create=True):
                with self.assertRaisesRegex(OSError, expected_error):
                    job._open_suspended_thread(700)
            kernel.CloseHandle.assert_called_once_with(10)

    def test_windows_auxiliary_handle_close_failures_retry_without_reuse(self):
        from rfatlas_sionna import client

        kernel = mock.Mock()
        kernel.CreateToolhelp32Snapshot.return_value = 10
        kernel.Thread32First.side_effect = self.fake_thread_entry(client, 22, 700)
        kernel.Thread32Next.return_value = 0
        kernel.CloseHandle.side_effect = [0, 1, 1]
        job = self.fake_windows_job(kernel)
        with mock.patch.object(client.ctypes, "get_last_error", return_value=18, create=True), \
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
        process = mock.Mock(pid=700)
        with mock.patch.object(job, "_open_suspended_thread", return_value=33), \
                self.assertRaisesRegex(OSError, "handle close"):
            job.assign_and_resume(process)
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

    def test_windows_resume_failure_and_thread_handle_close_are_structured(self):
        from rfatlas_sionna import client
        kernel = mock.Mock()
        kernel.OpenProcess.return_value = 44
        kernel.AssignProcessToJobObject.return_value = 1
        kernel.ResumeThread.return_value = client._WAIT_FAILED
        kernel.CloseHandle.return_value = 1
        job = self.fake_windows_job(kernel)
        process = mock.Mock(pid=700, _handle=44)
        kernel.AssignProcessToJobObject.return_value = 0
        with mock.patch.object(job, "_open_suspended_thread", return_value=32), \
                mock.patch.object(client.ctypes, "get_last_error", return_value=5, create=True), \
                self.assertRaisesRegex(OSError, "AssignProcessToJobObject"):
            job.assign_and_resume(process)
        self.assertFalse(job._process_assigned)
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(44), mock.call(32)])

        kernel.reset_mock()
        kernel.OpenProcess.return_value = 44
        kernel.AssignProcessToJobObject.return_value = 1
        with mock.patch.object(job, "_open_suspended_thread", return_value=33), \
                mock.patch.object(client.ctypes, "get_last_error", return_value=5, create=True), \
                self.assertRaisesRegex(OSError, "ResumeThread"):
            job.assign_and_resume(process)
        self.assertEqual(kernel.CloseHandle.call_args_list,
                         [mock.call(44), mock.call(33)])

        kernel.ResumeThread.return_value = 1
        kernel.CloseHandle.return_value = 0
        with mock.patch.object(job, "_open_suspended_thread", return_value=34), \
                mock.patch.object(client.ctypes, "get_last_error", return_value=6, create=True), \
                self.assertRaisesRegex(OSError, "handle close"):
            job.assign_and_resume(process)

    def test_windows_job_close_failure_is_retryable_and_never_successful(self):
        from rfatlas_sionna import client
        kernel = mock.Mock()
        kernel.CloseHandle.return_value = 0
        job = self.fake_windows_job(kernel)
        with mock.patch.object(client.ctypes, "get_last_error", return_value=5, create=True), \
                self.assertRaises(OSError):
            job.close()
        self.assertFalse(job._closed)
        self.assertEqual(job._handle, 99)

    def test_windows_job_constructor_retains_handle_after_close_failure(self):
        from rfatlas_sionna import client
        kernel = mock.Mock()
        kernel.CreateJobObjectW.return_value = 77
        kernel.SetInformationJobObject.return_value = 0
        kernel.CloseHandle.return_value = 0
        with mock.patch.object(client.os, "name", "nt"), \
                mock.patch.object(client.ctypes, "WinDLL", return_value=kernel, create=True), \
                mock.patch.object(client.ctypes, "get_last_error", return_value=5, create=True):
            job = client._WindowsJobObject()
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

    def test_windows_nested_job_assignment_failure_is_reported_after_cleanup(self):
        from rfatlas_sionna import client
        process = mock.Mock()
        job = mock.Mock()
        job.assign_and_resume.side_effect = OSError("nested job incompatible")
        with mock.patch.object(client, "_WindowsJobObject", return_value=job), \
                self.assertRaisesRegex(OSError, "nested job incompatible"):
            client._attach_windows_job(process)
        job.terminate.assert_called_once_with()
        job.close.assert_called_once_with()
        process.wait.assert_called_once_with(timeout=2)
        process.kill.assert_not_called()

    def test_windows_attach_cleanup_failures_are_not_suppressed(self):
        from rfatlas_sionna import client
        process = mock.Mock()
        job = mock.Mock()
        job.assign_and_resume.side_effect = OSError("resume setup failed")
        job.terminate.side_effect = OSError("terminate failed")
        job.close.side_effect = OSError("close failed")
        process.kill.side_effect = OSError("kill failed")
        process.wait.side_effect = TimeoutError("wait failed")
        with mock.patch.object(client, "_WindowsJobObject", return_value=job), \
                self.assertRaisesRegex(OSError, "resume setup failed.*cleanup failures"):
            client._attach_windows_job(process)
        job.terminate.assert_called_once_with()
        self.assertEqual(job.close.call_count, 2)
        self.assertEqual(process.kill.call_count, 2)
        self.assertEqual(process.wait.call_count, 2)

    @unittest.skipUnless(os.name == "nt", "Windows job-object descendant contract")
    def test_windows_job_object_kills_descendants_after_parent_exit(self):
        directory = ROOT / ".trash/test-runs" / ("sionna-windows-job-" + str(time.time_ns()))
        directory.mkdir(parents=True)
        marker = directory / "escaped.txt"
        descendant = ("from pathlib import Path; import time; time.sleep(.4); "
                      "Path(" + repr(str(marker)) + ").write_text('descendant escaped')")
        program = ("import subprocess,sys; subprocess.Popen([sys.executable,'-c',"
                   + repr(descendant) + "]); print('parent-exited')")
        result = self.supervise(program, timeout=1)
        self.assertEqual(result["state"], "exited", result)
        self.assertEqual(result["stdout"], b"parent-exited\n")
        time.sleep(.7)
        self.assertFalse(marker.exists(), "Windows job close failed to terminate descendant")

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

    def test_posix_exact_output_limits_are_terminal(self):
        for fd, bound in ((1, MAX_RESULT_BYTES), (2, MAX_LOG_BYTES)):
            result = self.supervise("import os; os.write(%d,b'x'*%d)" % (fd, bound))
            self.assertEqual(result["state"], "output_limit")
            self.assertEqual(len(result["stdout"] if fd == 1 else result["stderr"]), bound)

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
        worker = ROOT / "workers/sionna/worker.py"
        process = subprocess.Popen(
            [sys.executable, str(worker), "--python", sys.executable],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            creationflags=(getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
                            if os.name == "nt" else 0),
        )
        try:
            process.stdin.write(b"{")
            process.stdin.flush()
            time.sleep(0.25)
            if os.name == "nt":
                process.send_signal(getattr(signal, "CTRL_BREAK_EVENT", 1))
            else:
                process.send_signal(signal.SIGINT)
            stdout, stderr = process.communicate(timeout=2)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=2)
        result = json.loads(stdout)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error"], "cancelled")

    @unittest.skipUnless(os.name == "nt", "Windows CTRL_BREAK worker contract")
    def test_windows_supervisor_gets_canonical_cooperative_cancellation(self):
        event = threading.Event()
        timer = threading.Timer(0.1, event.set)
        program = ("import signal,sys,time\n"
                   "def cancel(*_):\n"
                   " sys.stdout.write('{\\\"status\\\":\\\"failed\\\",\\\"error\\\":\\\"cancelled\\\"}\\n'); sys.stdout.flush(); raise SystemExit(2)\n"
                   "signal.signal(signal.SIGBREAK, cancel)\n"
                   "sys.stdin.buffer.read()\n"
                   "time.sleep(10)\n")
        timer.start()
        try:
            result = supervise([sys.executable, "-c", program], b"{}", 1, event)
        finally:
            timer.join()
        self.assertEqual(result["state"], "cancelled", result)
        self.assertIn(b'"error":"cancelled"', result["stdout"])


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
