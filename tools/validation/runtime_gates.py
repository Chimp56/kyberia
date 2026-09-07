#!/usr/bin/env python3
"""Check evidence for explicit runtime gates; never turn mock tests into hardware proof."""

import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import sys
from datetime import datetime

ROOT = Path(__file__).resolve().parents[2]
CATALOG = Path(__file__).with_name("gates.json")
MAX_JSON_BYTES = 1024 * 1024
MAX_EVIDENCE_BYTES = 16 * 1024 * 1024
STATUSES = {"PASS", "FAIL", "NOT_RUN", "BLOCKED_EXTERNAL"}
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
REVISION = re.compile(r"[0-9a-f]{40}\Z")


def load_json(path):
    if path.stat().st_size > MAX_JSON_BYTES:
        raise ValueError("JSON exceeds 1 MiB limit")

    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON key: " + key)
            result[key] = value
        return result

    def finite_float(value):
        parsed = float(value)
        if not math.isfinite(parsed):
            raise ValueError("nonfinite JSON number")
        return parsed

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique,
                      parse_float=finite_float,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError("nonfinite JSON")))


def text_value(value):
    return isinstance(value, str) and bool(value.strip()) and value.strip().lower() not in {
        "unknown", "none", "n/a", "todo", "placeholder"}


def catalog():
    data = load_json(CATALOG)
    if data.get("schema_version") != 1:
        raise ValueError("unsupported catalog version")
    gates = {gate["id"]: gate for gate in data["gates"]}
    if len(gates) != len(data["gates"]):
        raise ValueError("duplicate gate id")
    for gate in gates.values():
        if gate["status"] != "NOT_RUN" or not gate["checks"]:
            raise ValueError("catalog specifies procedures, not runtime results")
        if len(set(gate["checks"])) != len(gate["checks"]):
            raise ValueError("duplicate check")
    return gates


def template(gate):
    return {"schema_version": 1, "gate_id": gate["id"], "status": "NOT_RUN",
            "evidence_kind": gate["evidence_kind"], "executed_at_utc": None,
            "operator": None, "command_argv": [], "exit_code": None,
            "versions": {key: None for key in gate["required_versions"]},
            "hardware": {key: None for key in gate["required_hardware"]},
            "checks": {key: {"status": "NOT_RUN", "evidence": [], "notes": ""}
                       for key in gate["checks"]},
            "external_blocker": None}


def verify_reference(ref, base):
    if not isinstance(ref, dict) or set(ref) != {"path", "sha256"}:
        raise ValueError("evidence reference needs path and sha256")
    if (not isinstance(ref["path"], str) or not isinstance(ref["sha256"], str)
            or not SHA256.fullmatch(ref["sha256"])):
        raise ValueError("invalid evidence reference")
    path = Path(ref["path"])
    if path.is_absolute() or not path.parts or any(p in {"..", "."} for p in path.parts):
        raise ValueError("evidence must be a relative file within evidence directory")
    target = base / path
    if any((base / Path(*path.parts[:i])).is_symlink() for i in range(1, len(path.parts) + 1)):
        raise ValueError("evidence symlinks are forbidden")
    if not target.resolve().is_relative_to(base.resolve()):
        raise ValueError("evidence path escapes directory")
    if not target.is_file() or not 0 < target.stat().st_size <= MAX_EVIDENCE_BYTES:
        raise ValueError("evidence missing, empty, or exceeds 16 MiB")
    digest = hashlib.sha256(target.read_bytes()).hexdigest()
    if digest != ref["sha256"]:
        raise ValueError("evidence checksum mismatch: " + ref["path"])


