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
_JOB_OBJECT_LIMIT_JOB_TIME = 0x0004
_WINDOW_CPU_TICKS_PER_SECOND = 10_000_000
_ERROR_INVALID_HANDLE = 6
_ERROR_NO_MORE_FILES = 18
_CREATE_SUSPENDED = 0x00000004
_WAIT_FAILED = 0xFFFFFFFF
_TH32CS_SNAPTHREAD = 0x00000004
_THREAD_SUSPEND_RESUME = 0x0002
_PROCESS_TERMINATE = 0x0001
_PROCESS_SET_QUOTA = 0x0100
_CTRL_BREAK_EVENT = 1
_WINDOW_PIPE_READ_BYTES = 8192
# Keep each stream's ceiling explicit: a descheduled supervisor can leave one
# data event for every capped stdout/stderr chunk plus bounded terminal events.
_WINDOW_PIPE_STDOUT_EVENTS = ((MAX_RESULT_BYTES + _WINDOW_PIPE_READ_BYTES - 1)
                               // _WINDOW_PIPE_READ_BYTES)
_WINDOW_PIPE_STDERR_EVENTS = ((MAX_LOG_BYTES + _WINDOW_PIPE_READ_BYTES - 1)
                              // _WINDOW_PIPE_READ_BYTES)
_WINDOW_PIPE_TERMINAL_QUEUE_SIZE = 3  # reader error+EOF or writer error+close+done
_WINDOW_PIPE_DRAIN_S = 0.5
_WINDOW_PIPE_JOIN_S = 0.2
_WINDOW_CANCEL_GRACE_S = 0.5
_WINDOW_HANDLE_CLOSE_ATTEMPTS = 2


class _PipeEvents:
    """Independent bounded data and terminal channels for one pipe."""
    def __init__(self, data_capacity):
        self.data = queue.Queue(maxsize=data_capacity)
        self.terminal = queue.Queue(maxsize=_WINDOW_PIPE_TERMINAL_QUEUE_SIZE)


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


class _WindowsThreadEntry32(ctypes.Structure):
    _fields_ = [("dwSize", ctypes.c_uint32),
                ("cntUsage", ctypes.c_uint32),
                ("th32ThreadID", ctypes.c_uint32),
                ("th32OwnerProcessID", ctypes.c_uint32),
                ("tpBasePri", ctypes.c_long),
                ("tpDeltaPri", ctypes.c_long),
                ("dwFlags", ctypes.c_uint32)]


def _windows_handle(value):
    try:
        if isinstance(value, ctypes.c_void_p):
            return value
        return ctypes.c_void_p(int(value))
    except (TypeError, ValueError, AttributeError) as error:
        raise OSError("Windows process handle is not available") from error


class _WindowsJobObject:
    """Own a suspended child and every descendant through a kill-on-close job."""
    def __init__(self, cpu_s=None):
        if os.name != "nt":
            raise OSError("Windows job objects are unavailable on this platform")
        if (cpu_s is not None
                and (type(cpu_s) is not int or not 1 <= cpu_s <= 120)):
            raise ValueError("cpu_s must be an integer from 1 through 120")
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
        self._kernel32.CreateToolhelp32Snapshot.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
        self._kernel32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
        self._kernel32.Thread32First.argtypes = [ctypes.c_void_p,
                                                  ctypes.POINTER(_WindowsThreadEntry32)]
        self._kernel32.Thread32First.restype = ctypes.c_int
        self._kernel32.Thread32Next.argtypes = [ctypes.c_void_p,
                                                 ctypes.POINTER(_WindowsThreadEntry32)]
        self._kernel32.Thread32Next.restype = ctypes.c_int
        self._kernel32.OpenThread.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        self._kernel32.OpenThread.restype = ctypes.c_void_p
        self._kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        self._kernel32.OpenProcess.restype = ctypes.c_void_p
        self._kernel32.GenerateConsoleCtrlEvent.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
        self._kernel32.GenerateConsoleCtrlEvent.restype = ctypes.c_int
        self._handle = self._kernel32.CreateJobObjectW(None, None)
        if not self._handle:
            self._raise_last_error("CreateJobObjectW")
        self._closed = False
        self._process_assigned = False
        self._initialization_error = None
        self._owned_handles = {}
        limits = _WindowsExtendedLimitInformation()
        limits.BasicLimitInformation.LimitFlags = _JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if cpu_s is not None:
            limits.BasicLimitInformation.LimitFlags |= _JOB_OBJECT_LIMIT_JOB_TIME
            limits.BasicLimitInformation.PerJobUserTime = (
                cpu_s * _WINDOW_CPU_TICKS_PER_SECOND)
        if not self._kernel32.SetInformationJobObject(
                self._handle, _JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                ctypes.byref(limits), ctypes.sizeof(limits)):
            primary_error = None
            try:
                self._raise_last_error("SetInformationJobObject")
            except BaseException as error:
                primary_error = error
            close_error = None
            try:
                self._close_native_handle(self._handle, "CloseHandle(job)")
            except BaseException as error:
                close_error = error
            if close_error is not None:
                # Keep the native handle owned by this object.  _attach_windows_job
                # will route the initialization error through the normal bounded
                # termination/close retry path.
                self._initialization_error = OSError(
                    f"{primary_error}; job handle close failed: {close_error}")
                return
            self._handle = None
            self._closed = True
            self._initialization_error = primary_error

    @staticmethod
    def _raise_last_error(operation):
        code = ctypes.get_last_error()
        detail = ctypes.FormatError(code) if hasattr(ctypes, "FormatError") else os.strerror(code)
        raise OSError(code, f"{operation} failed: {detail}")

    def _own_handle(self, label, handle):
        if handle:
            self._owned_handles[label] = handle
        return handle

    def _close_native_handle(self, handle, operation, label=None):
        if handle and not self._kernel32.CloseHandle(handle):
            self._raise_last_error(operation)
        if label is not None:
            self._owned_handles.pop(label, None)

    def _close_owned_handles(self):
        errors = []
        for label, handle in list(self._owned_handles.items()):
            try:
                self._close_native_handle(handle, f"CloseHandle({label})", label)
            except BaseException as error:
                errors.append(error)
        return errors

    def _open_suspended_thread(self, process_id):
        snapshot = self._kernel32.CreateToolhelp32Snapshot(_TH32CS_SNAPTHREAD, 0)
        invalid = ctypes.c_void_p(-1).value
        if not snapshot or int(snapshot) == invalid:
            self._raise_last_error("CreateToolhelp32Snapshot")
        self._own_handle("thread snapshot", snapshot)
        thread_ids = []
        primary_error = None
        try:
            entry = _WindowsThreadEntry32()
            entry.dwSize = ctypes.sizeof(entry)
            if not self._kernel32.Thread32First(snapshot, ctypes.byref(entry)):
                code = ctypes.get_last_error()
                if code != _ERROR_NO_MORE_FILES:
                    self._raise_last_error("Thread32First")
            else:
                while True:
                    if entry.th32OwnerProcessID == process_id:
                        thread_ids.append(entry.th32ThreadID)
                    entry.dwSize = ctypes.sizeof(entry)
                    if not self._kernel32.Thread32Next(snapshot, ctypes.byref(entry)):
                        code = ctypes.get_last_error()
                        if code != _ERROR_NO_MORE_FILES:
                            self._raise_last_error("Thread32Next")
                        break
        except BaseException as error:
            primary_error = error
        close_error = None
        try:
            self._close_native_handle(snapshot, "CloseHandle(thread snapshot)", "thread snapshot")
        except BaseException as error:
            close_error = error
        if primary_error is not None:
            if close_error is not None:
                raise OSError(f"{primary_error}; thread snapshot close failed: {close_error}") from primary_error
            raise primary_error
        if close_error is not None:
            raise close_error
        if not thread_ids:
            raise OSError(f"no thread found for suspended process {process_id}")
        if len(thread_ids) != 1:
            raise OSError(f"multiple threads found for suspended process {process_id}")
        thread = self._kernel32.OpenThread(_THREAD_SUSPEND_RESUME, 0, thread_ids[0])
        if not thread:
            self._raise_last_error("OpenThread")
        return self._own_handle("thread", thread)

    def assign_and_resume(self, process):
        thread = self._open_suspended_thread(process.pid)
        primary_error = None
        process_handle = None
        try:
            process_handle = self._kernel32.OpenProcess(
                _PROCESS_TERMINATE | _PROCESS_SET_QUOTA, 0, process.pid)
            if not process_handle:
                self._raise_last_error("OpenProcess")
            self._own_handle("process", process_handle)
            if not self._kernel32.AssignProcessToJobObject(self._handle,
                                                            _windows_handle(process_handle)):
                self._raise_last_error("AssignProcessToJobObject")
            self._process_assigned = True
            if self._kernel32.ResumeThread(_windows_handle(thread)) == _WAIT_FAILED:
                self._raise_last_error("ResumeThread")
        except BaseException as error:
            primary_error = error
        close_errors = []
        if process_handle:
            try:
                self._close_native_handle(process_handle, "CloseHandle(process)", "process")
            except BaseException as error:
                close_errors.append(error)
        try:
            self._close_native_handle(thread, "CloseHandle(thread)", "thread")
        except BaseException as error:
            close_errors.append(error)
        if primary_error is not None:
            if close_errors:
                raise OSError(f"{primary_error}; handle close failures: {'; '.join(map(str, close_errors))}") from primary_error
            raise primary_error
        if close_errors:
            raise OSError("handle close failures: " + "; ".join(map(str, close_errors)))

    def request_cancel(self, process):
        if not self._kernel32.GenerateConsoleCtrlEvent(_CTRL_BREAK_EVENT, process.pid):
            self._raise_last_error("GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT)")

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
        close_errors = self._close_owned_handles()
        handle = self._handle
        if handle:
            try:
                self._close_native_handle(handle, "CloseHandle(job)")
            except BaseException as error:
                close_errors.append(error)
            else:
                self._handle = None
        if close_errors:
            raise OSError("Windows handle close failures: "
                          + "; ".join(str(error) for error in close_errors)) from close_errors[0]
        self._closed = True


def _raise_lifecycle_error(primary, cleanup_errors):
    if not cleanup_errors:
        if primary is not None:
            raise primary
        return
    detail = "; ".join(str(error) for error in cleanup_errors)
    if primary is None:
        raise OSError(f"Windows worker cleanup failed: {detail}")
    raise OSError(f"{primary}; cleanup failures: {detail}") from primary


def _close_windows_job(job, cleanup_errors):
    for _ in range(_WINDOW_HANDLE_CLOSE_ATTEMPTS):
        try:
            job.close()
            return
        except BaseException as error:
            cleanup_errors.append(error)
            if getattr(job, "_closed", None) is True:
                return
    if getattr(job, "_closed", None) is not True:
        handle = getattr(job, "_handle", None)
        owned_handles = getattr(job, "_owned_handles", None)
        if handle is None and isinstance(owned_handles, dict) and owned_handles:
            labels = ", ".join(sorted(str(label) for label in owned_handles))
            cleanup_errors.append(OSError(
                "Windows auxiliary native handle cleanup unresolved after Job Object "
                f"close ({labels}); retry ownership retained"))
        else:
            cleanup_errors.append(OSError("Windows containment unknown: Job Object handle remains open"))


def _attach_windows_job(process, cpu_s=None):
    job = None
    primary_error = None
    try:
        job = _WindowsJobObject(cpu_s)
        process._kyberia_windows_job = job
        initialization_error = getattr(job, "_initialization_error", None)
        if isinstance(initialization_error, BaseException):
            raise initialization_error
        job.assign_and_resume(process)
    except BaseException as error:
        primary_error = error
    if primary_error is None:
        return job
    cleanup_errors = []
    terminate_failed = False
    if job is not None:
        try:
            job.terminate()
        except BaseException as error:
            cleanup_errors.append(error)
            terminate_failed = True
    process_assigned = getattr(job, "_process_assigned", True) if job is not None else False
    if job is None or terminate_failed or not process_assigned:
        try:
            process.kill()
        except BaseException as error:
            cleanup_errors.append(error)
    try:
        process.wait(timeout=2)
    except BaseException as error:
        cleanup_errors.append(error)
        try:
            process.kill()
        except ProcessLookupError:
            pass
        except BaseException as kill_error:
            cleanup_errors.append(kill_error)
        try:
            process.wait(timeout=2)
        except BaseException as reap_error:
            cleanup_errors.append(reap_error)
    try:
        if process.poll() is None:
            cleanup_errors.append(OSError(
                "Windows containment unknown: child remains live after attach cleanup"))
    except BaseException as error:
        cleanup_errors.append(error)
    if job is not None:
        _close_windows_job(job, cleanup_errors)
    _raise_lifecycle_error(primary_error, cleanup_errors)


def _stop(process):
    # POSIX jobs start in their own session. Windows jobs own the complete
    # descendant tree and are closed by the supervisor after reaping.
    if os.name == "nt":
        job = getattr(process, "_kyberia_windows_job", None)
        poll = getattr(process, "poll", None)
        errors = []
        try:
            # A direct child may have exited while a descendant still owns
            # an inherited pipe. Terminate the owned job even after the
            # parent reports an exit so containment does not depend on the
            # kill-on-close operation racing the descendant.
            child_live = poll is None or poll() is None
            if job is not None:
                job.terminate()
            elif child_live:
                process.kill()
        except ProcessLookupError:
            pass
        except BaseException as error:
            errors.append(error)
        try:
            process.wait(timeout=2)
        except BaseException as error:
            errors.append(error)
        if errors:
            still_live = True
            try:
                still_live = process.poll() is None
            except BaseException as error:
                errors.append(error)
            if still_live:
                try:
                    process.kill()
                except ProcessLookupError:
                    pass
                except BaseException as error:
                    errors.append(error)
                try:
                    process.wait(timeout=2)
                except BaseException as error:
                    errors.append(error)
                try:
                    still_live = process.poll() is None
                except BaseException as error:
                    errors.append(error)
                    still_live = True
                if still_live:
                    errors.append(OSError(
                        "Windows containment unknown: child remains live after direct kill"))
        if errors:
            raise OSError("Windows process termination/reap failed: "
                          + "; ".join(str(error) for error in errors)) from errors[0]
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


def _put_pipe_event(events, event, stop_event, terminal=False):
    target = events.terminal if terminal else events.data
    while not stop_event.is_set():
        try:
            target.put(event, timeout=0.05)
            return True
        except queue.Full:
            continue
    return False


def _pipe_reader(stream, label, events, stop_event):
    try:
        while True:
            chunk = stream.read(_WINDOW_PIPE_READ_BYTES)
            if not chunk:
                break
            if not _put_pipe_event(events, ("data", label, chunk), stop_event):
                return
    except (OSError, ValueError):
        pass
    except BaseException as error:
        _put_pipe_event(events, ("pipe_error", label, str(error).encode()[:1024]),
                        stop_event, terminal=True)
    finally:
        _put_pipe_event(events, ("eof", label, b""), stop_event, terminal=True)


def _pipe_writer(stream, payload, events, stop_event):
    try:
        stream.write(payload)
        stream.flush()
    except (BrokenPipeError, OSError, ValueError):
        pass
    except BaseException as error:
        _put_pipe_event(events, ("pipe_error", "stdin", str(error).encode()[:1024]),
                        stop_event, terminal=True)
    finally:
        try:
            stream.close()
        except BaseException as error:
            _put_pipe_event(events, ("pipe_error", "stdin", str(error).encode()[:1024]),
                            stop_event, terminal=True)
        _put_pipe_event(events, ("stdin_done", "stdin", b""), stop_event, terminal=True)


def _consume_pipe_events(events, output, logs, eof):
    status = "exited"
    # Terminal channels are separate from data channels, so a hostile data
    # stream cannot prevent EOF/error delivery from any other pipe.
    for pipe in events.values():
        while True:
            try:
                kind, label, chunk = pipe.terminal.get_nowait()
            except queue.Empty:
                break
            if kind == "eof":
                eof.add(label)
            elif kind == "pipe_error":
                status = "process_error"
    for label in ("stdout", "stderr"):
        pipe = events[label]
        while True:
            try:
                kind, _, chunk = pipe.data.get_nowait()
            except queue.Empty:
                break
            if kind != "data":
                continue
            target = output if label == "stdout" else logs
            bound = MAX_RESULT_BYTES if label == "stdout" else MAX_LOG_BYTES
            remaining = bound - len(target)
            if len(chunk) >= remaining:
                target.extend(chunk[:max(0, remaining)])
                status = "output_limit"
            else:
                target.extend(chunk)
    return status


def _close_pipes(process):
    errors = []
    for stream in (process.stdin, process.stdout, process.stderr):
        if stream is None:
            continue
        try:
            stream.close()
        except BaseException as error:
            errors.append(error)
    return errors


def _join_pipe_threads(threads):
    errors = []
    for label, thread in threads:
        thread.join(timeout=_WINDOW_PIPE_JOIN_S)
        if thread.is_alive():
            errors.append(OSError(f"Windows {label} pipe thread did not stop"))
    return errors


def _cpu_enforcement_provenance(cpu_s, execution=None, worker_response=None):
    """Report CPU-limit intent and confirmation without inferring enforcement."""
    if cpu_s is None:
        return {"requested_s": None, "enforced": False, "status": "not_requested",
                "mechanism": "none"}
    if os.name == "posix":
        mechanism = "posix_setrlimit"
    elif os.name == "nt":
        mechanism = "windows_job_object"
    else:
        return {"requested_s": cpu_s, "enforced": False, "status": "unsupported",
                "mechanism": "unsupported"}
    if (execution is not None
            and b"posix_resource_unavailable" in execution.get("stderr", b"")):
        return {"requested_s": cpu_s, "enforced": False, "status": "unsupported",
                "mechanism": mechanism}
    confirmed = (execution is not None
                 and execution.get("state") == "exited"
                 and execution.get("returncode") == 0
                 and worker_response is not None)
    return {"requested_s": cpu_s, "enforced": True if confirmed else None,
            "status": "enforced" if confirmed else "not_confirmed",
            "mechanism": mechanism}


def _supervise_windows(process, request_bytes, timeout_s, cancel, job=None):
    """Use threads for Windows anonymous pipes, which SelectSelector cannot poll."""
    start = time.monotonic()
    output, logs = bytearray(), bytearray()
    events = {"stdout": _PipeEvents(_WINDOW_PIPE_STDOUT_EVENTS),
              "stderr": _PipeEvents(_WINDOW_PIPE_STDERR_EVENTS),
              "stdin": _PipeEvents(1)}
    stop_event = threading.Event()
    eof = set()
    threads = []
    status = "exited"
    cleanup_error = None
    cancel_error = None
    cancel_requested = False
    try:
        for stream, label in ((process.stdout, "stdout"), (process.stderr, "stderr")):
            thread = threading.Thread(target=_pipe_reader,
                                       args=(stream, label, events[label], stop_event), daemon=True)
            thread.start()
            threads.append((label, thread))
        writer = threading.Thread(target=_pipe_writer,
                                  args=(process.stdin, request_bytes, events["stdin"], stop_event), daemon=True)
        writer.start()
        threads.append(("stdin writer", writer))
        while True:
            status = _consume_pipe_events(events, output, logs, eof) if status == "exited" else status
            if status != "exited":
                break
            if cancel is not None and cancel.is_set():
                status = "cancelled"
                cancel_requested = True
                try:
                    if job is not None:
                        job.request_cancel(process)
                    else:
                        process.send_signal(getattr(signal, "CTRL_BREAK_EVENT", 1))
                except (OSError, AttributeError) as error:
                    cancel_error = str(error)[:1024]
                cancel_deadline = time.monotonic() + _WINDOW_CANCEL_GRACE_S
                while process.poll() is None and time.monotonic() < cancel_deadline:
                    _consume_pipe_events(events, output, logs, eof)
                    time.sleep(0.01)
                break
            if time.monotonic() - start >= timeout_s:
                status = "timed_out"
                break
            if process.poll() is not None:
                break
            time.sleep(0.01)
    finally:
        cleanup_errors = []
        try:
            _stop(process)
        except BaseException as error:
            cleanup_errors.append(error)
            status = "process_error"
        if job is not None:
            # Closing a kill-on-close job also removes descendants that
            # inherited either output handle after the parent exited.
            _close_windows_job(job, cleanup_errors)
            if cleanup_errors:
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
            # Closing the parent pipe handles is the bounded interrupt for a
            # native read/write that did not observe process termination.
            cleanup_errors.extend(_close_pipes(process))
            interrupt_deadline = time.monotonic() + _WINDOW_PIPE_JOIN_S
            while eof != {"stdout", "stderr"} and time.monotonic() < interrupt_deadline:
                drained = _consume_pipe_events(events, output, logs, eof)
                if status == "exited" and drained != "exited":
                    status = drained
                if eof == {"stdout", "stderr"}:
                    break
                time.sleep(0.005)
        if eof != {"stdout", "stderr"}:
            cleanup_errors.append(
                OSError("pipe readers did not reach EOF before cleanup bound"))
            status = "process_error"
        stop_event.set()
        cleanup_errors.extend(_close_pipes(process))
        cleanup_errors.extend(_join_pipe_threads(threads))
        if cleanup_errors:
            cleanup_error = "; ".join(str(error) for error in cleanup_errors)[:1024]
            status = "process_error"
        drained = _consume_pipe_events(events, output, logs, eof)
        if status == "exited" and drained != "exited":
            status = drained
    return {"state": status, "returncode": process.returncode,
            "stdout": bytes(output), "stderr": bytes(logs),
            "elapsed_s": time.monotonic() - start, "cleanup_error": cleanup_error,
            "cancel_error": cancel_error if cancel_requested else None}


def supervise(command, request_bytes, timeout_s, cancel=None, env=None, cpu_s=None):
    """Trusted caller supplies command; protocol cannot select executables or paths."""
    if len(request_bytes) > MAX_REQUEST_BYTES:
        raise ContractError("request exceeds byte limit")
    start = time.monotonic()
    output, logs = bytearray(), bytearray()
    if os.name == "nt":
        # Popen.__exit__ waits without a timeout when returncode is still None.
        # Windows supervision owns the child explicitly so a structured cleanup
        # failure can return without an unbounded context-manager wait.
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, **_popen_options(env))
        try:
            job = _attach_windows_job(process, cpu_s)
            return _supervise_windows(process, request_bytes, timeout_s, cancel, job)
        except BaseException as primary_error:
            cleanup_errors = []
            try:
                if process.poll() is None:
                    _stop(process)
            except BaseException as error:
                cleanup_errors.append(error)
            cleanup_errors.extend(_close_pipes(process))
            job = getattr(process, "_kyberia_windows_job", None)
            if job is not None:
                _close_windows_job(job, cleanup_errors)
            _raise_lifecycle_error(primary_error, cleanup_errors)
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, **_popen_options(env)) as process:
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
    cpu_s = request.get("limits", {}).get("cpu_s", 30)
    # Runtime paths belong to the trusted launcher, never supplied by a scene/request.
    env = {k: v for k, v in os.environ.items() if k not in ("PYTHONPATH", "PYTHONHOME")}
    execution = supervise([str(python_executable), "-I", str(worker)], payload, timeout, cancel,
                          env, cpu_s)
    envelope = {"schema_version": 1, "request_id": request["request_id"],
                "request_sha256": digest(request), "started_utc": started,
                "ended_utc": datetime.now(timezone.utc).isoformat(),
                "elapsed_s": execution["elapsed_s"], "returncode": execution["returncode"],
                "log_sha256": hashlib.sha256(execution["stderr"]).hexdigest(),
                "log": execution["stderr"].decode("utf-8", errors="replace"),
                "cleanup_error": execution.get("cleanup_error"),
                "cancel_error": execution.get("cancel_error"),
                "resource_limits": {"wall_timeout_s": timeout,
                    "cpu_s": cpu_s,
                    "cpu_enforcement": _cpu_enforcement_provenance(cpu_s, execution),
                    "request_bytes": MAX_REQUEST_BYTES, "result_bytes": MAX_RESULT_BYTES,
                    "log_bytes": MAX_LOG_BYTES, "hard_memory_limit": "not_implemented"}}
    if execution["state"] != "exited" or execution["returncode"] != 0:
        envelope.update(status="failed", error=(execution["state"] if execution["state"] != "exited"
                                              else "worker_crashed"))
        return envelope
    try:
        response = decode(execution["stdout"], MAX_RESULT_BYTES)
        validate_result(response, request)
        envelope["resource_limits"]["cpu_enforcement"] = (
            _cpu_enforcement_provenance(cpu_s, execution, response))
        envelope["result"] = response
        envelope["status"] = response["status"]
        if response["status"] == "failed":
            envelope["error"] = response.get("error", "engine_failure")
    except (ContractError, ValueError, TypeError, RecursionError):
        envelope.update(status="failed", error="invalid_worker_response")
    return envelope
