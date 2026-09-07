"""Offline exact-source wheel build, checksum-checked before installing dependencies."""

import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tarfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--python", type=Path, required=True)
    parser.add_argument("--ledger", type=Path, default=Path("docs/licenses/sionna-sources.json"))
    args = parser.parse_args()
    import json
    ledger = json.loads(args.ledger.read_text())
    if hashlib.sha256(args.archive.read_bytes()).hexdigest() != ledger["source_archive_sha256"]:
        raise ValueError("audited source archive checksum mismatch")
    target = Path(".tools") / ("sionna-build-" + str(os.getpid()))
    target.mkdir(parents=True, exist_ok=False)
    with tarfile.open(args.archive, "r:gz") as archive:
        members = []
        for member in archive.getmembers():
            path = Path(member.name)
            # Upstream documentation-only link is unused by package builds; never create it.
            if member.issym() and member.name == "sionna-rt-"+ledger["audited_revision"]+"/doc/source/tutorials" and member.linkname == "../../tutorials":
                continue
            if path.is_absolute() or ".." in path.parts or not (member.isfile() or member.isdir()):
                raise ValueError("unsupported source archive entry")
            members.append(member)
        archive.extractall(target, members=members, filter="data")
    source = target / ("sionna-rt-" + ledger["audited_revision"])
    wheels = target / "wheels"
    env = dict(os.environ, SOURCE_DATE_EPOCH="1786419474")
    subprocess.run([str(args.python), "-m", "pip", "wheel", "--no-cache-dir", "--no-deps",
                    "--no-build-isolation", "--wheel-dir", str(wheels), str(source)],
                   env=env, check=True)
    wheel = wheels / "sionna_rt-2.0.1-py3-none-any.whl"
    expected = next(x["wheel_sha256"] for x in ledger["packages"] if x["name"] == "sionna-rt")
    data = wheel.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("wheel not reproducible with the recorded build dependencies")
    destination = Path(".tools/sionna-wheels")
    destination.mkdir(parents=True, exist_ok=True)
    installed = destination / wheel.name
    if installed.exists() and installed.read_bytes() != data:
        raise ValueError("existing wheel differs; preserve it and investigate")
    installed.write_bytes(data)
    print("Verified exact audited wheel:", installed)


if __name__ == "__main__":
    main()
