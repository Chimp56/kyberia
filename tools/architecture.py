#!/usr/bin/env python3
"""Check declared workspace dependency direction against reviewed layer policy."""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def check(metadata, policy):
    errors = []
    if policy.get("schema_version") != 1:
        return ["unsupported architecture policy schema"]
    rules = policy["packages"]
    packages = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    if set(packages) != set(rules):
        errors.append("every workspace crate must have exactly one reviewed layer entry")
    for name, package in packages.items():
        if name not in rules:
            continue
        rule = rules[name]
        layer = rule["layer"]
        if layer not in policy["allowed_internal_layers"]:
            errors.append(name + ": unknown layer")
            continue
        for dependency in package["dependencies"]:
            # Test-only dependencies cannot enter production. Build dependencies
            # and target/optional production dependencies must still pass.
            if dependency["kind"] == "dev":
                continue
            target = dependency["name"]
            if target in rules:
                if rules[target]["layer"] not in policy["allowed_internal_layers"][layer]:
                    errors.append(name + " -> " + target + ": forbidden outward dependency")
                expected_path = Path(packages[target]["manifest_path"]).parent.resolve() if target in packages else None
                if not dependency.get("path") or Path(dependency["path"]).resolve() != expected_path:
                    errors.append(name + " -> " + target + ": workspace identity/path mismatch")
            elif target not in rule["external_dependencies"]:
                errors.append(name + " -> " + target + ": external dependency requires architecture review")
    return errors


def main():
    result = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked", "--offline"],
                            cwd=ROOT, check=True, capture_output=True, text=True)
    errors = check(json.loads(result.stdout), json.loads((ROOT / "tools/architecture.json").read_text()))
    if errors:
        raise SystemExit("FAIL:\n" + "\n".join(errors))
    print("PASS: reviewed dependency directions and external package boundaries")


if __name__ == "__main__":
    main()
