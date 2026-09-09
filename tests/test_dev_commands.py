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
        self.assertEqual(output, "::error title=Validation command failed::Validation command failed (exit 17): cargo fmt a%25%0D%0A::warning::injected\n")
        self.assertNotIn("private", output)
        self.assertEqual(len(output.splitlines()), 1)

    def test_local_failure_preserves_exit_without_actions_annotation(self):
        self.assertEqual(self.run_failure("false"), "")
