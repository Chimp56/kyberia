#!/usr/bin/env python3
"""Pinned Rust SBOM and dependency-audit commands for the Kyberia CLI.

The repository intentionally keeps the downloaded binaries, RustSec checkout,
and generated evidence under the ignored ``.tools/supply-chain`` directory.
This module is the small, reviewable boundary around those tools: it validates
downloads before extraction, invokes the real tools, and refuses stale or
portable-looking evidence that does not describe the current source tree.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import time
import urllib.request
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Iterable

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python 3.10 fallback is explicit.
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ModuleNotFoundError:
        tomllib = None  # type: ignore[assignment]


ROOT = Path(__file__).resolve().parents[1]
TOOLS_MANIFEST = ROOT / "docs/licenses/supply-chain-tools.json"
TOOLS_ROOT = ROOT / ".tools/supply-chain"
DOWNLOADS_ROOT = TOOLS_ROOT / "downloads"
EVIDENCE_ROOT = TOOLS_ROOT / "evidence"
DENY_CONFIG = ROOT / "deny.toml"
LOCKFILE = ROOT / "Cargo.lock"
CLI_MANIFEST = ROOT / "apps/cli/Cargo.toml"
ADVISORY_MAX_AGE_SECONDS = 7 * 24 * 60 * 60
EVIDENCE_MAX_AGE_SECONDS = 7 * 24 * 60 * 60
EVIDENCE_FUTURE_TOLERANCE_SECONDS = 5 * 60
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024
MAX_UNPACKED_BYTES = 1024 * 1024 * 1024
MAX_ARCHIVE_MEMBERS = 4096
MAX_MEMBER_BYTES = 512 * 1024 * 1024
DOWNLOAD_TIMEOUT_SECONDS = 30
COPY_CHUNK_BYTES = 1024 * 1024
SBOM_NAME = "kyberia_bin.cdx.json"
AUDIT_NAME = "cargo-deny-audit.json"
CHECKS = ("advisories", "licenses", "bans", "sources")
SCHEMA_PINS = {
    "bom-1.5.schema.json": "2d956c1d05c092695457a91f3b5c57c749793c013ec224a0935807cfc8ae4480",
    "spdx.schema.json": "ea6e844ee6fba1e93473d94834d0ee0996970533497935f932f73d488ffdf4a3",
    "jsf-0.82.schema.json": "8bae002c25e723db7ee1f26afde680ae1a2b1a8f6b4b4b0fd65dc3becb090aae",
}


class SupplyChainError(RuntimeError):
    """An actionable supply-chain setup, provenance, or tool failure."""


def _sha256(path: Path, *, maximum_bytes: int | None = None) -> str:
    if maximum_bytes is not None and path.stat().st_size > maximum_bytes:
        raise SupplyChainError(f"file exceeds bounded size of {maximum_bytes} bytes: {path}")
    digest = hashlib.sha256()
    consumed = 0
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(COPY_CHUNK_BYTES), b""):
            consumed += len(chunk)
            if maximum_bytes is not None and consumed > maximum_bytes:
                raise SupplyChainError("file grew beyond bounded size during hashing")
            digest.update(chunk)
    return digest.hexdigest()


def _canonical_json(data: Any) -> str:
    return json.dumps(data, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def _load_manifest() -> dict[str, Any]:
    try:
        data = json.loads(TOOLS_MANIFEST.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SupplyChainError(f"cannot read pinned tool manifest {TOOLS_MANIFEST}: {error}") from error
    if data.get("schema_version") != 1 or not isinstance(data.get("tools"), list):
        raise SupplyChainError(f"invalid pinned tool manifest schema: {TOOLS_MANIFEST}")
    for tool in data["tools"]:
        if not isinstance(tool.get("license_files"), list) or not tool["license_files"] or not all(isinstance(value, str) and value for value in tool["license_files"]):
            raise SupplyChainError("tool manifest requires a nonempty license_files array")
        for key in ("name", "version", "target", "archive_name", "archive_sha256", "download_url", "source_url", "license", "install_dir", "executable", "redistribution", "transformation", "provenance", "update_procedure"):
            if not isinstance(tool.get(key), str) or not tool[key]:
                raise SupplyChainError(f"tool manifest entry is missing {key}: {tool!r}")
        digest = tool["archive_sha256"]
        if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
            raise SupplyChainError(f"invalid SHA-256 pin for {tool['name']}")
        for key in ("install_dir", "executable"):
            relative = PurePosixPath(tool[key])
            if relative.is_absolute() or ".." in relative.parts:
                raise SupplyChainError(f"unsafe relative path in tool manifest: {tool[key]}")
        archive_name = PurePosixPath(tool["archive_name"])
        if archive_name.is_absolute() or len(archive_name.parts) != 1 or archive_name.parts[0] in (".", "..") or "\\" in tool["archive_name"]:
            raise SupplyChainError(f"unsafe archive name in tool manifest: {tool['archive_name']}")
        for license_file in tool["license_files"]:
            _safe_archive_path(license_file)
    return data


def _tool_entry(name: str, target: str) -> dict[str, Any]:
    entries = [item for item in _load_manifest()["tools"] if item.get("name") == name and item.get("target") == target]
    if not entries:
        raise SupplyChainError(f"no pinned {name} tool for target {target}")
    if len(entries) != 1:
        raise SupplyChainError(f"duplicate pinned {name} tool for target {target}")
    return entries[0]


def rust_target() -> str:
    """Return rustc's host target, rather than guessing from Python platform names."""
    try:
        result = subprocess.run(["rustc", "-vV"], cwd=ROOT, check=True, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise SupplyChainError("rustc -vV is required to select the pinned target") from error
    for line in result.stdout.splitlines():
        if line.startswith("host:"):
            target = line.partition(":")[2].strip()
            if target:
                return target
    raise SupplyChainError("rustc -vV did not report a host target")


def _tool_path(name: str, target: str) -> Path:
    entry = _tool_entry(name, target)
    return TOOLS_ROOT / entry["install_dir"] / entry["executable"]


def _check_archive_hash(archive: Path, expected: str) -> None:
    if not archive.is_file():
        raise SupplyChainError(f"pinned archive is missing: {archive}; run `python3 tools/dev.py bootstrap`")
    actual = _sha256(archive, maximum_bytes=MAX_ARCHIVE_BYTES)
    if actual != expected:
        raise SupplyChainError(f"SHA-256 mismatch for {archive.name}: expected {expected}, got {actual}")


def _download_archive(entry: dict[str, Any], opener: Callable[..., Any] = urllib.request.urlopen) -> Path:
    DOWNLOADS_ROOT.mkdir(parents=True, exist_ok=True)
    archive = DOWNLOADS_ROOT / entry["archive_name"]
    expected = entry["archive_sha256"]
    if archive.exists():
        _check_archive_hash(archive, expected)
        return archive

    partial = DOWNLOADS_ROOT / f".{entry['archive_name']}.partial-{os.getpid()}"
    try:
        with opener(entry["download_url"], timeout=DOWNLOAD_TIMEOUT_SECONDS) as response, partial.open("xb") as output:
            copied = 0
            while True:
                chunk = response.read(COPY_CHUNK_BYTES)
                if not chunk:
                    break
                copied += len(chunk)
                if copied > MAX_ARCHIVE_BYTES:
                    raise SupplyChainError(f"download exceeds bounded size of {MAX_ARCHIVE_BYTES} bytes")
                output.write(chunk)
    except (OSError, urllib.error.URLError) as error:
        raise SupplyChainError(f"failed to download {entry['download_url']}: {error}") from error
    actual = _sha256(partial, maximum_bytes=MAX_ARCHIVE_BYTES)
    if actual != expected:
        raise SupplyChainError(f"SHA-256 mismatch for downloaded {entry['archive_name']}: expected {expected}, got {actual}")
    os.replace(partial, archive)
    return archive


def _safe_archive_path(raw_name: str) -> PurePosixPath:
    if not raw_name or "\x00" in raw_name or "\\" in raw_name or raw_name.startswith("/"):
        raise SupplyChainError(f"unsafe archive path: {raw_name!r}")
    if "//" in raw_name or raw_name.startswith("./") or "/./" in raw_name:
        raise SupplyChainError(f"malformed archive path: {raw_name!r}")
    path = PurePosixPath(raw_name)
    if path.is_absolute() or not path.parts or ".." in path.parts or "." in path.parts:
        raise SupplyChainError(f"unsafe archive path: {raw_name!r}")
    return path


def _bounded_members(bundle: tarfile.TarFile) -> list[tuple[tarfile.TarInfo, PurePosixPath]]:
    members: list[tuple[tarfile.TarInfo, PurePosixPath]] = []
    unpacked_bytes = 0
    for index, member in enumerate(bundle):
        if index >= MAX_ARCHIVE_MEMBERS:
            raise SupplyChainError(f"archive exceeds bounded member count of {MAX_ARCHIVE_MEMBERS}")
        path = _safe_archive_path(member.name)
        if member.issym() or member.islnk():
            raise SupplyChainError(f"archive links are not allowed: {member.name!r}")
        if not (member.isdir() or member.isfile()):
            raise SupplyChainError(f"archive special files are not allowed: {member.name!r}")
        if member.size < 0 or member.size > MAX_MEMBER_BYTES:
            raise SupplyChainError(f"archive member exceeds bounded size: {member.name!r}")
        unpacked_bytes += member.size
        if unpacked_bytes > MAX_UNPACKED_BYTES:
            raise SupplyChainError(f"archive exceeds bounded unpacked size of {MAX_UNPACKED_BYTES} bytes")
        mode = stat.S_IMODE(member.mode)
        if mode & (stat.S_ISUID | stat.S_ISGID | stat.S_ISVTX | stat.S_IWOTH):
            raise SupplyChainError(f"archive member has unsafe permission bits: {member.name!r}")
        members.append((member, path))
    return members


def _safe_extract(archive: Path, destination: Path) -> None:
    """Extract a regular tar archive without links, traversal, or overwrite."""
    if destination.exists():
        raise SupplyChainError(f"refusing to extract over an existing directory: {destination}")
    destination.mkdir(parents=True, exist_ok=False)
    try:
        with tarfile.open(archive, mode="r:*") as bundle:
            members = _bounded_members(bundle)
            seen: set[Path] = set()
            resolved_destination = destination.resolve()
            for member, path in members:
                target = destination.joinpath(*path.parts)
                if target in seen:
                    raise SupplyChainError(f"duplicate archive path: {member.name!r}")
                seen.add(target)
                resolved_target = target.resolve(strict=False)
                if resolved_target != resolved_destination and resolved_destination not in resolved_target.parents:
                    raise SupplyChainError(f"archive path escapes destination: {member.name!r}")

            for member, path in sorted(members, key=lambda item: (not item[0].isdir(), item[1].as_posix())):
                target = destination.joinpath(*path.parts)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=False)
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                with bundle.extractfile(member) as source, target.open("xb") as output:
                    if source is None:
                        raise SupplyChainError(f"archive member has no content: {member.name!r}")
                    remaining = member.size
                    while remaining:
                        chunk = source.read(min(COPY_CHUNK_BYTES, remaining))
                        if not chunk:
                            raise SupplyChainError(f"archive member ended early: {member.name!r}")
                        output.write(chunk)
                        remaining -= len(chunk)
                target.chmod(0o755 if stat.S_IMODE(member.mode) & 0o111 else 0o644)
    except (OSError, tarfile.TarError) as error:
        raise SupplyChainError(f"failed to safely extract {archive}: {error}") from error


