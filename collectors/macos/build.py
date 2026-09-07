#!/usr/bin/env python3
"""Build the native CoreWLAN process; all generated files remain in .build."""
import hashlib
from pathlib import Path
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent


def main():
    if platform.system() != "Darwin":
        print("CoreWLAN build requires macOS and Apple SDK", file=sys.stderr)
        return 69
    sources = sorted((ROOT / "Sources").glob("*.swift"))
    digest = hashlib.sha256()
    for path in sources + [ROOT / "Info.plist"]:
        digest.update(path.name.encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
    build = ROOT / ".build"
    build.mkdir(exist_ok=True)
    metadata = build / "BuildMetadata.swift"
    metadata.write_text('let collectorBuild = "sha256:' + digest.hexdigest() + '"\n')
    bundle = build / "KyberiaCollector.app"
    binaries = bundle / "Contents" / "MacOS"
    binaries.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "Info.plist", bundle / "Contents" / "Info.plist")
    output = binaries / "kyberia-macos-collector"
    command = ["xcrun", "swiftc", "-swift-version", "5", "-warnings-as-errors", "-O",
               "-module-cache-path", str(build / "module-cache"),
               "-framework", "CoreWLAN", "-framework", "CoreLocation",
               "-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist",
               "-Xlinker", str(ROOT / "Info.plist"), *map(str, sources), str(metadata), "-o", str(output)]
    result = subprocess.run(command, timeout=180)
    if result.returncode == 0:
        signed = subprocess.run(["codesign", "--force", "--sign", "-", str(bundle)], timeout=30)
        if signed.returncode != 0:
            return signed.returncode
        verified = subprocess.run(["codesign", "--verify", "--strict", str(bundle)], timeout=30)
        if verified.returncode != 0:
            return verified.returncode
        print(output)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
