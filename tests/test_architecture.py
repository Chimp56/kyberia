import copy
import importlib.util
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("architecture", Path(__file__).resolve().parents[1] / "tools/architecture.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ArchitectureTests(unittest.TestCase):
    def setUp(self):
        self.policy = {"schema_version": 1, "packages": {
            "core": {"layer": "domain", "external_dependencies": ["serde"]},
            "store": {"layer": "adapter", "external_dependencies": []}},
            "allowed_internal_layers": {"domain": ["domain"], "adapter": ["domain"]}}
        self.metadata = {"workspace_members": ["core", "store"], "packages": [
            {"name": "core", "id": "core", "manifest_path": "/fixture/core/Cargo.toml", "dependencies": [
                {"name": "serde", "kind": None}]},
            {"name": "store", "id": "store", "manifest_path": "/fixture/store/Cargo.toml", "dependencies": [
                {"name": "core", "path": "/fixture/core", "kind": None}]}]}

    def test_adapters_depend_inward(self):
        self.assertEqual(MODULE.check(self.metadata, self.policy), [])

    def test_outward_dependency_fails_even_optional_or_target_specific(self):
        for kind in [None, "build"]:
            metadata = copy.deepcopy(self.metadata)
            metadata["packages"][0]["dependencies"].append({"name": "store", "kind": kind,
                "path": "/fixture/store", "optional": True, "target": 'cfg(windows)'})
            self.assertTrue(any("outward" in error for error in MODULE.check(metadata, self.policy)))

    def test_renamed_external_dependency_does_not_hide_original_identity(self):
        self.metadata["packages"][0]["dependencies"].append({"name": "foreign-engine", "rename": "math", "kind": None})
        self.assertTrue(any("foreign-engine" in error for error in MODULE.check(self.metadata, self.policy)))

    def test_unregistered_crate_and_shadowed_internal_crate_fail(self):
        self.metadata["packages"][0]["dependencies"].append({"name": "store", "kind": None, "path": "/outside/store"})
        self.assertTrue(any("mismatch" in error for error in MODULE.check(self.metadata, self.policy)))
        self.metadata["packages"].append({"id": "extra", "name": "extra", "dependencies": []})
        self.metadata["workspace_members"].append("extra")
        self.assertTrue(any("every workspace" in error for error in MODULE.check(self.metadata, self.policy)))

    def test_test_only_dependency_does_not_change_production_direction(self):
        self.metadata["packages"][0]["dependencies"].append({"name": "proptest", "kind": "dev"})
        self.assertEqual(MODULE.check(self.metadata, self.policy), [])


if __name__ == "__main__":
    unittest.main()
