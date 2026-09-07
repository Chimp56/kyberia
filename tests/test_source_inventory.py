import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("source_inventory", Path(__file__).resolve().parents[1] / "tools/source_inventory.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SourceInventoryTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="kyberia-source-inventory-"))
        source = self.root / "registry/src/example/fixture-1.0.0"
        source.mkdir(parents=True)
        self.archive = self.root / "registry/cache/example/fixture-1.0.0.crate"
        self.archive.parent.mkdir(parents=True)
        self.archive.write_bytes(b"original independently constructed package fixture")
        self.digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.package = dict(id="fixture", name="fixture", version="1.0.0", source="registry+https://example.invalid/index",
                            manifest_path=str(source / "Cargo.toml"), license="MIT", repository=None)
        self.metadata = dict(packages=[self.package], workspace_members=[])
        self.lock = ('version = 4\n[[package]]\nname="fixture"\nversion="1.0.0"\n'
                     'source="registry+https://example.invalid/index"\nchecksum="' + self.digest + '"\n').encode()

    def test_matching_archive_is_bound_to_exact_lock_identity(self):
        result = MODULE.inventory(self.metadata, self.lock)
        self.assertEqual(result["packages"][0]["archive_sha256"], self.digest)
        self.assertEqual(result["cargo_lock_sha256"], hashlib.sha256(self.lock).hexdigest())

    def test_tampered_archive_is_rejected_before_inventory_generation(self):
        self.archive.write_bytes(b"altered")
        with self.assertRaisesRegex(ValueError, "checksum"):
            MODULE.inventory(self.metadata, self.lock)

    def test_missing_or_different_locked_identity_is_rejected(self):
        for lock in [b"package=[]", self.lock.replace(b'1.0.0', b'2.0.0'), self.lock.replace(b'example.invalid', b'other.invalid')]:
            with self.subTest(lock=lock), self.assertRaises(ValueError):
                MODULE.inventory(self.metadata, lock)

    def test_metadata_must_cover_all_locked_packages(self):
        with self.assertRaisesRegex(ValueError, "omits"):
            MODULE.inventory(dict(packages=[], workspace_members=[]), self.lock)

    def test_nonworkspace_path_dependency_cannot_disappear(self):
        self.package["source"] = None
        lock = b'[[package]]\nname="fixture"\nversion="1.0.0"\n'
        with self.assertRaisesRegex(ValueError, "nonworkspace"):
            MODULE.inventory(self.metadata, lock)
        self.metadata["workspace_members"] = ["fixture"]
        self.assertEqual(MODULE.inventory(self.metadata, lock)["packages"], [])

    def test_missing_license_is_rejected(self):
        self.package["license"] = None
        with self.assertRaisesRegex(ValueError, "missing license"):
            MODULE.inventory(self.metadata, self.lock)

    def test_duplicate_package_cannot_mask_omission(self):
        self.metadata["packages"].append(dict(self.package))
        with self.assertRaises(ValueError):
            MODULE.inventory(self.metadata, self.lock)


if __name__ == "__main__":
    unittest.main()
