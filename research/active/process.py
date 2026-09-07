"""Bounded POSIX process supervision; no shell or request-supplied commands."""
import hashlib
import os
from pathlib import Path
import platform
import re
import selectors
import select
import signal
import subprocess
import time
import uuid
from dataclasses import asdict, dataclass

from .contract import Invalid, MAX_JSON, VERSION, Request, number, parse_result

MAX_STDERR = 16384


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


def _stop(child):
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
    """Internal trusted argv only. Pipes are capped; owned groups are stopped and the direct child is reaped."""
    number(timeout_s, 0.05, 12)
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
                                 stderr=subprocess.PIPE, shell=False, start_new_session=True,
                                 env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"})
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
