"""Bounded one-job process supervision; standard library only, no engine imports."""

from datetime import datetime, timezone
import hashlib
import os
from pathlib import Path
import selectors
import signal
import subprocess
import time

from .contract import (ContractError, MAX_LOG_BYTES, MAX_REQUEST_BYTES, MAX_RESULT_BYTES,
                       canonical_bytes, decode, digest, validate, validate_result)


def _stop(process):
    # All worker jobs start in their own session. Reap after every exit path.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def supervise(command, request_bytes, timeout_s, cancel=None, env=None):
    """Trusted caller supplies command; protocol cannot select executables or paths."""
    if len(request_bytes) > MAX_REQUEST_BYTES:
        raise ContractError("request exceeds byte limit")
    start = time.monotonic()
    output, logs = bytearray(), bytearray()
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, start_new_session=True, env=env) as process:
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
