#!/usr/bin/env python3
"""Verify the isolated parser QA dependency and tool source inventory."""

import argparse
import hashlib
import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parent
INVENTORY = ROOT / "source-inventory.json"
LOCK = ROOT / "Cargo.lock"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def one_archive(cargo_home, name, version):
    matches = list((cargo_home / "registry" / "cache").glob(
        "*/{}-{}.crate".format(name, version)
    ))
    if len(matches) != 1:
        raise SystemExit("expected one cached archive for {} {}, got {}".format(
            name, version, len(matches)
        ))
    return matches[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-home", required=True, type=Path)
    args = parser.parse_args()
    cargo_home = args.cargo_home.resolve()
    inventory = json.loads(INVENTORY.read_text(encoding="utf-8"))
    if digest(LOCK) != inventory["cargo_lock_sha256"]:
        raise SystemExit("fuzz Cargo.lock digest is stale")

    environment = os.environ.copy()
    environment["CARGO_HOME"] = str(cargo_home)
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(ROOT / "Cargo.toml"),
            "--locked",
            "--offline",
            "--format-version",
            "1",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=environment,
    )
    metadata = json.loads(result.stdout)
    actual = {
        (package["name"], package["version"]): package
        for package in metadata["packages"]
        if package["source"] is not None
    }
    recorded = {
        (package["name"], package["version"]): package
        for package in inventory["packages"]
    }
    if actual.keys() != recorded.keys():
        raise SystemExit("metadata package identities differ from source inventory")
    for identity, package in actual.items():
        evidence = recorded[identity]
        if package.get("license") != evidence["license"]:
            raise SystemExit("license differs for {} {}".format(*identity))
        archive = one_archive(cargo_home, *identity)
        if digest(archive) != evidence["archive_sha256"]:
            raise SystemExit("archive digest differs for {} {}".format(*identity))

    tool = inventory["cargo_fuzz"]
    tool_archive = one_archive(cargo_home, "cargo-fuzz", tool["version"])
    if digest(tool_archive) != tool["crate_archive_sha256"]:
        raise SystemExit("cargo-fuzz archive digest differs")
    print("PASS: {} locked fuzz packages and cargo-fuzz {}".format(
        len(actual), tool["version"]
    ))


if __name__ == "__main__":
    main()