def validate(document, gate, base):
    """Return structural/evidence errors. A clean NOT_RUN is still not a passed gate."""
    errors = []
    if not isinstance(document, dict):
        return ["result must be an object"]
    if (type(document.get("schema_version")) is not int or document.get("schema_version") != 1
            or document.get("gate_id") != gate["id"]):
        errors.append("schema version or gate id mismatch")
    status = document.get("status")
    if not isinstance(status, str) or status not in STATUSES:
        return errors + ["invalid gate status"]
    if document.get("evidence_kind") != gate["evidence_kind"]:
        errors.append("wrong evidence kind; synthetic/contract evidence cannot satisfy runtime/field gates")
    checks = document.get("checks")
    if not isinstance(checks, dict) or set(checks) != set(gate["checks"]):
        errors.append("checks must exactly cover the catalog requirements")
        checks = {}
    for name, check in checks.items():
        if (not isinstance(check, dict) or not isinstance(check.get("status"), str)
                or check.get("status") not in STATUSES):
            errors.append("invalid check: " + name)
            continue
        if status == "PASS" and check["status"] != "PASS":
            errors.append("required check has not passed: " + name)
        if check["status"] in {"PASS", "FAIL", "BLOCKED_EXTERNAL"}:
            evidence = check.get("evidence")
            if not isinstance(evidence, list) or not evidence:
                errors.append("missing evidence for check: " + name)
            else:
                for reference in evidence:
                    try:
                        verify_reference(reference, base)
                    except (ValueError, OSError) as error:
                        errors.append(name + ": " + str(error))
        if check["status"] == "BLOCKED_EXTERNAL" and status != "BLOCKED_EXTERNAL":
            errors.append("blocked checks require gate blocker metadata")
    executed = [check.get("status") for check in checks.values() if isinstance(check, dict)
                and isinstance(check.get("status"), str) and check.get("status") in {"PASS", "FAIL"}]
    if status == "NOT_RUN" and executed:
        errors.append("NOT_RUN cannot contain executed checks")
    if status == "FAIL" and "FAIL" not in executed:
        errors.append("FAIL must identify a failed check")
    if status in {"PASS", "FAIL"} or executed:
        if not text_value(document.get("operator")):
            errors.append("execution operator missing")
        try:
            timestamp = document["executed_at_utc"]
            if not isinstance(timestamp, str) or "T" not in timestamp or not timestamp.endswith("Z"):
                raise ValueError("UTC required")
            parsed = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
            if parsed.tzinfo is None or parsed.utcoffset().total_seconds() != 0:
                raise ValueError("timezone-aware UTC required")
        except (KeyError, ValueError, TypeError):
            errors.append("valid executed_at_utc ending Z required")
        argv = document.get("command_argv")
        if not isinstance(argv, list) or not argv or not all(text_value(arg) for arg in argv):
            errors.append("exact executed command argv required")
        code = document.get("exit_code")
        if type(code) is not int or (status == "PASS" and code != 0):
            errors.append("passing execution requires integer exit code 0")
    if status == "PASS" or "PASS" in executed:
        for field, requirements in [("versions", "required_versions"), ("hardware", "required_hardware")]:
            values = document.get(field)
            if not isinstance(values, dict):
                errors.append(field + " metadata missing")
                continue
            for key in gate[requirements]:
                if not text_value(values.get(key)):
                    errors.append("concrete " + field + "." + key + " required")
        versions = document.get("versions", {})
        if isinstance(versions, dict):
            for key, expected in gate.get("pinned_versions", {}).items():
                if versions.get(key) != expected:
                    errors.append("unsupported " + key + "; update pin through compatibility review")
        if gate.get("requires_two_revisions") and status == "PASS":
            revisions = document.get("compatibility_revisions")
            if (not isinstance(revisions, list) or len(set(map(str, revisions))) < 2
                    or not all(isinstance(v, str) and REVISION.fullmatch(v) for v in revisions)):
                errors.append("at least two distinct exact compatibility revisions required")
            elif gate["pinned_versions"]["kismet_source_revision"] not in revisions:
                errors.append("compatibility revisions must include the pinned source revision")
    if status == "BLOCKED_EXTERNAL":
        blocker = document.get("external_blocker")
        if not isinstance(blocker, dict):
            errors.append("external blocker metadata missing")
        else:
            if (not isinstance(blocker.get("category"), str) or blocker.get("category") not in
                    {"hardware", "credentials", "legal", "proprietary_data", "os_access", "field_site"}):
                errors.append("blocker must name a genuine external dependency category")
            for field in ["requirement", "dependency", "reason", "resume_procedure"]:
                if not text_value(blocker.get(field)):
                    errors.append("external blocker missing " + field)
            try:
                verify_reference(blocker.get("evidence"), base)
            except (ValueError, OSError, TypeError) as error:
                errors.append("external blocker: " + str(error))
    elif document.get("external_blocker") is not None:
        errors.append("blocker metadata only allowed for BLOCKED_EXTERNAL")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    commands.add_parser("list")
    item = commands.add_parser("template")
    item.add_argument("gate_id")
    item = commands.add_parser("check")
    item.add_argument("result", type=Path)
    item = commands.add_parser("hash")
    item.add_argument("file", type=Path)
    args = parser.parse_args()
    try:
        gates = catalog()
        if args.action == "list":
            print(json.dumps(list(gates.values()), indent=2, sort_keys=True))
        elif args.action == "template":
            print(json.dumps(template(gates[args.gate_id]), indent=2, sort_keys=True))
        elif args.action == "hash":
            if not args.file.is_file() or not 0 < args.file.stat().st_size <= MAX_EVIDENCE_BYTES:
                raise ValueError("evidence missing, empty, or exceeds 16 MiB")
            print(hashlib.sha256(args.file.read_bytes()).hexdigest())
        else:
            data = load_json(args.result)
            gate_id = data.get("gate_id") if isinstance(data, dict) else None
            if gate_id not in gates:
                raise ValueError("unknown gate id")
            errors = validate(data, gates[gate_id], args.result.parent)
            if errors:
                print(json.dumps({"valid": False, "errors": errors}, indent=2))
                return 1
            print(json.dumps({"valid": True, "status": data["status"], "gate_id": gate_id}))
            return 0 if data["status"] == "PASS" else 2
    except (ValueError, KeyError, TypeError, OSError, RecursionError) as error:
        print("gate evidence error: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
