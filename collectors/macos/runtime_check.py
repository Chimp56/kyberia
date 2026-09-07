#!/usr/bin/env python3
"""Run actual redacted probe/scan; retain only status/count/version evidence."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import subprocess

from contract import decode_stream

ROOT = Path(__file__).resolve().parent
BINARY = ROOT / ".build/KyberiaCollector.app/Contents/MacOS/kyberia-macos-collector"


def validate_exit(complete, returncode):
    expected = {"ok": 0, "partial": 2, "permission_required": 77, "unsupported": 69,
                "unavailable": 69, "error": 70, "timeout": 124}
    if complete["status"] == "cancelled":
        code = {"sigint": 130, "sigterm": 143}.get(complete["reason"])
    else:
        code = expected.get(complete["status"])
    if code is None or returncode != code:
        raise ValueError("exit code and native terminal state disagree")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--execution-context", choices=["default_workspace_sandbox", "outside_workspace_sandbox"], required=True)
    args = parser.parse_args()
    results = []
    for command in ["probe", "scan"]:
        argv = [str(BINARY), command, "--timeout-seconds", "5", "--limit", "16"]
        executed = subprocess.run(argv, capture_output=True, timeout=8)
        events = decode_stream(executed.stdout)
        hello, complete = events[0], events[-1]
        capability = next((event for event in events if event["kind"] == "capabilities"), None)
        validate_exit(complete, executed.returncode)
        results.append({"command": ["kyberia-macos-collector", *argv[1:]], "exit_code": executed.returncode,
                        "collector_version": hello["collector_version"], "collector_build": hello["collector_build"],
                        "native_os_version": hello["os_version"], "identifier_policy": hello["identifier_policy"],
                        "status": complete["status"], "reason": complete["reason"],
                        "observation_count": complete["observation_count"], "record_count": len(events),
                        "source_count": len(capability["sources"]) if capability else None,
                        "location_authorization": capability["location_authorization"] if capability else None,
                        "location_services_enabled": capability["location_services_enabled"] if capability else None,
                        "stderr_bytes": len(executed.stderr), "contract_validation": "PASS"})
    report = {"schema_version": 1, "evidence_kind": "native_runtime_redacted_summary",
              "executed_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
              "execution_context": args.execution_context,
              "binary_sha256": hashlib.sha256(BINARY.read_bytes()).hexdigest(),
              "machine_architecture": platform.machine(), "python_version": platform.python_version(),
              "runs": results, "permission_prompt_requested": False,
              "limitations": ["No raw SSID/BSSID/interface identifiers or raw output retained in this report.",
                              "An empty interface list reports this process's API access, not physical absence of radios.",
                              "Permission-denied/unavailable results do not validate authorized scan metrics or full Gate A."]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