def _archive_member_sha256(archive: Path, relative_path: str) -> str:
    with tarfile.open(archive, mode="r:*") as bundle:
        members = _bounded_members(bundle)
        matches = [(member, path) for member, path in members if path.as_posix() == relative_path]
        if len(matches) != 1 or not matches[0][0].isfile():
            raise SupplyChainError(f"pinned archive has no unique executable member: {relative_path}")
        member = matches[0][0]
        source = bundle.extractfile(member)
        if source is None:
            raise SupplyChainError(f"pinned archive executable has no content: {relative_path}")
        digest = hashlib.sha256()
        remaining = member.size
        while remaining:
            chunk = source.read(min(COPY_CHUNK_BYTES, remaining))
            if not chunk:
                raise SupplyChainError(f"pinned archive executable ended early: {relative_path}")
            digest.update(chunk)
            remaining -= len(chunk)
        return digest.hexdigest()


def _verify_archive_licenses(archive: Path, entry: dict[str, Any]) -> None:
    with tarfile.open(archive, mode="r:*") as bundle:
        members = _bounded_members(bundle)
        by_path = {path.as_posix(): member for member, path in members}
        for license_file in entry["license_files"]:
            member = by_path.get(license_file)
            if member is None or not member.isfile():
                raise SupplyChainError(f"pinned archive is missing declared license file: {license_file}")
            source = bundle.extractfile(member)
            if source is None or not source.read(4096).strip():
                raise SupplyChainError(f"declared license file is empty: {license_file}")


