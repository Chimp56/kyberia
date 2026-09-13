"""Bounded one-job process supervision; standard library only, no engine imports."""

from datetime import datetime, timezone
import ctypes
import hashlib
import os
from pathlib import Path
import queue
import selectors
import signal
import subprocess
import threading
import time

from .contract import (ContractError, MAX_LOG_BYTES, MAX_REQUEST_BYTES, MAX_RESULT_BYTES,
                       canonical_bytes, decode, digest, validate, validate_result)


_JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9
_JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
_ERROR_INVALID_HANDLE = 6
_CREATE_SUSPENDED = 0x00000004
_WAIT_FAILED = 0xFFFFFFFF
_WINDOW_PIPE_QUEUE_SIZE = 8
_WINDOW_PIPE_DRAIN_S = 0.5
_WINDOW_PIPE_JOIN_S = 0.2


class _WindowsIoCounters(ctypes.Structure):
    _fields_ = [("ReadOperationCount", ctypes.c_ulonglong),
                ("WriteOperationCount", ctypes.c_ulonglong),
                ("OtherOperationCount", ctypes.c_ulonglong),
                ("ReadTransferCount", ctypes.c_ulonglong),
                ("WriteTransferCount", ctypes.c_ulonglong),
                ("OtherTransferCount", ctypes.c_ulonglong)]


class _WindowsBasicLimitInformation(ctypes.Structure):
    _fields_ = [("PerProcessUserTime", ctypes.c_longlong),
                ("PerJobUserTime", ctypes.c_longlong),
                ("LimitFlags", ctypes.c_uint32),
                ("MinimumWorkingSetSize", ctypes.c_size_t),
                ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", ctypes.c_uint32),
                ("Affinity", ctypes.c_size_t),
                ("Priority", ctypes.c_uint32),
                ("SchedulingClass", ctypes.c_uint32)]


class _WindowsExtendedLimitInformation(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", _WindowsBasicLimitInformation),
                ("IoInfo", _WindowsIoCounters),
                ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t),
                ("PeakJobMemoryUsed", ctypes.c_size_t)]


def _windows_handle(value):
    try:
        return ctypes.c_void_p(int(value))
    except (TypeError, ValueError, AttributeError) as error:
        raise OSError("Windows process handle is not available") from error


class _WindowsJobObject:
    """Own a suspended child and every descendant through a kill-on-close job."""
    def __init__(self):
        if os.name != "nt":
            raise OSError("Windows job objects are unavailable on this platform")
        self._kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self._kernel32.CreateJobObjectW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p]
        self._kernel32.CreateJobObjectW.restype = ctypes.c_void_p
        self._kernel32.SetInformationJobObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32,
                                                            ctypes.c_void_p, ctypes.c_uint32]
        self._kernel32.SetInformationJobObject.restype = ctypes.c_int
        self._kernel32.AssignProcessToJobObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
        self._kernel32.AssignProcessToJobObject.restype = ctypes.c_int
        self._kernel32.TerminateJobObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        self._kernel32.TerminateJobObject.restype = ctypes.c_int
        self._kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        self._kernel32.CloseHandle.restype = ctypes.c_int
        self._kernel32.ResumeThread.argtypes = [ctypes.c_void_p]
        self._kernel32.ResumeThread.restype = ctypes.c_uint32
        self._handle = self._kernel32.CreateJobObjectW(None, None)
        if not self._handle:
            self._raise_last_error("CreateJobObjectW")
        self._closed = False
        limits = _WindowsExtendedLimitInformation()
        limits.BasicLimitInformation.LimitFlags = _JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not self._kernel32.SetInformationJobObject(
                self._handle, _JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                ctypes.byref(limits), ctypes.sizeof(limits)):
            try:
                self._kernel32.CloseHandle(self._handle)
            finally:
                self._handle = None
            self._raise_last_error("SetInformationJobObject")

    @staticmethod
    def _raise_last_error(operation):
        code = ctypes.get_last_error()
        raise OSError(code, f"{operation} failed: {ctypes.FormatError(code)}")

    def assign_and_resume(self, process):
        if not self._kernel32.AssignProcessToJobObject(self._handle,
                                                        _windows_handle(process._handle)):
            self._raise_last_error("AssignProcessToJobObject")
        if self._kernel32.ResumeThread(_windows_handle(process._thread_handle)) == _WAIT_FAILED:
            self._raise_last_error("ResumeThread")

    def terminate(self):
        if self._closed or not self._handle:
            return
        if not self._kernel32.TerminateJobObject(self._handle, 1):
            code = ctypes.get_last_error()
            if code != _ERROR_INVALID_HANDLE:
                self._raise_last_error("TerminateJobObject")

    def close(self):
        if self._closed:
            return
        handle = self._handle
        self._handle = None
        self._closed = True
        if handle and not self._kernel32.CloseHandle(handle):
            self._raise_last_error("CloseHandle")


