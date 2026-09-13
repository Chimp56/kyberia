"""One JSON document on stdin; one bounded engine-neutral result on stdout."""

import argparse
import os
from pathlib import Path
import queue
import signal
import selectors
import sys
import threading
import time

from rfatlas_sionna.client import run
from rfatlas_sionna.contract import ContractError, MAX_REQUEST_BYTES, canonical_bytes, decode


def read_request(cancellation):
    if os.name == "nt":
        # Windows anonymous stdin pipes cannot be registered with
        # SelectSelector.  Keep the blocking read in a daemon thread so the
        # signal handler can still cancel an incomplete request promptly.
        result = queue.Queue(maxsize=1)

        def read_blocking():
            try:
                result.put(("ok", sys.stdin.buffer.read(MAX_REQUEST_BYTES + 1)))
            except (OSError, ValueError) as error:
                result.put(("error", error))

        threading.Thread(target=read_blocking, daemon=True).start()
        deadline = time.monotonic() + 5
        while True:
            if cancellation.is_set():
                raise ContractError("cancelled while reading request")
            if time.monotonic() >= deadline:
                raise ContractError("request input deadline exceeded")
            try:
                state, value = result.get(timeout=0.02)
            except queue.Empty:
                continue
            if state == "error":
                raise value
            if len(value) > MAX_REQUEST_BYTES:
                raise ContractError("request exceeds byte limit")
            return value
    data = bytearray()
    descriptor = sys.stdin.fileno()
    os.set_blocking(descriptor, False)
    deadline = time.monotonic() + 5
    with selectors.SelectSelector() as selector:
        # SelectSelector also accepts redirected regular files on macOS.
        selector.register(descriptor, selectors.EVENT_READ)
        while True:
            if cancellation.is_set():
                raise ContractError("cancelled while reading request")
            if time.monotonic() >= deadline:
                raise ContractError("request input deadline exceeded")
            if not selector.select(0.02):
                continue
            if cancellation.is_set():
                raise ContractError("cancelled while reading request")
            chunk = os.read(descriptor, min(8192, MAX_REQUEST_BYTES+1-len(data)))
            if not chunk:
                return bytes(data)
            data.extend(chunk)
            if len(data) > MAX_REQUEST_BYTES:
                raise ContractError("request exceeds byte limit")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--python", type=Path, default=Path(sys.executable))
    args = parser.parse_args()
    cancellation = threading.Event()
    cancel_signal = lambda *_: cancellation.set()
    signal.signal(signal.SIGINT, cancel_signal)
    signal.signal(signal.SIGTERM, cancel_signal)
    if hasattr(signal, "SIGBREAK"):
        signal.signal(signal.SIGBREAK, cancel_signal)
    try:
        result = run(decode(read_request(cancellation)), args.python, cancellation)
    except (ContractError, ValueError, OSError) as error:
        if cancellation.is_set():
            result = {"schema_version": 1, "status": "failed", "error": "cancelled"}
        else:
            result = {"schema_version": 1, "status": "failed", "error": "invalid_request_or_runtime",
                      "detail": str(error)}
    sys.stdout.buffer.write(canonical_bytes(result) + b"\n")
    return 0 if result["status"] == "completed" else 2


if __name__ == "__main__":
    raise SystemExit(main())