def ensure_tool(name: str, target: str, *, install: bool = False) -> Path:
    entry = _tool_entry(name, target)
    executable = _tool_path(name, target)
    if executable.is_file() and os.access(executable, os.X_OK):
        archive = DOWNLOADS_ROOT / entry["archive_name"]
        if not archive.is_file():
            raise SupplyChainError(f"pinned archive is missing beside {executable}; refusing an unproven executable")
        _check_archive_hash(archive, entry["archive_sha256"])
        _verify_archive_licenses(archive, entry)
        archive_sha256 = _archive_member_sha256(archive, entry["executable"])
        if _sha256(executable, maximum_bytes=MAX_MEMBER_BYTES) != archive_sha256:
            raise SupplyChainError(f"installed {name} executable bytes do not match the verified archive")
        _assert_tool_version(name, executable, entry["version"])
        return executable
    if not install:
        raise SupplyChainError(f"pinned {name} executable is unavailable at {executable}; run `python3 tools/dev.py supply-chain-bootstrap`")
    archive = _download_archive(entry)
    _verify_archive_licenses(archive, entry)
    install_root = TOOLS_ROOT / entry["install_dir"]
    _safe_extract(archive, install_root)
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise SupplyChainError(f"archive did not provide executable at the pinned path: {executable}")
    if _sha256(executable, maximum_bytes=MAX_MEMBER_BYTES) != _archive_member_sha256(archive, entry["executable"]):
        raise SupplyChainError(f"installed {name} executable bytes do not match the verified archive")
    _assert_tool_version(name, executable, entry["version"])
    return executable


