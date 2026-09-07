"""Explicit local research CLI. Executable and request file are operator configuration."""
import argparse
import json
from pathlib import Path
import signal
import sys
import threading

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from research.active.contract import Invalid, Request
from research.active.process import run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--request", type=Path, required=True)
    args = parser.parse_args()
    cancelled = threading.Event()
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda *_: cancelled.set())
    try:
        with args.request.open("rb") as source:
            request = Request.decode(source.read(4097))
        result = run(request, args.binary, cancelled)
    except (Invalid, OSError) as exc:
        result = {"schema_version": "1", "status": "invalid_request", "reason": str(exc), "measurement": None}
    print(json.dumps(result, sort_keys=True, allow_nan=False))
    return 0 if result["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
