"""Adversarial tests for pinned supply-chain tooling and evidence boundaries."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("supply_chain", ROOT / "tools/supply_chain.py")
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class SupplyChainTests(unittest.TestCase):
    def retained_dir(self) -> Path:
        return Path(tempfile.mkdtemp(prefix="kyberia-supply-chain-test-"))

    def test_manifest_contains_only_valid_pinned_paths_and_hashes(self):
        manifest = MODULE._load_manifest()
        self.assertEqual(manifest["target"], "aarch64-apple-darwin")
        self.assertEqual({tool["name"] for tool in manifest["tools"]}, {"cargo-cyclonedx", "cargo-deny"})
        for tool in manifest["tools"]:
            self.assertRegex(tool["archive_sha256"], r"^[0-9a-f]{64}$")
            self.assertNotIn("..", Path(tool["install_dir"]).parts)
            self.assertNotIn("..", Path(tool["executable"]).parts)

    def test_wrong_archive_hash_is_rejected(self):
        root = self.retained_dir()
        archive = root / "tool.tar.gz"
        archive.write_bytes(b"tampered")
        with self.assertRaisesRegex(MODULE.SupplyChainError, "SHA-256 mismatch"):
            MODULE._check_archive_hash(archive, "0" * 64)

    def test_existing_executable_without_pinned_archive_is_rejected(self):
        root = self.retained_dir()
        old_tools_root, old_downloads_root = MODULE.TOOLS_ROOT, MODULE.DOWNLOADS_ROOT
        try:
            MODULE.TOOLS_ROOT = root
            MODULE.DOWNLOADS_ROOT = root / "downloads"
            executable = MODULE._tool_path("cargo-deny", "aarch64-apple-darwin")
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"unproven executable")
            executable.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
            with self.assertRaisesRegex(MODULE.SupplyChainError, "unproven executable"):
                MODULE.ensure_tool("cargo-deny", "aarch64-apple-darwin")
        finally:
            MODULE.TOOLS_ROOT, MODULE.DOWNLOADS_ROOT = old_tools_root, old_downloads_root

    def test_modified_installed_executable_is_rejected_against_archive_member(self):
        root = self.retained_dir()
        old_tools_root, old_downloads_root, old_entry = MODULE.TOOLS_ROOT, MODULE.DOWNLOADS_ROOT, MODULE._tool_entry
        member_path = "fake-target/fake-tool"
        entry = {
            "name": "fake-tool",
            "version": "1.0.0",
            "target": "fake-target",
            "archive_name": "fake-tool.tar",
            "archive_sha256": "",
            "install_dir": "fake-install",
            "executable": member_path,
            "license_files": [],
        }
        try:
            MODULE.TOOLS_ROOT = root / "tools"
            MODULE.DOWNLOADS_ROOT = MODULE.TOOLS_ROOT / "downloads"
            MODULE.DOWNLOADS_ROOT.mkdir(parents=True)
            archive = MODULE.DOWNLOADS_ROOT / entry["archive_name"]
            with tarfile.open(archive, "w") as bundle:
                info = tarfile.TarInfo(member_path)
                payload = b"ORIGINAL"
                info.size = len(payload)
                info.mode = 0o755
                bundle.addfile(info, io.BytesIO(payload))
            entry["archive_sha256"] = MODULE._sha256(archive)
            executable = MODULE.TOOLS_ROOT / entry["install_dir"] / member_path
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"TAMPERED")
            executable.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
            MODULE._tool_entry = lambda name, target: entry
            with self.assertRaisesRegex(MODULE.SupplyChainError, "executable bytes do not match"):
                MODULE.ensure_tool("fake-tool", "fake-target")
        finally:
            MODULE.TOOLS_ROOT, MODULE.DOWNLOADS_ROOT, MODULE._tool_entry = old_tools_root, old_downloads_root, old_entry

    def test_archive_traversal_and_absolute_paths_are_rejected(self):
        for member_name in ("../outside", "/absolute", "folder/../../outside"):
            with self.subTest(member_name=member_name):
                root = self.retained_dir()
                archive = root / "unsafe.tar"
                with tarfile.open(archive, "w") as bundle:
                    info = tarfile.TarInfo(member_name)
                    payload = b"unsafe"
                    info.size = len(payload)
                    bundle.addfile(info, io.BytesIO(payload))
                with self.assertRaisesRegex(MODULE.SupplyChainError, "unsafe archive path"):
                    MODULE._safe_extract(archive, root / "extract")

    def test_archive_links_are_rejected(self):
        root = self.retained_dir()
        archive = root / "link.tar"
        with tarfile.open(archive, "w") as bundle:
            info = tarfile.TarInfo("tool")
            info.type = tarfile.SYMTYPE
            info.linkname = "/etc/passwd"
            bundle.addfile(info)
        with self.assertRaisesRegex(MODULE.SupplyChainError, "links are not allowed"):
            MODULE._safe_extract(archive, root / "extract")

    def test_archive_bounds_and_permissions_are_rejected(self):
        root = self.retained_dir()
        old_member_limit, old_unpacked_limit = MODULE.MAX_MEMBER_BYTES, MODULE.MAX_UNPACKED_BYTES
        try:
            MODULE.MAX_MEMBER_BYTES = 3
            MODULE.MAX_UNPACKED_BYTES = 3
            archive = root / "large.tar"
            with tarfile.open(archive, "w") as bundle:
                info = tarfile.TarInfo("large")
                info.size = 4
                bundle.addfile(info, io.BytesIO(b"1234"))
            with self.assertRaisesRegex(MODULE.SupplyChainError, "bounded size"):
                MODULE._safe_extract(archive, root / "large-extract")
        finally:
            MODULE.MAX_MEMBER_BYTES, MODULE.MAX_UNPACKED_BYTES = old_member_limit, old_unpacked_limit

        archive = root / "unsafe-mode.tar"
        with tarfile.open(archive, "w") as bundle:
            info = tarfile.TarInfo("unsafe-mode")
            info.mode = 0o666
            info.size = 1
            bundle.addfile(info, io.BytesIO(b"x"))
        with self.assertRaisesRegex(MODULE.SupplyChainError, "permission bits"):
            MODULE._safe_extract(archive, root / "unsafe-mode-extract")

    def test_download_has_timeout_and_byte_bound(self):
        root = self.retained_dir()
        old_downloads_root, old_limit = MODULE.DOWNLOADS_ROOT, MODULE.MAX_ARCHIVE_BYTES
        calls = []

        class Response:
            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

            def read(self, size):
                return b"12345"

        def opener(url, timeout):
            calls.append((url, timeout))
            return Response()

        try:
            MODULE.DOWNLOADS_ROOT = root / "downloads"
            MODULE.MAX_ARCHIVE_BYTES = 4
            entry = {
                "archive_name": "bounded.tar",
                "archive_sha256": "0" * 64,
                "download_url": "https://example.invalid/bounded.tar",
            }
            with self.assertRaisesRegex(MODULE.SupplyChainError, "bounded size"):
                MODULE._download_archive(entry, opener=opener)
            self.assertEqual(calls, [(entry["download_url"], MODULE.DOWNLOAD_TIMEOUT_SECONDS)])
        finally:
            MODULE.DOWNLOADS_ROOT, MODULE.MAX_ARCHIVE_BYTES = old_downloads_root, old_limit

    def test_subprocess_failure_is_not_hidden(self):
        with self.assertRaises(subprocess.CalledProcessError):
            MODULE._run([sys.executable, "-c", "raise SystemExit(1)"])

    def test_missing_and_stale_audit_evidence_are_rejected(self):
        root = self.retained_dir()
        with self.assertRaisesRegex(MODULE.SupplyChainError, "missing cargo-deny evidence"):
            MODULE.validate_audit_evidence(root / "missing.json")
        stale = root / "stale.json"
        stale.write_text(
            json.dumps({
                "schema_version": 1,
                "scope": "Rust CLI dependency closure (apps/cli/Cargo.toml)",
                "generated_epoch": 1,
                "result": "pass",
            }),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(MODULE.SupplyChainError, "fresh generated_epoch"):
            MODULE.validate_audit_evidence(stale, now=MODULE.EVIDENCE_MAX_AGE_SECONDS + 2)

    def test_empty_future_audit_evidence_is_rejected(self):
        root = self.retained_dir()
        future = root / "future.json"
        future.write_text(
            json.dumps({
                "schema_version": 1,
                "scope": "Rust CLI dependency closure (apps/cli/Cargo.toml)",
                "generated_epoch": 10_000_000_000,
                "result": "pass",
            }),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(MODULE.SupplyChainError, "fresh generated_epoch"):
            MODULE.validate_audit_evidence(future, now=1_000)

    def test_workspace_bom_refs_are_portable(self):
        workspace = Path("/tmp/kyberia-test-worktree").resolve()
        source = {
            "metadata": {
                "component": {
                    "bom-ref": f"path+file://{workspace}/apps/cli#kyberia-cli@0.1.0",
                    "purl": "pkg:cargo/kyberia-cli@0.1.0?download_url=file://.#src/main.rs",
                }
            }
        }
        normalized = MODULE._replace_workspace_paths(source, workspace)
        self.assertEqual(
            normalized["metadata"]["component"]["bom-ref"],
            "path+file://workspace/apps/cli#kyberia-cli@0.1.0",
        )
        self.assertEqual(
            normalized["metadata"]["component"]["purl"],
            source["metadata"]["component"]["purl"],
        )

    def test_minimal_bom_without_provenance_is_rejected(self):
        document = {
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "version": 1,
            "metadata": {
                "tools": [{"name": "cargo-cyclonedx", "version": "0.5.9"}],
                "component": {"name": "kyberia", "type": "application", "bom-ref": "root"},
                "properties": [{"name": "cdx:rustc:sbom:target:triple", "value": "aarch64-apple-darwin"}],
            },
            "components": [{"name": "fixture", "type": "library", "bom-ref": "fixture"}],
            "dependencies": [{"ref": "root", "dependsOn": ["fixture"]}, {"ref": "fixture"}],
            "properties": [],
        }
        # Schema assertions have separate real-validator tests; this probes the
        # Kyberia provenance boundary even before optional tools are installed.
        with mock.patch.object(MODULE, "validate_cyclonedx_schema"), self.assertRaisesRegex(MODULE.SupplyChainError, "provenance property"):
            MODULE.validate_sbom(document, target="aarch64-apple-darwin", revision="r", lock_sha256="l", manifest_sha256="m")

    def test_duplicate_kyberia_provenance_is_rejected(self):
        document = {"properties": [{"name": "kyberia:target", "value": value} for value in ("old", "new")]}
        with self.assertRaisesRegex(MODULE.SupplyChainError, "duplicate"):
            MODULE._sbom_properties(document)

    def test_audit_never_silently_ignores_requested_target(self):
        with self.assertRaisesRegex(MODULE.SupplyChainError, "all target platforms"):
            MODULE.audit("x86_64-pc-windows-msvc")

    @unittest.skipUnless(
        os.environ.get("KYBERIA_TEST_SUPPLY_CHAIN_RUNTIME") == "1",
        "set KYBERIA_TEST_SUPPLY_CHAIN_RUNTIME=1 for explicit real-tool execution",
    )
    def test_real_cyclonedx_output_is_fresh_and_portable(self):
        evidence = MODULE.generate_sbom("aarch64-apple-darwin")
        document = json.loads(evidence.read_text(encoding="utf-8"))
        revision, _ = MODULE._git_provenance()
        MODULE.validate_sbom(
            document,
            target="aarch64-apple-darwin",
            revision=revision,
            lock_sha256=MODULE._sha256(MODULE.LOCKFILE),
            manifest_sha256=MODULE._sha256(MODULE.CLI_MANIFEST),
        )
        self.assertNotIn("path+file:///", evidence.read_text(encoding="utf-8"))

    @unittest.skipUnless(importlib.util.find_spec("jsonschema"), "optional pinned schema validator not installed")
    def test_official_schema_accepts_valid_minimum_and_rejects_invalid_component(self):
        document = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
                    "components": [{"type": "library", "name": "synthetic"}]}
        MODULE.validate_cyclonedx_schema(document)
        del document["components"][0]["type"]
        with self.assertRaisesRegex(MODULE.SupplyChainError, "official schema"):
            MODULE.validate_cyclonedx_schema(document)

    @unittest.skipUnless(importlib.util.find_spec("jsonschema"), "optional pinned schema validator not installed")
    def test_official_schema_resolves_spdx_locally_and_rejects_tampered_schema(self):
        document = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
                    "components": [{"type": "library", "name": "synthetic", "licenses": [{"license": {"id": "MIT"}}]}]}
        with mock.patch("urllib.request.urlopen", side_effect=AssertionError("network forbidden")):
            MODULE.validate_cyclonedx_schema(document)
            document["components"][0]["licenses"][0]["license"]["id"] = "NOT-A-LICENSE"
            with self.assertRaisesRegex(MODULE.SupplyChainError, "official schema"):
                MODULE.validate_cyclonedx_schema(document)
        with mock.patch.dict(MODULE.SCHEMA_PINS, {"bom-1.5.schema.json": "0" * 64}):
            with self.assertRaisesRegex(MODULE.SupplyChainError, "hash mismatch"):
                MODULE.validate_cyclonedx_schema(document)

    def test_audit_rejects_changes_during_tool_execution(self):
        snapshot = {"cargo_lock_sha256": "a", "workspace_manifests_sha256": "b"}
        advisory = {"revision": "c", "commit_epoch": 1, "path": "advisories"}
        with mock.patch.object(MODULE, "ensure_tool", return_value=Path("fake")), \
                mock.patch.object(MODULE, "rust_target", return_value="aarch64-apple-darwin"), \
                mock.patch.object(MODULE, "advisory_provenance", return_value=advisory), \
                mock.patch.object(MODULE, "_git_provenance", return_value=("revision", 1)), \
                mock.patch.object(MODULE, "_source_snapshot", side_effect=[snapshot, dict(snapshot, cargo_lock_sha256="changed")]), \
                mock.patch.object(MODULE, "_run", return_value=subprocess.CompletedProcess([], 0, "", "")), \
                mock.patch.object(MODULE, "EVIDENCE_ROOT", self.retained_dir()):
            with self.assertRaisesRegex(MODULE.SupplyChainError, "input_changed_during_audit"):
                MODULE.audit()
            evidence = json.loads((MODULE.EVIDENCE_ROOT / MODULE.AUDIT_NAME).read_text())
            self.assertEqual(evidence["result"], "failed")

    def test_dirty_advisory_checkout_is_rejected(self):
        results = [subprocess.CompletedProcess([], 0, "a" * 40, ""),
                   subprocess.CompletedProcess([], 0, "1", ""),
                   subprocess.CompletedProcess([], 0, " M crates/example.md", "")]
        with mock.patch.object(MODULE, "_advisory_repository", return_value=ROOT / ".tools/advisories"), \
                mock.patch.object(MODULE, "_run", side_effect=results):
            with self.assertRaisesRegex(MODULE.SupplyChainError, "dirty"):
                MODULE.advisory_provenance(now=10)


if __name__ == "__main__":
    unittest.main()
