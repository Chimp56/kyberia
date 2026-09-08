import importlib.util
import itertools
import json
import os
import sys
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
TOOLS_PATH = REPOSITORY_ROOT / "tools" / "trash_clean.py"
_SPEC = importlib.util.spec_from_file_location("kyberia_trash_clean", TOOLS_PATH)
assert _SPEC is not None and _SPEC.loader is not None
trash_clean = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = trash_clean
_SPEC.loader.exec_module(trash_clean)


class TrashCleanTests(unittest.TestCase):
    _fixture_counter = itertools.count()

    def setUp(self):
        # These fixtures are intentionally retained for manual inspection.  A
        # test teardown must never recursively remove project evidence.
        fixture_name = f"trash-clean-{os.getpid()}-{next(self._fixture_counter)}"
        self.fixture = REPOSITORY_ROOT / ".trash" / "test-runs" / fixture_name
        self.fixture.mkdir(parents=True, exist_ok=False)
        self.project = self.fixture / "project"
        self.project.mkdir()

    def _write(self, relative, content="fixture"):
        path = self.project / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def test_empty_root_succeeds_without_creating_trash_run(self):
        report = trash_clean.clean(self.project)

        self.assertTrue(report.succeeded)
        self.assertEqual([], report.moved)
        self.assertEqual([], report.failures)
        self.assertFalse((self.project / ".trash").exists())

    def test_allowlist_is_explicit_and_root_level(self):
        self.assertEqual(
            (
                Path("target"),
                Path("dist"),
                Path("coverage"),
                Path("test-results"),
                Path("playwright-report"),
            ),
            trash_clean.KNOWN_OUTPUTS,
        )

    def test_moves_only_documented_outputs_and_writes_original_path_manifest(self):
        for output in ("target", "dist", "coverage", "test-results", "playwright-report"):
            self._write(Path(output) / "marker.txt", output)
        protected = (
            ".tools/venv-marker",
            ".venv/marker",
            "node_modules/marker",
            ".pnpm-store/marker",
            "research/fixture.txt",
            ".worktrees/user.txt",
            ".git/user-data.txt",
            "unknown-output/marker",
        )
        for path in protected:
            self._write(path, "preserve")

        report = trash_clean.clean(self.project)

        self.assertTrue(report.succeeded)
        self.assertEqual(5, len(report.moved))
        self.assertIsNotNone(report.manifest_path)
        manifest = json.loads(Path(report.manifest_path).read_text(encoding="utf-8"))
        self.assertEqual(1, manifest["schema_version"])
        self.assertEqual("success", manifest["status"])
        self.assertEqual(
            ["target", "dist", "coverage", "test-results", "playwright-report"],
            [entry["original_relative_path"] for entry in manifest["moved"]],
        )
        for output in ("target", "dist", "coverage", "test-results", "playwright-report"):
            self.assertFalse((self.project / output).exists())
        for path in protected:
            self.assertTrue((self.project / path).exists())

    def test_run_directory_collision_is_retried_without_overwrite(self):
        clean_root = self.project / ".trash" / "clean-runs"
        collision = clean_root / "collision"
        collision.mkdir(parents=True)
        sentinel = collision / "sentinel.txt"
        sentinel.write_text("preserve", encoding="utf-8")
        self._write("target/marker.txt")
        ids = iter(("collision", "fresh"))

        report = trash_clean.clean(self.project, run_id_factory=lambda: next(ids))

        self.assertTrue(report.succeeded)
        self.assertEqual(str(clean_root / "fresh"), report.run_path)
        self.assertEqual("preserve", sentinel.read_text(encoding="utf-8"))
        self.assertTrue((clean_root / "fresh" / "target" / "marker.txt").exists())

    def test_symlink_output_is_rejected_without_inspecting_or_moving_target(self):
        outside = self.fixture / "outside"
        outside.mkdir()
        marker = outside / "marker.txt"
        marker.write_text("outside", encoding="utf-8")
        try:
            os.symlink(outside, self.project / "target")
        except OSError as error:
            self.skipTest(f"platform does not permit fixture symlinks: {error}")

        report = trash_clean.clean(self.project)

        self.assertFalse(report.succeeded)
        self.assertEqual([], report.moved)
        self.assertEqual("target", report.failures[0]["relative_path"])
        self.assertIn("symlink", report.failures[0]["reason"])
        self.assertTrue((self.project / "target").is_symlink())
        self.assertEqual("outside", marker.read_text(encoding="utf-8"))

    def test_symlink_trash_parent_is_rejected_without_moving_output(self):
        outside = self.fixture / "outside-trash"
        outside.mkdir()
        try:
            os.symlink(outside, self.project / ".trash")
        except OSError as error:
            self.skipTest(f"platform does not permit fixture symlinks: {error}")
        self._write("target/marker.txt")

        report = trash_clean.clean(self.project)

        self.assertFalse(report.succeeded)
        self.assertEqual([], report.moved)
        self.assertTrue((self.project / "target").exists())
        self.assertEqual([], list(outside.iterdir()))

    def test_partial_move_is_explicit_and_manifest_remains_recoverable(self):
        self._write("target/marker.txt", "target")
        self._write("dist/marker.txt", "dist")

        def fail_dist(source, destination):
            if source.name == "dist":
                raise PermissionError("fixture refusal")
            source.rename(destination)

        report = trash_clean.clean(self.project, move=fail_dist)

        self.assertFalse(report.succeeded)
        self.assertTrue(report.partial)
        self.assertEqual(["target"], [entry["original_relative_path"] for entry in report.moved])
        self.assertTrue((self.project / "dist").exists())
        self.assertIsNotNone(report.manifest_path)
        manifest = json.loads(Path(report.manifest_path).read_text(encoding="utf-8"))
        self.assertEqual("partial", manifest["status"])
        self.assertEqual("dist", manifest["failures"][0]["relative_path"])

    def test_second_empty_run_does_not_overwrite_first_run(self):
        self._write("target/marker.txt")
        first = trash_clean.clean(self.project)
        second = trash_clean.clean(self.project)

        self.assertTrue(first.succeeded)
        self.assertTrue(second.succeeded)
        self.assertEqual([], second.moved)
        self.assertEqual([], second.failures)
        self.assertTrue(Path(first.manifest_path).exists())


if __name__ == "__main__":
    unittest.main()
