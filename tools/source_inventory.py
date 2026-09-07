#!/usr/bin/env python3
"""Reproduce the resolved Cargo source inventory from locked package metadata."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path
try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/licenses/cargo-sources.json"


def inventory(metadata, lock_bytes):
    locked = {}
    for package in tomllib.loads(lock_bytes.decode("utf-8"))["package"]:
        key = (package["name"], package["version"], package.get("source"))
        if key in locked:
            raise ValueError("duplicate locked package")
        locked[key] = package
    packages = []
    seen = set()
    for package in metadata["packages"]:
        key = (package["name"], package["version"], package["source"])
        if key not in locked or key in seen:
            raise ValueError("metadata does not match locked package identities")
        seen.add(key)
        if package["source"] is None:
            if package["id"] not in metadata["workspace_members"]:
                raise ValueError("nonworkspace path dependency needs explicit provenance review")
            continue
        if not package["source"].startswith("registry+"):
            raise ValueError("nonregistry dependency needs explicit provenance review")
        registry_source = Path(package["manifest_path"]).parent.parent
        archive = registry_source.parent.parent / "cache" / registry_source.name / (package["name"] + "-" + package["version"] + ".crate")
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        if checksum != locked[key].get("checksum"):
            raise ValueError("cached archive differs from Cargo.lock checksum: " + package["name"])
        if not package.get("license"):
            raise ValueError("missing license declaration: " + package["name"])
        packages.append({
            "name": package["name"], "version": package["version"],
            "source": package["source"], "repository": package.get("repository"),
            "license_declared": package["license"], "archive_sha256": checksum,
            "redistribution": "Source inventory only; retain upstream licenses and review bundled native notices before distribution",
            "transformation": "Unmodified registry package; compiled by Cargo when selected by target/features",
            "provenance": "Cargo.lock, Cargo metadata and SHA-256 of downloaded registry .crate archive",
            "update_procedure": "Review new upstream version/license, update lock, regenerate inventory, run affected tests and dependency audit",
        })
    if seen != set(locked):
        raise ValueError("metadata omits locked packages")
    return {"schema_version": 1, "scope": "All resolved targets and development dependencies; not a shipped-binary SBOM",
            "cargo_lock_sha256": hashlib.sha256(lock_bytes).hexdigest(),
            "packages": sorted(packages, key=lambda p: (p["name"], p["version"]))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["generate", "check"])
    args = parser.parse_args()
    result = subprocess.run(["cargo", "metadata", "--format-version", "1", "--locked", "--offline"],
                            cwd=ROOT, check=True, capture_output=True, text=True)
    value = inventory(json.loads(result.stdout), (ROOT / "Cargo.lock").read_bytes())
    content = json.dumps(value, indent=2, ensure_ascii=False) + "\n"
    if args.mode == "generate":
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_text(content)
    elif not OUTPUT.exists() or OUTPUT.read_text() != content:
        raise SystemExit("FAIL: stale Cargo source inventory; regenerate and review")
    print("PASS: {} locked external packages".format(len(value["packages"])))


if __name__ == "__main__":
    main()