def _assert_tool_version(name: str, executable: Path, expected_version: str) -> None:
    args = [executable, "cyclonedx", "--version"] if name == "cargo-cyclonedx" else [executable, "--version"]
    result = _run(args, capture=True)
    output = (result.stdout + "\n" + result.stderr).strip()
    if not re.search(rf"(?<![0-9]){re.escape(expected_version)}(?![0-9])", output):
        raise SupplyChainError(f"{name} reported unexpected version: {output!r}")


def _run(args: Iterable[object], *, env: dict[str, str] | None = None, check: bool = True, capture: bool = False) -> subprocess.CompletedProcess[str]:
    command = [str(arg) for arg in args]
    print("+ " + " ".join(command), flush=True)
    try:
        return subprocess.run(command, cwd=ROOT, env=env, check=check, capture_output=capture, text=True)
    except OSError as error:
        raise SupplyChainError(f"could not execute {command[0]}: {error}") from error


def _git_provenance() -> tuple[str, int]:
    revision = _run(["git", "rev-parse", "HEAD"], capture=True).stdout.strip()
    epoch_text = _run(["git", "show", "-s", "--format=%ct", "HEAD"], capture=True).stdout.strip()
    if not revision or not epoch_text.isdigit():
        raise SupplyChainError("could not establish Git HEAD provenance")
    return revision, int(epoch_text)


def _workspace_manifest_paths() -> list[Path]:
    manifests = [ROOT / "Cargo.toml", CLI_MANIFEST]
    manifests.extend(sorted((ROOT / "crates").glob("*/Cargo.toml")))
    return sorted(set(manifests))


def _workspace_manifest_sha256() -> str:
    records = {
        str(path.relative_to(ROOT)): _sha256(path)
        for path in _workspace_manifest_paths()
        if path.is_file()
    }
    if len(records) != len(_workspace_manifest_paths()):
        raise SupplyChainError("workspace manifest set is incomplete")
    return hashlib.sha256(_canonical_json(records).encode("utf-8")).hexdigest()


def _source_snapshot() -> dict[str, str]:
    return {
        "cargo_lock_sha256": _sha256(LOCKFILE),
        "workspace_manifests_sha256": _workspace_manifest_sha256(),
    }


def _replace_workspace_paths(value: Any, workspace_root: Path) -> Any:
    if isinstance(value, list):
        return [_replace_workspace_paths(item, workspace_root) for item in value]
    if isinstance(value, dict):
        return {key: _replace_workspace_paths(item, workspace_root) for key, item in value.items()}
    if not isinstance(value, str):
        return value
    prefix = "path+file://" + workspace_root.as_posix()
    if value.startswith(prefix):
        remainder = value[len(prefix):]
        if remainder and not remainder.startswith("/") and not remainder.startswith("#"):
            return value
        return "path+file://workspace" + remainder
    return value


def _sbom_properties(document: dict[str, Any]) -> dict[str, str]:
    properties = document.get("properties", [])
    if not isinstance(properties, list):
        raise SupplyChainError("CycloneDX properties must be an array")
    result: dict[str, str] = {}
    for item in properties:
        if isinstance(item, dict) and isinstance(item.get("name"), str) and isinstance(item.get("value"), str):
            if item["name"].startswith("kyberia:") and item["name"] in result:
                raise SupplyChainError("duplicate Kyberia provenance property")
            result[item["name"]] = item["value"]
    return result


