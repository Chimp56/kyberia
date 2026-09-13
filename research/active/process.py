"""Bounded process supervision; no shell or request-supplied commands."""
import ctypes
import hashlib
import os
from pathlib import Path
import platform
import queue
import re
import selectors
import select
import signal
import subprocess
import threading
import time
import uuid
from dataclasses import asdict, dataclass

from .contract import Invalid, MAX_JSON, VERSION, Request, number, parse_result

MAX_STDERR = 16384
_JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9
_JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
_ERROR_INVALID_HANDLE = 6
_CREATE_SUSPENDED = 0x00000004
_WAIT_FAILED = 0xFFFFFFFF
_WINDOW_PIPE_QUEUE_SIZE = 8
_WINDOW_PIPE_DRAIN_S = 0.5
_WINDOW_PIPE_JOIN_S = 0.2


@dataclass(frozen=True)
class Execution:
    status: str
    returncode: object
    stdout: bytes
    stderr: bytes
    start_ns: int
    end_ns: int
    started_unix_ns: int
    cleanup_error: object = None


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


def _stop(child):
    if os.name == "nt":
        # The job owns the complete descendant tree. Closing it is handled by
        # the supervisor after this reaping step so inherited pipe handles are
        # released before the readers are joined.
        job = getattr(child, "_kyberia_windows_job", None)
        poll = getattr(child, "poll", None)
        try:
            if poll is None or poll() is None:
                try:
                    if job is not None:
                        job.terminate()
                    else:
                        child.kill()
                except ProcessLookupError:
                    pass
        finally:
            child.wait(timeout=2)
        return
    try:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except PermissionError:
            # Darwin can return EPERM for a zombie-only group. Reap only our
            # direct child, then require ESRCH from a non-mutating probe. Never
            # send another signal after reaping: the group ID could be reused.
            child.wait(timeout=2)
            try:
                os.killpg(child.pid, 0)
            except ProcessLookupError:
                return
            raise
    finally:
        child.wait(timeout=2)


def _portable_environment():
    """Keep locale deterministic while retaining Windows launch essentials."""
    if os.name == "nt":
        # CreateProcess/Python may require these values even when the trusted
        # executable path is absolute. Do not pass application secrets or
        # arbitrary environment state into the measurement child.
        names = ("PATH", "PATHEXT", "SystemRoot", "WINDIR", "TEMP", "TMP",
                 "SYSTEMDRIVE", "COMSPEC")
        return {name: os.environ[name] for name in names if os.environ.get(name)}
    return {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C"}


def _popen_options():
    options = {"shell": False, "env": _portable_environment()}
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


def _pipe_reader(pipe, label, events, stop_event):
    try:
        while True:
            chunk = pipe.read(16384)
            if not chunk:
                break
            if not _put_pipe_event(events, ("data", label, chunk), stop_event):
                return
    except (OSError, ValueError):
        pass
    finally:
        _put_pipe_event(events, ("eof", label, b""), stop_event)


def _consume_pipe_events(events, output, eof, stdout_limit):
    status = "completed"
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
        bound = stdout_limit if label == "stdout" else MAX_STDERR
        remaining = bound - len(output[label])
        if len(chunk) > remaining:
            output[label].extend(chunk[:max(0, remaining)])
            status = "output_limit"
        else:
            output[label].extend(chunk)


def _close_pipes(child):
    for pipe in (child.stdin, child.stdout, child.stderr):
        if pipe is None:
            continue
        try:
            pipe.close()
        except (OSError, ValueError):
            pass


def _execute_windows(argv, timeout_s, cancel, stdout_limit):
    """Supervise anonymous Windows pipes without POSIX select semantics."""
    start, utc = time.monotonic_ns(), time.time_ns()
    if cancel and cancel.is_set():
        return Execution("cancelled", None, b"", b"", start, time.monotonic_ns(), utc)
    output = {"stdout": bytearray(), "stderr": bytearray()}
    status = "completed"
    cleanup_error = None
    child = None
    events = queue.Queue(maxsize=_WINDOW_PIPE_QUEUE_SIZE)
    stop_event = threading.Event()
    eof = set()
    readers = []
    try:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, **_popen_options())
        _attach_windows_job(child)
        for label, pipe in (("stdout", child.stdout), ("stderr", child.stderr)):
            thread = threading.Thread(target=_pipe_reader,
                                       args=(pipe, label, events, stop_event), daemon=True)
            thread.start()
            readers.append(thread)
        while True:
            if cancel and cancel.is_set():
                status = "cancelled"
                break
            if (time.monotonic_ns() - start) / 1e9 >= timeout_s:
                status = "timeout"
                break
            if status == "completed":
                status = _consume_pipe_events(events, output, eof, stdout_limit)
            if status != "completed" or child.poll() is not None:
                break
            time.sleep(0.01)
    except FileNotFoundError:
        status = "unavailable"
    except (OSError, subprocess.TimeoutExpired) as exc:
        status = "process_error"
        output["stderr"] = str(exc).encode()[:MAX_STDERR]
    finally:
        if child:
            try:
                _stop(child)
            except (OSError, subprocess.TimeoutExpired) as exc:
                cleanup_error = str(exc)[:1024]
                status = "process_error"
            if getattr(child, "_kyberia_windows_job", None) is not None:
                try:
                    child._kyberia_windows_job.close()
                except OSError as exc:
                    cleanup_error = str(exc)[:1024]
                    status = "process_error"
            drain_deadline = time.monotonic() + _WINDOW_PIPE_DRAIN_S
            while eof != {"stdout", "stderr"} and time.monotonic() < drain_deadline:
                drained = _consume_pipe_events(events, output, eof, stdout_limit)
                if status == "completed" and drained != "completed":
                    status = drained
                if eof == {"stdout", "stderr"}:
                    break
                time.sleep(0.005)
            if eof != {"stdout", "stderr"}:
                cleanup_error = cleanup_error or "pipe readers did not reach EOF before cleanup bound"
                status = "process_error"
            stop_event.set()
            _close_pipes(child)
            for thread in readers:
                thread.join(timeout=_WINDOW_PIPE_JOIN_S)
            drained = _consume_pipe_events(events, output, eof, stdout_limit)
            if status == "completed" and drained != "completed":
                status = drained
    if status == "completed" and child and child.returncode != 0:
        status = "process_error"
    return Execution(status, child.returncode if child else None,
                     bytes(output["stdout"]), bytes(output["stderr"]), start,
                     time.monotonic_ns(), utc, cleanup_error)