def _attach_windows_job(process):
    job = None
    try:
        job = _WindowsJobObject()
        process._kyberia_windows_job = job
        job.assign_and_resume(process)
    except BaseException:
        if job is not None:
            try:
                job.terminate()
            except BaseException:
                pass
        try:
            process.kill()
        except BaseException:
            pass
        try:
            process.wait(timeout=2)
        except BaseException:
            pass
        if job is not None:
            try:
                job.close()
            except BaseException:
                pass
        raise
    return job


def _stop(process):
    # POSIX jobs start in their own session. Windows jobs own the complete
    # descendant tree and are closed by the supervisor after reaping.
    if os.name == "nt":
        job = getattr(process, "_kyberia_windows_job", None)
        poll = getattr(process, "poll", None)
        try:
            if poll is None or poll() is None:
                try:
                    if job is not None:
                        job.terminate()
                    else:
                        process.kill()
                except ProcessLookupError:
                    pass
        finally:
            process.wait(timeout=2)
        return
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def _popen_options(env):
    options = {"env": env}
    if os.name == "posix":
        options["start_new_session"] = True
    elif os.name == "nt":
        options["creationflags"] = (getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
                                     | getattr(subprocess, "CREATE_SUSPENDED", _CREATE_SUSPENDED))
    return options


def _put_pipe_event(events, event, stop_event):
    while not stop_event.is_set():
        try:
            events.put(event, timeout=0.05)
            return True
        except queue.Full:
            continue
    return False


def _pipe_reader(stream, label, events, stop_event):
    try:
        while True:
            chunk = stream.read(8192)
            if not chunk:
                break
            if not _put_pipe_event(events, ("data", label, chunk), stop_event):
                return
    except (OSError, ValueError):
        pass
    finally:
        _put_pipe_event(events, ("eof", label, b""), stop_event)


def _pipe_writer(stream, payload, events, stop_event):
    try:
        stream.write(payload)
        stream.flush()
    except (BrokenPipeError, OSError, ValueError):
        pass
    finally:
        try:
            stream.close()
        except (OSError, ValueError):
            pass
        _put_pipe_event(events, ("stdin_done", "stdin", b""), stop_event)


def _consume_pipe_events(events, output, logs, eof):
    status = "exited"
    while True:
        try:
            kind, label, chunk = events.get_nowait()
        except queue.Empty:
            return status
        if kind == "eof":
            eof.add(label)
            continue
        if kind != "data":
            continue
        target = output if label == "stdout" else logs
        bound = MAX_RESULT_BYTES if label == "stdout" else MAX_LOG_BYTES
        remaining = bound - len(target)
        if len(chunk) > remaining:
            target.extend(chunk[:max(0, remaining)])
            status = "output_limit"
        else:
            target.extend(chunk)


def _close_pipes(process):
    for stream in (process.stdin, process.stdout, process.stderr):
        try:
            stream.close()
        except (OSError, ValueError):
            pass


def _supervise_windows(process, request_bytes, timeout_s, cancel, job=None):
    """Use threads for Windows anonymous pipes, which SelectSelector cannot poll."""
    start = time.monotonic()
    output, logs = bytearray(), bytearray()
    events = queue.Queue(maxsize=_WINDOW_PIPE_QUEUE_SIZE)
    stop_event = threading.Event()
    eof = set()
    threads = []
    status = "exited"
    cleanup_error = None
    try:
        for stream, label in ((process.stdout, "stdout"), (process.stderr, "stderr")):
            thread = threading.Thread(target=_pipe_reader,
                                       args=(stream, label, events, stop_event), daemon=True)
            thread.start()
            threads.append(thread)
        writer = threading.Thread(target=_pipe_writer,
                                  args=(process.stdin, request_bytes, events, stop_event), daemon=True)
        writer.start()
        threads.append(writer)
        while True:
            status = _consume_pipe_events(events, output, logs, eof) if status == "exited" else status
            if status != "exited":
                break
            if cancel is not None and cancel.is_set():
                status = "cancelled"
                break
            if time.monotonic() - start >= timeout_s:
                status = "timed_out"
                break
            if process.poll() is not None:
                break
            time.sleep(0.01)
    finally:
        try:
            _stop(process)
        except (OSError, subprocess.TimeoutExpired) as error:
            cleanup_error = str(error)[:1024]
            status = "process_error"
        if job is not None:
            try:
                # Closing a kill-on-close job also removes descendants that
                # inherited either output handle after the parent exited.
                job.close()
            except OSError as error:
                cleanup_error = str(error)[:1024]
                status = "process_error"
        drain_deadline = time.monotonic() + _WINDOW_PIPE_DRAIN_S
        while eof != {"stdout", "stderr"} and time.monotonic() < drain_deadline:
            drained = _consume_pipe_events(events, output, logs, eof)
            if status == "exited" and drained != "exited":
                status = drained
            if eof == {"stdout", "stderr"}:
                break
            time.sleep(0.005)
        if eof != {"stdout", "stderr"}:
            cleanup_error = cleanup_error or "pipe readers did not reach EOF before cleanup bound"
            status = "process_error"
        stop_event.set()
        _close_pipes(process)
        for thread in threads:
            thread.join(timeout=_WINDOW_PIPE_JOIN_S)
        drained = _consume_pipe_events(events, output, logs, eof)
        if status == "exited" and drained != "exited":
            status = drained
    return {"state": status, "returncode": process.returncode,
            "stdout": bytes(output), "stderr": bytes(logs),
            "elapsed_s": time.monotonic() - start, "cleanup_error": cleanup_error}


