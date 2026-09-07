"""Build a platform wheel lock and source ledger from actual installed metadata."""

import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path
import platform
import re
import sys

from rfatlas_sionna import AUDITED_REVISION


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def normalize(name):
    return re.sub(r"[-_.]+", "-", name).lower()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--pip-report", type=Path, required=True)
    parser.add_argument("--upstream", type=Path, required=True)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--lock", type=Path, required=True)
    parser.add_argument("--source-wheel", type=Path)
    parser.add_argument("--build-report", type=Path)
    parser.add_argument("--source-archive", type=Path)
    parser.add_argument("--previous-ledger", type=Path)
    parser.add_argument("--source-pin", type=Path)
    parser.add_argument("--homebrew-prefix", type=Path)
    args = parser.parse_args()
    downloads = {normalize(x["metadata"]["name"]): x for x in json.loads(args.pip_report.read_text())["install"]}
    if args.build_report:
        for entry in json.loads(args.build_report.read_text())["install"]:
            downloads[normalize(entry["metadata"]["name"])] = entry
    if args.source_wheel:
        downloads["sionna-rt"]["download_info"] = {"url": str(args.source_wheel),
            "archive_info": {"hashes": {"sha256": sha(args.source_wheel)}}}
    installed = sorted(importlib.metadata.distributions(), key=lambda d: normalize(d.metadata["Name"]))
    if set(downloads) != {normalize(d.metadata["Name"]) for d in installed}:
        raise ValueError("resolver and installed environment package sets differ")
    packages, lock = [], ["# CPython 3.12, macOS ARM64 wheel lock. Regenerate/review for other platforms.",
                          "# Exact resolved versions and PyPI wheel hashes, verified against installed metadata.",
                          "--only-binary=:all:"]
    for distribution in installed:
        name, version = distribution.metadata["Name"], distribution.version
        item = downloads[normalize(name)]
        if item["metadata"]["version"] != version:
            raise ValueError("resolver/installed version mismatch")
        wheel = item["download_info"]
        files, licenses, native = [], [], []
        for entry in distribution.files or []:
            path = distribution.locate_file(entry)
            if not path.is_file():
                raise ValueError("missing installed file " + str(entry))
            if path.name in ("METADATA", "RECORD", "WHEEL"):
                files.append({"path": str(entry), "sha256": sha(path)})
            if any(word in str(entry).lower() for word in ("license", "copying", "notice")):
                licenses.append({"path": str(entry), "sha256": sha(path)})
            if path.suffix in (".dylib", ".so", ".dll"):
                native.append({"path": str(entry), "sha256": sha(path)})
        declared = distribution.metadata.get("License-Expression") or distribution.metadata.get("License")
        license_summary = declared if declared and len(declared) < 200 else "See installed license files and metadata"
        packages.append({"name": name, "version": version, "source": "installed Python distribution metadata",
                         "download_url": wheel["url"], "wheel_sha256": wheel["archive_info"]["hashes"]["sha256"],
                         "project_urls": distribution.metadata.get_all("Project-URL") or [],
                         "license_declared": license_summary,
                         "license_classifiers": [x for x in distribution.metadata.get_all("Classifier") or [] if x.startswith("License")],
                         "metadata_files": files, "license_files": licenses, "native_libraries": native,
                         "redistribution_review": "pending; metadata is evidence, not a license grant"})
        requirement = str(args.source_wheel) if normalize(name) == "sionna-rt" and args.source_wheel else name+"=="+version
        lock.append(requirement + " --hash=sha256:" + wheel["archive_info"]["hashes"]["sha256"])
    upstream = args.upstream / "src/sionna/rt"
    root = Path(importlib.metadata.distribution("sionna-rt").locate_file("sionna/rt"))
    comparisons = []
    for source in sorted(upstream.rglob("*.py")):
        relative = source.relative_to(upstream)
        local = root / relative
        comparisons.append({"path": str(relative), "source_sha256": sha(source),
                            "installed_sha256": sha(local) if local.is_file() else None,
                            "matches": local.is_file() and sha(source) == sha(local)})
    ledger = {"schema_version": 1, "scope": "optional_sionna_worker_cpu_environment",
              "platform": platform.platform(), "python": platform.python_version(),
              "python_binary_sha256": sha(Path(sys.executable).resolve()),
              "python_source": "https://github.com/python/cpython/tree/v3.12.12",
              "python_distribution": "https://github.com/astral-sh/python-build-standalone/releases/tag/20260211",
              "python_distribution_license_review": "pending standalone bundled-library review",
              "upstream_source": "https://github.com/NVlabs/sionna-rt/tree/"+AUDITED_REVISION,
              "audited_revision": AUDITED_REVISION, "package_source_comparison": comparisons,
              "all_python_sources_match": all(x["matches"] for x in comparisons),
              "packages": packages, "assets": {"upstream_scene_assets_used": False,
                  "worker_scene": "original empty-space coordinates; no material catalog or mesh asset",
                  "installed_upstream_assets": "present transitively; not copied into source or licensed by this ledger"},
              "release_sbom_complete": False,
              "remaining_review": ["Python standalone bundled native libraries", "LLVM bottle and native transitive notices",
                                   "Sionna packaged example assets", "platform packaging and redistribution review"]}
    if args.source_archive:
        ledger["source_archive_sha256"] = sha(args.source_archive)
    if args.previous_ledger:
        previous = json.loads(args.previous_ledger.read_text())
        ledger["pypi_wheel_mismatch"] = {"version": "2.0.1", "differing_python_files":
            [x for x in previous["package_source_comparison"] if not x["matches"]],
            "pypi_wheel": next(x for x in previous["packages"] if normalize(x["name"]) == "sionna-rt"),
            "resolution": "Build exact audited commit; no claim that PyPI version identity establishes source equivalence."}
    if args.source_pin:
        args.source_pin.write_text(json.dumps({"audited_revision": AUDITED_REVISION,
            "python_files": {x["path"]: x["source_sha256"] for x in comparisons},
            "upstream_test_files": {p: sha(args.upstream / p) for p in (
                "test/conftest.py", "test/unit/test_paths_cir.py", "test/unit/test_radio_maps.py")}}, indent=2)+"\n")
    if args.homebrew_prefix:
        ledger["external_native_packages"] = []
        for name in ("llvm@18", "lz4", "xz", "zstd"):
            package = (args.homebrew_prefix / "opt" / name).resolve()
            receipt = package / "INSTALL_RECEIPT.json"
            formula = package / ".brew" / (name+".rb")
            source = formula.read_text()
            ledger["external_native_packages"].append({"name": name, "installed_prefix": str(package),
                "receipt": json.loads(receipt.read_text()), "receipt_sha256": sha(receipt),
                "formula_sha256": sha(formula),
                "source_url": re.search(r'^  url "([^"]+)"', source, re.M).group(1),
                "declared_license_formula": next(line.strip() for line in source.splitlines() if line.strip().startswith("license ")),
                "native_libraries": [{"path": str(p.relative_to(package)), "sha256": sha(p)}
                    for p in sorted((package / "lib").glob("*.dylib")) if not p.is_symlink()],
                "redistribution_review": "pending; installed Homebrew metadata only"})
    args.ledger.parent.mkdir(parents=True, exist_ok=True)
    args.ledger.write_text(json.dumps(ledger, indent=2)+"\n")
    args.lock.write_text("\n".join(lock)+"\n")
    print(json.dumps({"packages": len(packages), "source_files_compared": len(comparisons),
                      "all_python_sources_match": ledger["all_python_sources_match"]}))


if __name__ == "__main__":
    main()