class _ExitObserver:
    """Observe exit without reaping, retaining process-group ownership for cleanup."""
    def __init__(self, pid):
        self.pid, self.exited, self.queue = pid, False, None
        if hasattr(os, "waitid") and hasattr(os, "WNOWAIT"):
            return
        if not hasattr(select, "kqueue"):
            raise OSError("non-reaping process-exit observation unavailable")
        self.queue = select.kqueue()
        try:
            self.queue.control([select.kevent(pid, filter=select.KQ_FILTER_PROC,
                                             flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT,
                                             fflags=select.KQ_NOTE_EXIT)], 0, 0)
        except BaseException:
            self.queue.close()
            raise

    def poll(self):
        if not self.exited:
            if self.queue is not None:
                self.exited = bool(self.queue.control(None, 1, 0))
            else:
                result = os.waitid(os.P_PID, self.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                self.exited = result is not None and result.si_pid == self.pid
        return self.exited

    def close(self):
        if self.queue is not None: self.queue.close()


def execute(argv, timeout_s, cancel=None, stdout_limit=MAX_JSON):
    """Internal trusted argv only. Pipes are capped and the child is reaped."""
    number(timeout_s, 0.05, 12)
    if os.name == "nt":
        return _execute_windows(argv, timeout_s, cancel, stdout_limit)
    start, utc = time.monotonic_ns(), time.time_ns()
    if cancel and cancel.is_set():
        return Execution("cancelled", None, b"", b"", start, time.monotonic_ns(), utc)
    child = None
    observer = None
    output = {"stdout": bytearray(), "stderr": bytearray()}
    status = "completed"
    cleanup_error = None
    try:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, **_popen_options())
        observer = _ExitObserver(child.pid)
        with selectors.DefaultSelector() as selector:
            for name, pipe in (("stdout", child.stdout), ("stderr", child.stderr)):
                os.set_blocking(pipe.fileno(), False)
                selector.register(pipe, selectors.EVENT_READ, name)
            while selector.get_map() or not observer.poll():
                if cancel and cancel.is_set(): status = "cancelled"; break
                if (time.monotonic_ns() - start) / 1e9 >= timeout_s: status = "timeout"; break
                for key, _ in selector.select(0.02):
                    chunk = os.read(key.fd, 16384)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    limit = stdout_limit if key.data == "stdout" else MAX_STDERR
                    remaining = limit - len(output[key.data])
                    output[key.data].extend(chunk[:remaining])
                    if len(chunk) > remaining: status = "output_limit"; break
                if status != "completed": break
    except FileNotFoundError:
        status = "unavailable"
    except (OSError, subprocess.TimeoutExpired) as exc:
        status = "process_error"
        output["stderr"] = str(exc).encode()[:MAX_STDERR]
    finally:
        if child:
            # A reaped direct child can leave descendants in this process group
            # after closing all inherited pipes. Terminate the owned group on
            # every terminal path, including ordinary successful parent exit.
            try:
                _stop(child)
            except (OSError, subprocess.TimeoutExpired) as exc:
                cleanup_error = str(exc)[:1024]
                status = "process_error"
            finally:
                if observer: observer.close()
                for pipe in (child.stdout, child.stderr): pipe.close()
    if status == "completed" and child.returncode != 0: status = "process_error"
    return Execution(status, child.returncode if child else None,
                     bytes(output["stdout"]), bytes(output["stderr"]),
                     start, time.monotonic_ns(), utc, cleanup_error)