def validate_cyclonedx_schema(document: dict[str, Any]) -> None:
    """Apply pinned official Draft 7 assertions with all reference retrieval offline."""
    try:
        from jsonschema import Draft7Validator
        from jsonschema.exceptions import ValidationError, SchemaError
        from referencing import Registry, Resource
        from referencing.exceptions import NoSuchResource
    except ImportError as exc:
        raise SupplyChainError("install pinned tools/supply-chain/requirements.txt via supply-chain-bootstrap") from exc
    def refuse_network(uri):
        raise NoSuchResource(ref=uri)
    registry = Registry(retrieve=refuse_network)
    schemas = {}
    for name, expected in SCHEMA_PINS.items():
        path = ROOT / "tools/supply-chain/schemas" / name
        if _sha256(path, maximum_bytes=2 * 1024 * 1024) != expected:
            raise SupplyChainError("pinned CycloneDX schema hash mismatch: " + name)
        schema = json.loads(path.read_text(encoding="utf-8"))
        Draft7Validator.check_schema(schema)
        schemas[name] = schema
        registry = registry.with_resource(schema["$id"], Resource.from_contents(schema))
    try:
        Draft7Validator(schemas["bom-1.5.schema.json"], registry=registry).validate(document)
    except (ValidationError, SchemaError) as exc:
        raise SupplyChainError("CycloneDX official schema validation failed: " + str(exc.message)[:1024]) from exc


def validate_sbom(
    document: dict[str, Any],
    *,
    target: str,
    revision: str,
    lock_sha256: str,
    manifest_sha256: str,
    workspace_manifests_sha256: str | None = None,
) -> None:
    validate_cyclonedx_schema(document)
    if document.get("bomFormat") != "CycloneDX" or document.get("specVersion") != "1.5":
        raise SupplyChainError("CycloneDX output is missing the required 1.5 BOM identity")
    if document.get("version") != 1 or not isinstance(document.get("metadata"), dict):
        raise SupplyChainError("CycloneDX output has an invalid document version or metadata")
    tools = document["metadata"].get("tools")
    if not isinstance(tools, list) or not any(item.get("name") == "cargo-cyclonedx" and item.get("version") == "0.5.9" for item in tools if isinstance(item, dict)):
        raise SupplyChainError("CycloneDX output was not generated by pinned cargo-cyclonedx 0.5.9")
    component = document["metadata"].get("component")
    if not isinstance(component, dict) or component.get("name") != "kyberia":
        raise SupplyChainError("CycloneDX output does not describe the kyberia CLI")
    metadata_properties = document["metadata"].get("properties")
    if not isinstance(metadata_properties, list) or not any(
        isinstance(item, dict)
        and item.get("name") == "cdx:rustc:sbom:target:triple"
        and item.get("value") == target
        for item in metadata_properties
    ):
        raise SupplyChainError("CycloneDX output is missing its target metadata")
    components = document.get("components")
    dependencies = document.get("dependencies")
    if not isinstance(components, list) or not components or not isinstance(dependencies, list) or not dependencies:
        raise SupplyChainError("CycloneDX output is missing components or dependency relationships")
    component_refs = [item.get("bom-ref") for item in components if isinstance(item, dict)]
    root_ref = component.get("bom-ref")
    if not isinstance(root_ref, str) or not root_ref or root_ref in component_refs or not all(isinstance(ref, str) and ref for ref in component_refs) or len(set(component_refs)) != len(component_refs):
        raise SupplyChainError("CycloneDX output has missing or duplicate component references")
    references = set(component_refs) | {root_ref}
    graph_refs = set()
    for dependency in dependencies:
        if not isinstance(dependency, dict) or not isinstance(dependency.get("ref"), str) or dependency["ref"] not in references:
            raise SupplyChainError("CycloneDX output has an unknown dependency reference")
        if dependency["ref"] in graph_refs:
            raise SupplyChainError("CycloneDX output has duplicate dependency entries")
        graph_refs.add(dependency["ref"])
        # The official schema permits empty graph elements containing only ref.
        depends_on = dependency.get("dependsOn", [])
        if not isinstance(depends_on, list) or any(not isinstance(ref, str) or ref not in references for ref in depends_on):
            raise SupplyChainError("CycloneDX output has an unknown dependency edge")
    if graph_refs != references:
        raise SupplyChainError("CycloneDX output omits components from its dependency graph")
    properties = _sbom_properties(document)
    expected = {
        "kyberia:target": target,
        "kyberia:git-revision": revision,
        "kyberia:cargo-lock-sha256": lock_sha256,
        "kyberia:cli-manifest-sha256": manifest_sha256,
        "kyberia:cargo-describe": "binaries",
        "kyberia:cargo-all-dependencies": "true",
        "kyberia:cargo-offline": "true",
        "kyberia:cargo-lock-guard": "pre-post-sha256",
    }
    if workspace_manifests_sha256 is not None:
        expected["kyberia:workspace-manifests-sha256"] = workspace_manifests_sha256
    for key, value in expected.items():
        if properties.get(key) != value:
            raise SupplyChainError(f"CycloneDX provenance property {key} is missing or stale")
    serialized = json.dumps(document, ensure_ascii=False)
    if "path+file:///" in serialized:
        raise SupplyChainError("CycloneDX output retains an absolute workspace path")


