"""Failure diagnostics must retain exit status and never replay child output."""
import contextlib
import importlib.util
import io
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("kyberia_dev", Path(__file__).resolve().parents[1] / "tools/dev.py")
DEV = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DEV)


class DeveloperFailureTests(unittest.TestCase):
    def run_failure(self, github_actions):
        output = io.StringIO()
        error = subprocess.CalledProcessError(17, ["cargo", "fmt", "a%\r\n::warning::injected"], output="private output", stderr="private stderr")
        with patch.object(DEV.sys, "argv", ["dev.py", "lint"]), patch.object(DEV, "command", side_effect=error), patch.dict(DEV.os.environ, {"GITHUB_ACTIONS": github_actions}), contextlib.redirect_stdout(output):
            with self.assertRaises(SystemExit) as stopped:
                DEV.main()
        self.assertEqual(stopped.exception.code, 17)
        return output.getvalue()

    def test_actions_annotation_is_one_escaped_line_without_child_output(self):
        output = self.run_failure("true")
        self.assertEqual(output, "::error title=Validation command failed::Validation command failed (exit 17)\n")
        self.assertNotIn("private", output)
        self.assertNotIn("injected", output)
        self.assertEqual(len(output.splitlines()), 1)

    def test_local_failure_preserves_exit_without_actions_annotation(self):
        self.assertEqual(self.run_failure("false"), "")


class LabPackageManagerTests(unittest.TestCase):
    def test_repository_pnpm_is_preferred_when_bootstrapped(self):
        with patch.object(Path, "is_file", return_value=True), patch.object(
            DEV, "run"
        ) as run:
            DEV.lab_pnpm("run", "check")
        run.assert_called_once_with(
            "node",
            DEV.ROOT / ".tools/pnpm/package/bin/pnpm.mjs",
            "--dir",
            "tools/lab-mcp",
            "run",
            "check",
        )

    def test_corepack_cache_is_confined_to_the_worktree(self):
        with patch.object(Path, "is_file", return_value=False), patch.object(
            DEV, "run"
        ) as run:
            DEV.lab_pnpm("run", "check")
        args, kwargs = run.call_args
        self.assertEqual(
            args[:4], ("corepack", "pnpm@12.3.4", "--dir", "tools/lab-mcp")
        )
        self.assertEqual(
            kwargs["env"]["COREPACK_HOME"],
            str(DEV.ROOT / "tools/lab-mcp/.tools/corepack"),
        )
