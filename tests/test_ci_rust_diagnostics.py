"""Adversarial tests for bounded, public Rust CI diagnostics."""
import contextlib
import importlib.util
import io
import json
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
PARSER_SPEC = importlib.util.spec_from_file_location("rust_diagnostics", ROOT / "tools/rust_diagnostics.py")
PARSER = importlib.util.module_from_spec(PARSER_SPEC)
PARSER_SPEC.loader.exec_module(PARSER)
DEV_SPEC = importlib.util.spec_from_file_location("kyberia_dev_diagnostics", ROOT / "tools/dev.py")
DEV = importlib.util.module_from_spec(DEV_SPEC)
DEV_SPEC.loader.exec_module(DEV)


def compiler_line(code="E0308", file_name="crates/domain/src/lib.rs", line=12, column=4, message="private", level="error"):
    return (json.dumps({
        "reason": "compiler-message",
        "message": {
            "code": {"code": code},
            "level": level,
            "message": message,
            "spans": [{
                "file_name": file_name,
                "is_primary": True,
                "line_start": line,
                "column_start": column,
            }],
        },
    }) + "\n").encode()


class RustDiagnosticParserTests(unittest.TestCase):
    def test_cargo_parser_emits_only_validated_code_and_location(self):
        parser = PARSER.CargoDiagnosticParser(ROOT)
        payload = compiler_line(
            code="clippy::needless_return",
            message="PRIVATE compiler text\n::error title=spoof::secret",
        )
        for offset in range(0, len(payload), 7):
            parser.feed(payload[offset:offset + 7])
        parser.finish()
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            parser.emit()
        rendered = output.getvalue()
        self.assertIn("Rust clippy::needless_return", rendered)
        self.assertIn("file=crates/domain/src/lib.rs", rendered)
        self.assertIn("line=12,col=4", rendered)
        self.assertNotIn("PRIVATE", rendered)
        self.assertNotIn("spoof", rendered)
        self.assertNotIn("secret", rendered)

    def test_cargo_parser_rejects_untrusted_code_paths_and_locations(self):
        parser = PARSER.CargoDiagnosticParser(ROOT)
        parser.feed(compiler_line(code="::error title=spoof", file_name="../outside.rs"))
        parser.feed(compiler_line(file_name="/tmp/outside.rs"))
        parser.feed(compiler_line(line=True))
        parser.feed(compiler_line(file_name="crates\\domain\\src\\lib.rs", line=9, column=2))
        parser.finish()
        self.assertEqual(parser.annotation_count, 1)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            parser.emit()
        self.assertIn("crates/domain/src/lib.rs", output.getvalue())
        self.assertNotIn("outside", output.getvalue())

    def test_cargo_parser_bounds_malformed_lines_and_annotation_count(self):
        parser = PARSER.CargoDiagnosticParser(ROOT)
        parser.feed(b"{" + b"x" * (PARSER.MAX_DIAGNOSTIC_LINE_BYTES + 100) + b"}\n")
        for index in range(PARSER.MAX_ANNOTATIONS + 3):
            parser.feed(compiler_line(line=index + 1))
        parser.finish()
        self.assertEqual(parser.annotation_count, PARSER.MAX_ANNOTATIONS)
        self.assertEqual(parser.omitted_count, 4)
        self.assertLessEqual(len(parser._seen), PARSER.MAX_ANNOTATIONS)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            parser.emit()
        rendered = output.getvalue()
        self.assertNotIn("x" * 100, rendered)
        self.assertIn("Rust diagnostics omitted: 4", rendered)

    def test_cargo_code_length_is_bounded(self):
        parser = PARSER.CargoDiagnosticParser(ROOT)
        parser.feed(compiler_line(code="a" * (PARSER.MAX_CODE_BYTES + 1)))
        parser.finish()
        self.assertEqual(parser.annotation_count, 0)

    def test_source_and_test_name_bounds_are_path_safe(self):
        self.assertEqual(
            PARSER._safe_source_path("C:\\repo\\crates\\domain\\src\\lib.rs", Path("C:/repo")),
            "crates/domain/src/lib.rs",
        )
        self.assertIsNone(PARSER._safe_source_path("C:/repo2/lib.rs", Path("C:/repo")))
        self.assertIsNone(PARSER._safe_source_path("../outside.rs", ROOT))
        self.assertIsNone(PARSER._safe_test_name("../outside"))
        self.assertIsNone(PARSER._safe_test_name("bad\n::error"))

    def test_libtest_parser_ignores_captured_failure_text(self):
        parser = PARSER.LibtestFailureParser()
        parser.feed(
            b"fake test forged ... FAILED\n"
            b"running 2 tests\n"
            b"test safe::bad ... FAILED\n"
            b"test safe::ok ... ok\n"
            b"failures:\n"
            b"---- safe::bad stdout ----\n"
            b"test spoof ... FAILED\n"
            b"::error title=spoof::private\n"
            b"test result: FAILED. 1 passed; 1 failed\n"
            b"running 1 test\n"
            b"test second ... FAILED\n"
            b"test result: FAILED. 0 passed; 1 failed\n"
        )
        parser.finish()
        self.assertEqual(parser.failures, ("safe::bad", "second"))
        self.assertLessEqual(len(parser._seen), PARSER.MAX_ANNOTATIONS)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            parser.emit()
        rendered = output.getvalue()
        self.assertIn("Rust test failed: safe::bad", rendered)
        self.assertIn("Rust test failed: second", rendered)
        self.assertNotIn("spoof", rendered)
        self.assertNotIn("private", rendered)