def generate_sbom(target: str | None = None) -> Path:
    target = target or rust_target()
    cyclonedx = ensure_tool("cargo-cyclonedx", rust_target())
    revision, epoch = _git_provenance()
    before = _source_snapshot()
    manifest_sha256 = _sha256(CLI_MANIFEST)
    generated = ROOT / "apps/cli/kyberia_bin.cdx.json"
    started_ns = time.time_ns()
    environment = os.environ.copy()
    environment.update({"SOURCE_DATE_EPOCH": str(epoch), "CARGO_NET_OFFLINE": "true"})
    _run(
        [
            cyclonedx,
            "cyclonedx",
            "--manifest-path",
            CLI_MANIFEST,
            "--target",
            target,
            "--spec-version",
            "1.5",
            "--format",
            "json",
            "--describe",
            "binaries",
            "--all",
        ],
        env=environment,
    )
    if not generated.is_file() or generated.stat().st_mtime_ns < started_ns:
        raise SupplyChainError(f"cargo-cyclonedx did not produce fresh {generated}")
    after = _source_snapshot()
    after_revision, _ = _git_provenance()
    if before != after:
        raise SupplyChainError("Cargo.lock or workspace manifests changed during SBOM generation")
    if revision != after_revision:
        raise SupplyChainError("Git HEAD changed during SBOM generation")
    try:
        document = json.loads(generated.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SupplyChainError(f"generated SBOM is not valid JSON: {error}") from error
    document = _replace_workspace_paths(document, ROOT.resolve())
    EVIDENCE_ROOT.mkdir(parents=True, exist_ok=True)
    # Keep the exact tool output beside the normalized portable evidence. This
    # moves the plugin's source-tree output into the ignored evidence area so
    # a developer run cannot dirty apps/cli with a generated artifact.
    generated.replace(EVIDENCE_ROOT / "kyberia_bin.tool.cdx.json")
    properties = [item for item in document.get("properties", []) if not isinstance(item, dict) or not str(item.get("name", "")).startswith("kyberia:")]
    properties.extend(
        {"name": key, "value": value}
        for key, value in {
            "kyberia:target": target,
            "kyberia:git-revision": revision,
            "kyberia:git-head-epoch": str(epoch),
            "kyberia:cargo-lock-sha256": after["cargo_lock_sha256"],
            "kyberia:cli-manifest-sha256": manifest_sha256,
            "kyberia:workspace-manifests-sha256": after["workspace_manifests_sha256"],
            "kyberia:cargo-describe": "binaries",
            "kyberia:cargo-all-dependencies": "true",
            "kyberia:cargo-offline": "true",
            "kyberia:cargo-lock-guard": "pre-post-sha256",
        }.items()
    )
    document["properties"] = properties
    validate_sbom(
        document,
        target=target,
        revision=revision,
        lock_sha256=after["cargo_lock_sha256"],
        manifest_sha256=manifest_sha256,
        workspace_manifests_sha256=after["workspace_manifests_sha256"],
    )
    evidence = EVIDENCE_ROOT / SBOM_NAME
    evidence.write_text(_canonical_json(document), encoding="utf-8")
    return evidence


def _advisory_repository() -> Path:
    if tomllib is None:
        raise SupplyChainError("the pinned Python tooling parser is required to parse deny.toml safely; run `python3 tools/dev.py bootstrap`")
    try:
        config = tomllib.loads(DENY_CONFIG.read_text(encoding="utf-8"))
        configured = config["advisories"]["db-path"]
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        raise SupplyChainError(f"deny.toml must declare advisories.db-path: {error}") from error
    configured_path = Path(configured)
    if configured_path.parts[:2] == (".tools", "supply-chain"):
        path = (TOOLS_ROOT / Path(*configured_path.parts[2:])).resolve()
    else:
        path = (ROOT / configured_path).resolve()
    candidates = [path] if (path / ".git").exists() else sorted(item for item in path.iterdir() if item.is_dir() and (item / ".git").exists()) if path.is_dir() else []
    if len(candidates) != 1:
        raise SupplyChainError(f"expected one RustSec advisory Git checkout under {path}")
    return candidates[0]


def advisory_provenance(*, now: int | None = None) -> dict[str, Any]:
    repository = _advisory_repository()
    revision = _run(["git", "-C", repository, "rev-parse", "HEAD"], capture=True).stdout.strip()
    commit_epoch_text = _run(["git", "-C", repository, "show", "-s", "--format=%ct", "HEAD"], capture=True).stdout.strip()
    if not revision or not commit_epoch_text.isdigit():
        raise SupplyChainError("RustSec advisory checkout has no readable HEAD provenance")
    if _run(["git", "-C", repository, "status", "--porcelain", "--untracked-files=all"], capture=True).stdout:
        raise SupplyChainError("RustSec advisory checkout is dirty; committed provenance is required")
    commit_epoch = int(commit_epoch_text)
    now = int(time.time() if now is None else now)
    if commit_epoch > now + EVIDENCE_FUTURE_TOLERANCE_SECONDS:
        raise SupplyChainError("RustSec advisory commit is unexpectedly in the future")
    age = max(0, now - commit_epoch)
    if age > ADVISORY_MAX_AGE_SECONDS:
        raise SupplyChainError(f"RustSec advisory database is stale ({age // 86400} days old; maximum is 7 days)")
    try:
        repository_path = str(repository.relative_to(ROOT))
    except ValueError:
        repository_path = str(Path(".tools/supply-chain") / repository.relative_to(TOOLS_ROOT))
    return {
        "path": repository_path,
        "revision": revision,
        "commit_epoch": commit_epoch,
        "max_staleness_seconds": ADVISORY_MAX_AGE_SECONDS,
        "age_seconds": age,
    }


def refresh_advisories(target: str | None = None) -> dict[str, Any]:
    """Explicitly refresh RustSec without mutating or deleting old checkouts."""
    deny = ensure_tool("cargo-deny", target or rust_target())
    _run([deny, "--config", DENY_CONFIG, "fetch"])
    return advisory_provenance()


def audit(target: str | None = None) -> Path:
    if target is not None:
        raise SupplyChainError("audit covers all target platforms; --target is only supported for SBOM generation")
    deny = ensure_tool("cargo-deny", rust_target())
    advisory = advisory_provenance()
    revision, _ = _git_provenance()
    deny_sha256 = _sha256(DENY_CONFIG)
    source_snapshot = _source_snapshot()
    cli_manifest_sha256 = _sha256(CLI_MANIFEST)
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "scope": "Rust CLI dependency closure (apps/cli/Cargo.toml)",
        "result": "failed",
        "generated_epoch": int(time.time()),
        "git_revision": revision,
        "cargo_lock_sha256": source_snapshot["cargo_lock_sha256"],
        "workspace_manifests_sha256": source_snapshot["workspace_manifests_sha256"],
        "cli_manifest_sha256": cli_manifest_sha256,
        "deny_toml_sha256": deny_sha256,
        "cargo_deny_version": f"cargo-deny { _tool_entry('cargo-deny', rust_target())['version'] }",
        "advisory_db": advisory,
        "checks": [],
    }
    failures: list[str] = []
    for check in CHECKS:
        result = _run(
            [
                deny,
                "--manifest-path",
                CLI_MANIFEST,
                "--config",
                DENY_CONFIG,
                "--locked",
                "--offline",
                "--format",
                "json",
                "check",
                check,
            ],
            check=False,
            capture=True,
        )
        record = {
            "name": check,
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }
        evidence["checks"].append(record)
        if result.returncode != 0:
            failures.append(check)
    after_advisory = advisory_provenance()
    if (_git_provenance()[0] != revision or _source_snapshot() != source_snapshot
            or _sha256(DENY_CONFIG) != deny_sha256
            or _sha256(CLI_MANIFEST) != cli_manifest_sha256
            or any(after_advisory[key] != advisory[key] for key in ("revision", "commit_epoch", "path"))):
        failures.append("input_changed_during_audit")
    EVIDENCE_ROOT.mkdir(parents=True, exist_ok=True)
    evidence_path = EVIDENCE_ROOT / AUDIT_NAME
    evidence["result"] = "pass" if not failures else "failed"
    evidence_path.write_text(_canonical_json(evidence), encoding="utf-8")
    if failures:
        raise SupplyChainError("cargo-deny checks failed: " + ", ".join(failures))
    return evidence_path


