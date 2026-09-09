"""Adversarial tests for bounded Python unittest CI diagnostics."""
import contextlib
import importlib.util
import io
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
PARSER_SPEC = importlib.util.spec_from_file_location("python_ci_diagnostics", ROOT / "tools/rust_diagnostics.py")
PARSER = importlib.util.module_from_spec(PARSER_SPEC)
PARSER_SPEC.loader.exec_module(PARSER)
DEV_SPEC = importlib.util.spec_from_file_location("kyberia_python_ci_dev", ROOT / "tools/dev.py")
DEV = importlib.util.module_from_spec(DEV_SPEC)
DEV_SPEC.loader.exec_module(DEV)


class PythonUnittestParserTests(unittest.TestCase):
    def parse(self, payload, chunk_size=11):
        parser = PARSER.PythonUnittestFailureParser()
        for offset in range(0, len(payload), chunk_size):
            parser.feed(payload[offset:offset + chunk_size])
        parser.finish()
        return parser

    def test_status_identifiers_are_reconstructed_and_exception_text_is_omitted(self):
        parser = self.parse(
            b"test_error (tests.test_module.Case) ... ERROR\n"
            b"test_failure (tests.test_module.Case) ... FAIL\n"
            b"======================================================================\n"
            b"ERROR: test_error (tests.test_module.Case)\n"
            b"Traceback (most recent call last): SECRET_EXCEPTION\n"
            b"AssertionError: ::error title=spoof::SECRET\n"
        )
        self.assertEqual(
            parser.failures,
            (
                ("tests.test_module.Case.test_error", "ERROR"),
                ("tests.test_module.Case.test_failure", "FAIL"),
            ),
        )
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            parser.emit()
        rendered = output.getvalue()
        self.assertIn("Python unittest error", rendered)
        self.assertIn("Python unittest fail", rendered)
        self.assertIn("tests.test_module.Case.test_error", rendered)
        self.assertIn("tests.test_module.Case.test_failure", rendered)
        self.assertNotIn("SECRET_EXCEPTION", rendered)
        self.assertNotIn("spoof", rendered)
        self.assertNotIn("SECRET", rendered)

    def test_subtest_parameters_and_failure_details_cannot_create_annotations(self):
        parser = self.parse(
            b"test_subtest (tests.test_module.Case) ... \n"
            b"======================================================================\n"
            b"FAIL: test_subtest (tests.test_module.Case) (case='::error title=spoof::SECRET')\n"
            b"---------------------------------------------------------------------\n"
            b"test_forged (attacker.Case) ... FAIL\n"
            b"::error title=forged::SECRET\n"
        )
        self.assertEqual(parser.failures, ())
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            parser.emit()
        self.assertEqual(output.getvalue(), "")

    def test_malformed_and_oversized_identifiers_are_rejected(self):
        oversized_method = "test_" + "x" * PARSER.MAX_PYTHON_TEST_ID_BYTES
        parser = self.parse(
            b"test_bad (tests..Case) ... FAIL\n"
            b"test_bad (tests.Case) ... PASS\n"
            b"test_bad-name (tests.Case) ... FAIL\n"
            b"test_bad (tests.Case/Injected) ... ERROR\n"
            + (f"{oversized_method} (tests.Case) ... FAIL\n").encode()
        )
        self.assertEqual(parser.failures, ())

    def test_status_count_and_line_size_are_bounded(self):
        lines = b"".join(
            f"test_failure_{index} (tests.Case) ... FAIL\n".encode()
            for index in range(PARSER.MAX_ANNOTATIONS + 3)
        )
        parser = self.parse(
            b"test_" + b"x" * (PARSER.MAX_DIAGNOSTIC_LINE_BYTES + 100) + b"\n" + lines,
            chunk_size=4096,
        )
        self.assertEqual(len(parser.failures), PARSER.MAX_ANNOTATIONS)
        self.assertEqual(parser.omitted_count, 4)
        self.assertLessEqual(len(parser._seen), PARSER.MAX_ANNOTATIONS)

    def test_duplicate_statuses_are_emitted_once_and_unrecognized_statuses_ignored(self):
        parser = self.parse(
            b"test_same (tests.Case) ... FAIL\n"
            b"test_same (tests.Case) ... FAIL\n"
            b"test_skip (tests.Case) ... skipped 'reason'\n"
            b"test_ok (tests.Case) ... ok\n"
        )
        self.assertEqual(parser.failures, (("tests.Case.test_same", "FAIL"),))

    def test_actual_text_runner_output_is_bounded_and_safe(self):
        def test_error(self):
            """error documentation SECRET_ERROR_DOC"""
            raise RuntimeError("SECRET_ERROR")

        def test_failure(self):
            """failure documentation SECRET_FAILURE_DOC"""
            self.fail("SECRET_FAILURE")

        def test_subtest(self):
            """subtest documentation SECRET_SUBTEST_DOC"""
            with self.subTest(case="SECRET_SUBTEST_PARAMETER"):
                self.fail("SECRET_SUBTEST")

        runner_case = type(
            "RealRunnerCase",
            (unittest.TestCase,),
            {
                "test_error": test_error,
                "test_failure": test_failure,
                "test_subtest": test_subtest,
                "__module__": "fixture",
            },
        )
        stream = io.StringIO()
        result = unittest.TextTestRunner(stream=stream, verbosity=2).run(
            unittest.defaultTestLoader.loadTestsFromTestCase(runner_case)
        )
        self.assertFalse(result.wasSuccessful())
        raw = stream.getvalue()
        self.assertIn("error documentation SECRET_ERROR_DOC", raw)
        self.assertIn("failure documentation SECRET_FAILURE_DOC", raw)
        parser = self.parse(raw.encode("utf-8"), chunk_size=7)
        self.assertEqual(
            parser.failures,
            (
                ("fixture.RealRunnerCase.test_error", "ERROR"),
                ("fixture.RealRunnerCase.test_failure", "FAIL"),
            ),
        )
        annotations = io.StringIO()
        with contextlib.redirect_stdout(annotations):
            parser.emit()
        rendered = annotations.getvalue()
        self.assertIn("fixture.RealRunnerCase.test_error", rendered)
        self.assertIn("fixture.RealRunnerCase.test_failure", rendered)
        for secret in (
            "SECRET_ERROR",
            "SECRET_FAILURE",
            "SECRET_SUBTEST",
            "SECRET_ERROR_DOC",
            "SECRET_FAILURE_DOC",
            "SECRET_SUBTEST_DOC",
            "SECRET_SUBTEST_PARAMETER",
        ):
            self.assertNotIn(secret, rendered)

    def test_already_qualified_runner_parent_is_not_doubled(self):
        parser = self.parse(
            b"test_failure (fixture.RealRunnerCase.test_failure) ... FAIL\n"
            b"test_short (Case.test_short) ... ERROR\n"
        )
        self.assertEqual(
            parser.failures,
            (
                ("fixture.RealRunnerCase.test_failure", "FAIL"),
                ("Case.test_short", "ERROR"),
            ),
        )