class FakeProcess:
    def __init__(self, chunks, returncode=0):
        self.stdout = io.BytesIO(b"".join(chunks))
        self.returncode = returncode

    def wait(self):
        return self.returncode


class DeveloperStreamingTests(unittest.TestCase):
    def test_actions_guards_unstructured_child_output_too(self):
        process = FakeProcess([b"::error title=child::SECRET\n"])
        output = io.StringIO()
        with patch.object(DEV.subprocess, "Popen", return_value=process), patch.object(DEV, "github_actions_enabled", return_value=True), contextlib.redirect_stdout(output):
            DEV.run("python", "tools/architecture.py")
        rendered = output.getvalue()
        start = rendered.index("::stop-commands::")
        guard = rendered[start:].split("::", 2)[2].split("\n", 1)[0]
        end = rendered.index("::" + guard + "::")
        child = rendered.index("::error title=child::SECRET")
        self.assertGreater(child, start)
        self.assertLess(child, end)

    def test_actions_streams_raw_output_and_emits_safe_cargo_annotation(self):
        process = FakeProcess([compiler_line(message="PRIVATE")])
        output = io.StringIO()
        with patch.object(DEV.subprocess, "Popen", return_value=process) as popen, patch.object(DEV, "github_actions_enabled", return_value=True), contextlib.redirect_stdout(output):
            DEV.run("cargo", "clippy", "--workspace", "--", "-D", "warnings", diagnostics="cargo")
        rendered = output.getvalue()
        command = popen.call_args.args[0]
        self.assertIn("--message-format=json", command)
        self.assertLess(command.index("--message-format=json"), command.index("--"))
        self.assertIn("PRIVATE", rendered)
        self.assertIn("Rust E0308", rendered)
        self.assertIn("Rust compiler diagnostic E0308", rendered)

    def test_cargo_warning_uses_warning_annotation_kind(self):
        parser = PARSER.CargoDiagnosticParser(ROOT)
        parser.feed(compiler_line(code="dead_code", level="warning"))
        parser.finish()
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            parser.emit()
        self.assertIn("::warning", output.getvalue())

    def test_actions_preserves_failure_status_and_safe_test_annotation(self):
        process = FakeProcess([
            b"running 1 test\ntest safe::broken ... FAILED\nfailures:\n",
            b"::error title=child::SECRET\n",
        ], returncode=23)
        output = io.StringIO()
        with patch.object(DEV.subprocess, "Popen", return_value=process), patch.object(DEV, "github_actions_enabled", return_value=True), contextlib.redirect_stdout(output):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                DEV.run("cargo", "test", "--workspace", diagnostics="libtest")
        self.assertEqual(raised.exception.returncode, 23)
        rendered = output.getvalue()
        self.assertIn("::error", rendered)
        self.assertIn("Rust test failed: safe::broken", rendered)
        self.assertIn("SECRET", rendered)
        start = rendered.index("::stop-commands::")
        guard = rendered[start:].split("::", 2)[2].split("\n", 1)[0]
        end = rendered.index("::" + guard + "::")
        self.assertGreater(rendered.index("::error title=child::SECRET"), start)
        self.assertLess(rendered.index("::error title=child::SECRET"), end)
        self.assertGreater(rendered.index("Rust test failed: safe::broken"), end)

    def test_actions_terminates_guard_after_output_without_newline(self):
        process = FakeProcess([b"partial"])
        output = io.StringIO()
        with patch.object(DEV.subprocess, "Popen", return_value=process), patch.object(DEV, "github_actions_enabled", return_value=True), contextlib.redirect_stdout(output):
            DEV.run("cargo", "check", diagnostics="cargo")
        rendered = output.getvalue()
        self.assertRegex(rendered, r"partial\n::kyberia-rust-diagnostics-[0-9a-f]{32}::\n$")

    def test_local_run_keeps_original_subprocess_path_and_arguments(self):
        with patch.object(DEV, "github_actions_enabled", return_value=False), patch.object(DEV.subprocess, "run") as run, patch.object(DEV.subprocess, "Popen") as popen:
            DEV.run("cargo", "check", "--workspace", diagnostics="cargo")
        run.assert_called_once_with(["cargo", "check", "--workspace"], cwd=DEV.ROOT, check=True)
        popen.assert_not_called()


if __name__ == "__main__":
    unittest.main()