def validate_audit_evidence(path: Path | None = None, *, now: int | None = None) -> dict[str, Any]:
    path = path or (EVIDENCE_ROOT / AUDIT_NAME)
    if not path.is_file():
        raise SupplyChainError(f"missing cargo-deny evidence: {path}")
    try:
        evidence = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SupplyChainError(f"invalid cargo-deny evidence: {error}") from error
    if evidence.get("schema_version") != 1 or evidence.get("scope") != "Rust CLI dependency closure (apps/cli/Cargo.toml)":
        raise SupplyChainError("cargo-deny evidence has an invalid schema or scope")
    generated = evidence.get("generated_epoch")
    current = int(time.time() if now is None else now)
    if type(generated) is not int or generated > current + EVIDENCE_FUTURE_TOLERANCE_SECONDS or max(0, current - generated) > EVIDENCE_MAX_AGE_SECONDS:
        raise SupplyChainError("cargo-deny evidence is missing a fresh generated_epoch")
    if evidence.get("result") != "pass":
        raise SupplyChainError("cargo-deny evidence does not record a passing audit")
    revision, _ = _git_provenance()
    if not isinstance(evidence.get("git_revision"), str) or not re.fullmatch(r"[0-9a-f]{40}", evidence["git_revision"]) or evidence["git_revision"] != revision:
        raise SupplyChainError("cargo-deny evidence does not describe the current Git revision")
    snapshot = _source_snapshot()
    if evidence.get("cargo_lock_sha256") != snapshot["cargo_lock_sha256"] or evidence.get("workspace_manifests_sha256") != snapshot["workspace_manifests_sha256"] or evidence.get("cli_manifest_sha256") != _sha256(CLI_MANIFEST) or evidence.get("deny_toml_sha256") != _sha256(DENY_CONFIG):
        raise SupplyChainError("cargo-deny evidence does not describe the current lock/config")
    expected_version = f"cargo-deny { _tool_entry('cargo-deny', rust_target())['version'] }"
    if evidence.get("cargo_deny_version") != expected_version:
        raise SupplyChainError("cargo-deny evidence does not record the pinned tool version")
    advisory = advisory_provenance(now=current)
    recorded = evidence.get("advisory_db")
    if not isinstance(recorded, dict) or not re.fullmatch(r"[0-9a-f]{40}", str(recorded.get("revision"))) or any(recorded.get(key) != advisory[key] for key in ("revision", "commit_epoch", "path")) or type(recorded.get("commit_epoch")) is not int:
        raise SupplyChainError("cargo-deny evidence does not describe the current RustSec revision")
    checks = evidence.get("checks")
    if not isinstance(checks, list) or {item.get("name") for item in checks if isinstance(item, dict)} != set(CHECKS) or len(checks) != len(CHECKS):
        raise SupplyChainError("cargo-deny evidence does not contain exactly one record per required check")
    for item in checks:
        if not isinstance(item, dict) or not isinstance(item.get("name"), str) or type(item.get("returncode")) is not int or item.get("returncode") != 0 or not isinstance(item.get("stdout"), str) or not isinstance(item.get("stderr"), str):
            raise SupplyChainError("cargo-deny evidence contains a failed or malformed check record")
    return evidence


