"""One JSON document on stdin; one bounded engine-neutral result on stdout."""

import argparse
import os
from pathlib import Path
import signal
import selectors
import sys
import threading
import time

from rfatlas_sionna.client import run
from rfatlas_sionna.contract import ContractError, MAX_REQUEST_BYTES, canonical_bytes, decode


def read_request(cancellation):
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
    signal.signal(signal.SIGINT, lambda *_: cancellation.set())
    signal.signal(signal.SIGTERM, lambda *_: cancellation.set())
    try:
        result = run(decode(read_request(cancellation)), args.python, cancellation)
    except (ContractError, ValueError, OSError) as error:
        result = {"schema_version": 1, "status": "failed", "error": "invalid_request_or_runtime",
                  "detail": str(error)}
    sys.stdout.buffer.write(canonical_bytes(result) + b"\n")
    return 0 if result["status"] == "completed" else 2


if __name__ == "__main__":
    raise SystemExit(main())
