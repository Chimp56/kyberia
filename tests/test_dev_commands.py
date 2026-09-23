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

    def test_windows_uses_corepack_cmd_without_enabling_a_shell(self):
        with patch.object(DEV.os, "name", "nt"), patch.object(
            Path, "is_file", return_value=False
        ), patch.object(DEV, "run") as run:
            DEV.lab_pnpm("install", "--frozen-lockfile")

        args, kwargs = run.call_args
        self.assertEqual(args[:2], ("corepack.cmd", "pnpm@12.3.4"))
        self.assertEqual(args[2:6], ("--dir", "tools/lab-mcp", "install", "--frozen-lockfile"))
        self.assertNotIn("shell", kwargs)
        self.assertEqual(
            kwargs["env"]["COREPACK_HOME"],
            str(DEV.ROOT / "tools/lab-mcp/.tools/corepack"),
        )

    def test_lab_bootstrap_store_is_relative_to_package_directory(self):
        with patch.object(DEV, "lab_pnpm") as pnpm:
            DEV.command("lab-mcp-bootstrap")
        pnpm.assert_called_once_with(
            "install", "--frozen-lockfile", "--store-dir", ".tools/pnpm-store"
        )

    def test_antenna_schema_command_uses_configured_tool_python(self):
        with patch.object(DEV, "python") as python:
            DEV.command("validate-antenna-schema")
        python.assert_called_once_with("tools/validate_antenna_schema.py")


class BootstrapStageDiagnosticTests(unittest.TestCase):
    def test_bootstrap_wraps_each_subcommand_with_a_closed_logical_stage_id(self):
        stages = []

        def record(stage, operation):
            stages.append(stage)
            return operation()

        with patch.object(DEV, "_run_bootstrap_stage", side_effect=record), patch.object(
            DEV, "run"
        ), patch.object(DEV, "python"), patch.object(DEV, "lab_pnpm"):
            DEV.command("bootstrap")

        self.assertEqual(stages, list(DEV.BOOTSTRAP_STAGE_IDS))

    def test_each_stage_keeps_only_its_allowlisted_id_on_failure(self):
        for stage in DEV.BOOTSTRAP_STAGE_IDS:
            with self.subTest(stage=stage):
                error = subprocess.CalledProcessError(
                    29,
                    ["hostile-tool", "--token", "SECRET"],
                    output="PRIVATE_CHILD_OUTPUT",
                    stderr="PRIVATE_STDERR",
                )
                with self.assertRaises(subprocess.CalledProcessError) as raised:
                    DEV._run_bootstrap_stage(stage, lambda: (_ for _ in ()).throw(error))
                self.assertIs(raised.exception, error)
                self.assertEqual(error.kyberia_stage, stage)

    def test_actions_bootstrap_diagnostic_hides_command_and_child_text(self):
        class FakeProcess:
            stdout = io.BytesIO(
                b"::error title=spoof::SECRET_CHILD\n"
                b"private path C:\\Users\\runner\\token.txt\n"
            )
            returncode = 23

            def wait(self):
                return self.returncode

        def failing_command(_name):
            return DEV._run_bootstrap_stage(
                "bootstrap.cargo-fetch",
                lambda: DEV.run("cargo", "fetch", "--token", "SECRET"),
            )

        output = io.StringIO()
        with patch.object(DEV.sys, "argv", ["dev.py", "bootstrap"]), patch.object(
            DEV, "command", side_effect=failing_command
        ), patch.object(DEV.subprocess, "Popen", return_value=FakeProcess()), patch.object(
            DEV, "github_actions_enabled", return_value=True
        ), contextlib.redirect_stdout(output):
            with self.assertRaises(SystemExit) as stopped:
                DEV.main()

        self.assertEqual(stopped.exception.code, 23)
        rendered = output.getvalue()
        self.assertIn("Kyberia validation stage bootstrap.cargo-fetch", rendered)
        self.assertIn("Validation command failed at stage bootstrap.cargo-fetch", rendered)
        for secret in (
            "--token",
            "SECRET",
            "SECRET_CHILD",
            "private path",
            "Users\\runner",
            "spoof",
        ):
            self.assertNotIn(secret, rendered)
        self.assertEqual(len(rendered.splitlines()), 4)

    def test_actions_bootstrap_start_failure_is_bounded_and_local_start_failure_is_preserved(self):
        hostile_start_error = OSError(
            "SECRET startup detail at C:\\Users\\runner\\private-tool.exe"
        )

        def failing_command(_name):
            return DEV._run_bootstrap_stage(
                "bootstrap.lab-pnpm",
                lambda: DEV.run("private-tool", "--secret", "TOKEN"),
            )

        output = io.StringIO()
        with patch.object(DEV.sys, "argv", ["dev.py", "bootstrap"]), patch.object(
            DEV, "command", side_effect=failing_command
        ), patch.object(DEV.subprocess, "Popen", side_effect=hostile_start_error), patch.object(
            DEV, "github_actions_enabled", return_value=True
        ), contextlib.redirect_stdout(output):
            with self.assertRaises(SystemExit) as stopped:
                DEV.main()

        self.assertEqual(stopped.exception.code, 1)
        rendered = output.getvalue()
        self.assertIn("Kyberia validation stage bootstrap.lab-pnpm", rendered)
        self.assertIn("Validation command failed at stage bootstrap.lab-pnpm (exit 1)", rendered)
        for secret in (
            "startup detail",
            "Users\\runner",
            "private-tool",
            "--secret",
            "TOKEN",
        ):
            self.assertNotIn(secret, rendered)
        self.assertEqual(len(rendered.splitlines()), 4)

        local_start_error = OSError("local startup detail")
        with patch.object(DEV, "github_actions_enabled", return_value=False), patch.object(
            DEV.subprocess, "run", side_effect=local_start_error
        ), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(OSError) as raised:
                DEV._run_bootstrap_stage(
                    "bootstrap.python-venv",
                    lambda: DEV.run("private-tool", "--secret", "TOKEN"),
                )
        self.assertIs(raised.exception, local_start_error)

    def test_root_bootstrap_uses_same_package_relative_store(self):
        with patch.object(DEV, "run"), patch.object(DEV, "python"), patch.object(
            DEV, "lab_pnpm"
        ) as pnpm:
            DEV.command("bootstrap")
        pnpm.assert_called_once_with(
            "install", "--frozen-lockfile", "--store-dir", ".tools/pnpm-store"
        )