def supervise(command, request_bytes, timeout_s, cancel=None, env=None):
    """Trusted caller supplies command; protocol cannot select executables or paths."""
    if len(request_bytes) > MAX_REQUEST_BYTES:
        raise ContractError("request exceeds byte limit")
    start = time.monotonic()
    output, logs = bytearray(), bytearray()
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, **_popen_options(env)) as process:
        if os.name == "nt":
            job = _attach_windows_job(process)
            return _supervise_windows(process, request_bytes, timeout_s, cancel, job)
        with selectors.DefaultSelector() as selector:
            for stream, label in ((process.stdout, "stdout"), (process.stderr, "stderr")):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, label)
            # Requests are below the bounded pipe payload; nonblocking writes keep cancellation live.
            os.set_blocking(process.stdin.fileno(), False)
            selector.register(process.stdin, selectors.EVENT_WRITE, "stdin")
            remaining = memoryview(request_bytes)
            status = "exited"
            try:
                while selector.get_map():
                    if cancel is not None and cancel.is_set():
                        status = "cancelled"
                        break
                    if time.monotonic() - start >= timeout_s:
                        status = "timed_out"
                        break
                    for key, _ in selector.select(0.02):
                        stream, label = key.fileobj, key.data
                        if label == "stdin":
                            try:
                                count = os.write(stream.fileno(), remaining)
                                remaining = remaining[count:]
                            except BrokenPipeError:
                                remaining = memoryview(b"")
                            if not remaining:
                                selector.unregister(stream)
                                stream.close()
                            continue
                        chunk = os.read(stream.fileno(), 8192)
                        if not chunk:
                            selector.unregister(stream)
                            continue
                        target = output if label == "stdout" else logs
                        bound = MAX_RESULT_BYTES if label == "stdout" else MAX_LOG_BYTES
                        target.extend(chunk[:max(0, bound-len(target))])
                        if len(target) >= bound:
                            status = "output_limit"
                            break
                    if status != "exited":
                        break
                # A process can close pipes and keep computing; deadline still applies.
                while status == "exited" and process.poll() is None:
                    if cancel is not None and cancel.is_set():
                        status = "cancelled"
                    elif time.monotonic() - start >= timeout_s:
                        status = "timed_out"
                    else:
                        time.sleep(0.01)
            finally:
                _stop(process)
            return {"state": status, "returncode": process.returncode,
                    "stdout": bytes(output), "stderr": bytes(logs),
                    "elapsed_s": time.monotonic() - start}


def run(request, python_executable, cancel=None):
    """Validate and execute one immutable request; process failures never return predictions."""
    payload = canonical_bytes(request)
    if len(payload) > MAX_REQUEST_BYTES:
        raise ContractError("request exceeds byte limit")
    validate(request)
    started = datetime.now(timezone.utc).isoformat()
    worker = Path(__file__).resolve().with_name("engine.py")
    timeout = request.get("limits", {}).get("timeout_s", 60)
    # Runtime paths belong to the trusted launcher, never supplied by a scene/request.
    env = {k: v for k, v in os.environ.items() if k not in ("PYTHONPATH", "PYTHONHOME")}
    execution = supervise([str(python_executable), "-I", str(worker)], payload, timeout, cancel, env)
    envelope = {"schema_version": 1, "request_id": request["request_id"],
                "request_sha256": digest(request), "started_utc": started,
                "ended_utc": datetime.now(timezone.utc).isoformat(),
                "elapsed_s": execution["elapsed_s"], "returncode": execution["returncode"],
                "log_sha256": hashlib.sha256(execution["stderr"]).hexdigest(),
                "log": execution["stderr"].decode("utf-8", errors="replace"),
                "cleanup_error": execution.get("cleanup_error"),
                "resource_limits": {"wall_timeout_s": timeout,
                    "cpu_s": request.get("limits", {}).get("cpu_s", 30),
                    "request_bytes": MAX_REQUEST_BYTES, "result_bytes": MAX_RESULT_BYTES,
                    "log_bytes": MAX_LOG_BYTES, "hard_memory_limit": "not_implemented"}}
    if execution["state"] != "exited" or execution["returncode"] != 0:
        envelope.update(status="failed", error=(execution["state"] if execution["state"] != "exited"
                                              else "worker_crashed"))
        return envelope
    try:
        response = decode(execution["stdout"], MAX_RESULT_BYTES)
        validate_result(response, request)
        envelope["result"] = response
        envelope["status"] = response["status"]
        if response["status"] == "failed":
            envelope["error"] = response.get("error", "engine_failure")
    except (ContractError, ValueError, TypeError, RecursionError):
        envelope.update(status="failed", error="invalid_worker_response")
    return envelope
