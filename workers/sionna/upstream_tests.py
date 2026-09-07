"""Run a bounded unmodified upstream CIR/LOS subset and retain its actual output."""

import argparse
from datetime import datetime, timezone
import json
import hashlib
from pathlib import Path
import sys

from rfatlas_sionna import AUDITED_REVISION
from rfatlas_sionna.client import run, supervise


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--upstream", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    pin = json.loads(Path(__file__).with_name("rfatlas_sionna").joinpath("source_pin.json").read_text())
    for relative, expected in pin["upstream_test_files"].items():
        if hashlib.sha256((args.upstream / relative).read_bytes()).hexdigest() != expected:
            raise ValueError("upstream test source differs from audited pin: "+relative)
    capability = run({"schema_version": 1, "request_id": "upstream-environment", "operation": "capabilities"}, sys.executable)
    if capability["status"] != "completed":
        raise ValueError("upstream tests require the verified audited CPU environment")
    command = [sys.executable, "-m", "pytest", "--cpu", "-q",
               str(args.upstream / "test/unit/test_paths_cir.py"),
               str(args.upstream / "test/unit/test_radio_maps.py") + "::test_los"]
    result = supervise(command, b"", 120)
    result["stdout"] = result["stdout"].decode("utf-8", errors="replace")
    result["stderr"] = result["stderr"].decode("utf-8", errors="replace")
    report = {"schema_version": 1, "evidence_kind": "runtime", "source_revision": AUDITED_REVISION,
              "verified_test_files": pin["upstream_test_files"], "runtime": capability["result"]["versions"],
              "command": command, "created_utc": datetime.now(timezone.utc).isoformat(),
              "execution": result, "full_upstream_suite": "NOT_RUN", "selection": "CIR suite and LOS radio-map test",
              "status": "PASS" if result["returncode"] == 0 and result["state"] == "exited" else "FAIL"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+"\n")
    print(report["status"], result["stdout"][-250:])
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