def bootstrap(target: str | None = None) -> None:
    target = target or rust_target()
    ensure_tool("cargo-cyclonedx", target, install=True)
    deny = ensure_tool("cargo-deny", target, install=True)
    # The audit command deliberately requires an existing, inspectable checkout.
    # Fetching it is explicit so an offline build cannot silently refresh policy.
    try:
        _advisory_repository()
    except SupplyChainError as error:
        if "expected one RustSec advisory Git checkout" not in str(error):
            raise
        _run([deny, "--config", DENY_CONFIG, "fetch"])
        _advisory_repository()
    try:
        advisory_provenance()
    except SupplyChainError as error:
        if "stale" in str(error):
            raise SupplyChainError(f"RustSec advisory database is stale; run `python3 tools/dev.py supply-chain-refresh`: {error}") from error
        raise


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("bootstrap", "sbom", "audit", "refresh-advisories"))
    parser.add_argument("--target", default=None, help="Rust target triple (defaults to rustc host)")
    args = parser.parse_args(argv)
    try:
        if args.command == "bootstrap":
            bootstrap(args.target)
        elif args.command == "sbom":
            print(generate_sbom(args.target))
        elif args.command == "refresh-advisories":
            print(json.dumps(refresh_advisories(args.target), sort_keys=True))
        else:
            print(audit(args.target))
    except (SupplyChainError, subprocess.CalledProcessError) as error:
        print(f"supply-chain: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