def run(request, binary, cancel=None, timeout_s=8):
    if not isinstance(request, Request): raise Invalid("validated Request required")
    # Revalidate even a dataclass forged through object.__setattr__ by a caller.
    request = Request(**asdict(request))
    number(timeout_s, 0.05, 10)
    result = {"schema_version": "1", "adapter_version": "0.1.0", "request": asdict(request),
              "status": "unsupported", "reason": None, "measurement": None,
              "authentication": "not_implemented_loopback_only", "wifi_attribution": "not_established",
              "source": None, "process_window": None, "exit_code": None,
              "stdout_sha256": None, "stderr_bytes": 0}
    result["cleanup_error"] = None
    if request.unsupported_reason():
        result["reason"] = request.unsupported_reason()
        return result
    if os.name != "posix": result["reason"] = "posix_only"; return result
    path = Path(binary).resolve()
    if not path.is_file(): result.update(status="unavailable", reason="iperf3_not_installed"); return result
    # Executable selection is trusted local configuration, never an IPC request field.
    binary_hash = hashlib.sha256(path.read_bytes()).hexdigest()
    probe = execute([str(path), "--version"], min(2, timeout_s), cancel, 8192)
    if probe.status != "completed":
        result.update(status=probe.status, reason="version_probe_failed", exit_code=probe.returncode,
                      cleanup_error=probe.cleanup_error)
        return result
    version_text = probe.stdout.decode("utf-8", errors="replace")
    match = re.match(r"iperf (\d+\.\d+(?:\.\d+)?) \(cJSON ([\d.]+)\)\n", version_text)
    if not match or match.group(1) != VERSION or match.group(2) != "1.7.15":
        result.update(status="unsupported", reason="unverified_iperf3_version")
        return result
    result["source"] = {"tool": "iperf3", "version": VERSION, "cjson_version": match.group(2),
                        "binary_sha256": binary_hash, "python": platform.python_version(),
                        "os": platform.platform(), "machine": platform.machine(),
                        "optional_features": version_text.splitlines()[-1]}
    execution = execute(request.argv(str(path)), timeout_s, cancel)
    result.update(status=execution.status, exit_code=execution.returncode, cleanup_error=execution.cleanup_error,
                  stdout_sha256=hashlib.sha256(execution.stdout).hexdigest(), stderr_bytes=len(execution.stderr),
                  process_window={"clock_epoch": str(uuid.uuid4()), "clock": "host_monotonic_ns",
                                  "start_ns": execution.start_ns, "end_ns": execution.end_ns,
                                  "host_started_unix_ns": execution.started_unix_ns,
                                  "meaning": "client_process_execution_not_RF_capture",
                                  "clock_synchronization": "unknown"})
    if execution.status != "completed":
        result["reason"] = "client_" + execution.status
        return result
    try:
        result["measurement"] = parse_result(execution.stdout, request).json_value()
    except Invalid as exc:
        result.update(status="invalid_output", reason=str(exc))
    return result