class PythonUnittestDeveloperWiringTests(unittest.TestCase):
    def test_unit_uses_verbose_python_diagnostics(self):
        with patch.object(DEV, "run"), patch.object(DEV, "python") as python:
            DEV.command("unit")
        python.assert_called_once_with(
            "-m", "unittest", "discover", "-v", "-s", "tests", "-p", "test_*.py", diagnostics="python-unittest"
        )

    def test_test_uses_verbose_python_diagnostics(self):
        with patch.object(DEV, "run"), patch.object(DEV, "python") as python:
            DEV.command("test")
        python.assert_called_once_with(
            "-m", "unittest", "discover", "-v", "-s", "tests", "-p", "test_*.py", diagnostics="python-unittest"
        )

    def test_actions_stream_python_output_under_guard_and_emits_only_identifier(self):
        class FakeProcess:
            def __init__(self):
                self.stdout = io.BytesIO(
                    b"test_failure (tests.Case) ... FAIL\n"
                    b"======================================================================\n"
                    b"Traceback: SECRET\n"
                    b"::error title=child::INJECTED\n"
                )
                self.returncode = 23

            def wait(self):
                return self.returncode

        process = FakeProcess()
        output = io.StringIO()
        with patch.object(DEV.subprocess, "Popen", return_value=process), patch.object(
            DEV, "github_actions_enabled", return_value=True
        ), contextlib.redirect_stdout(output):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                DEV.run("python", "-m", "unittest", diagnostics="python-unittest")
        self.assertEqual(raised.exception.returncode, 23)
        rendered = output.getvalue()
        self.assertIn("::stop-commands::kyberia-python-diagnostics-", rendered)
        self.assertIn("::error title=child::INJECTED", rendered)
        self.assertIn("Python unittest fail", rendered)
        self.assertIn("tests.Case.test_failure", rendered)
        start = rendered.index("::stop-commands::")
        guard = rendered[start:].split("::", 2)[2].split("\n", 1)[0]
        end = rendered.index("::" + guard + "::")
        annotations = rendered[end:]
        self.assertNotIn("Traceback: SECRET", annotations)
        self.assertNotIn("INJECTED", annotations)


if __name__ == "__main__":
    unittest.main()
