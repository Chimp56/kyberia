"""Bounded, non-sensitive CI diagnostics for repository workflow annotations.

The parsers emit only grammar-validated compiler codes, repository-relative
Rust source locations, and test identifiers.  They never copy compiler
messages, exception text, captured test output, environment values, or command
arguments into an annotation.
"""

from __future__ import annotations

import json
import os
import posixpath
import re
import sys
from pathlib import Path
from typing import Any, Iterable, Optional


MAX_DIAGNOSTIC_LINE_BYTES = 64 * 1024
MAX_ANNOTATIONS = 20
MAX_CODE_BYTES = 128
MAX_TEST_NAME_BYTES = 256
MAX_SOURCE_PATH_BYTES = 512
MAX_SOURCE_LINE = 1_000_000
MAX_SOURCE_COLUMN = 1_000_000
MAX_PYTHON_TEST_ID_BYTES = 256

_CODE = re.compile(r"^(?:E[0-9]{4}|[A-Za-z][A-Za-z0-9_]*(?:::[A-Za-z][A-Za-z0-9_]*)?)$")
_TEST_NAME = re.compile(r"^[A-Za-z0-9_:.\-/]+$")
_PYTHON_TEST_COMPONENT = r"[A-Za-z_][A-Za-z0-9_]*"
_PYTHON_TEST_STATUS = re.compile(
    r"^(?P<method>" + _PYTHON_TEST_COMPONENT + r") "
    r"\((?P<parent>" + _PYTHON_TEST_COMPONENT + r"(?:\." + _PYTHON_TEST_COMPONENT + r")*)\) "
    r"\.\.\. (?P<status>FAIL|ERROR)$"
)
_PYTHON_TEST_HEADER = re.compile(
    r"^(?P<method>" + _PYTHON_TEST_COMPONENT + r") "
    r"\((?P<parent>" + _PYTHON_TEST_COMPONENT + r"(?:\." + _PYTHON_TEST_COMPONENT + r")*)\)$"
)
_RUNNING = re.compile(r"^running [0-9]+ tests?$")
_FAILED_TEST = re.compile(r"^test ([A-Za-z0-9_:.\-/]+) \.\.\. FAILED$")


def _escape_command_value(value: str) -> str:
    """Escape the three characters interpreted by GitHub command syntax."""
    return value.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


def _emit_annotation(title: str, message: str, *, kind: str = "error",
                     path: Optional[str] = None,
                     line: Optional[int] = None, column: Optional[int] = None) -> None:
    properties = []
    if path is not None:
        properties.append("file=" + _escape_command_value(path))
    if line is not None:
        properties.append("line=" + str(line))
    if column is not None:
        properties.append("col=" + str(column))
    properties.append("title=" + _escape_command_value(title))
    prefix = "::" + kind + (" " + ",".join(properties) if properties else "")
    print(prefix + "::" + _escape_command_value(message), flush=True)


def _safe_source_path(value: Any, root: Path) -> Optional[str]:
    if not isinstance(value, str):
        return None
    if len(value.encode("utf-8", "ignore")) > MAX_SOURCE_PATH_BYTES:
        return None
    if any(ord(char) < 0x20 for char in value):
        return None
    raw = value.replace("\\", "/")
    root_text = str(root).replace("\\", "/").rstrip("/")
    raw_compare = raw.casefold()
    root_compare = root_text.casefold()
    if raw.startswith("/") or re.match(r"^[A-Za-z]:/", raw):
        prefix = root_compare + "/"
        if not raw_compare.startswith(prefix):
            return None
        relative = raw[len(root_text) + 1:]
    else:
        relative = raw
    relative = posixpath.normpath(relative)
    if relative in {"", ".", ".."} or relative.startswith("../") or relative.startswith("/"):
        return None
    if not re.fullmatch(r"[A-Za-z0-9._/\-]+", relative) or not relative.endswith(".rs"):
        return None
    return relative


def _safe_test_name(value: Any) -> Optional[str]:
    if not isinstance(value, str) or len(value.encode("utf-8", "ignore")) > MAX_TEST_NAME_BYTES:
        return None
    if not _TEST_NAME.fullmatch(value):
        return None
    if any(part in {"", ".", ".."} for part in value.split("/")):
        return None
    return value


def _safe_python_test_id(value: Any) -> Optional[str]:
    if not isinstance(value, str) or len(value.encode("utf-8", "ignore")) > MAX_PYTHON_TEST_ID_BYTES:
        return None
    if not re.fullmatch(_PYTHON_TEST_COMPONENT + r"(?:\." + _PYTHON_TEST_COMPONENT + r")*", value):
        return None
    return value


class _BoundedLines:
    """Split a streamed byte stream without retaining an unbounded line."""

    def __init__(self) -> None:
        self._buffer = bytearray()
        self.oversized_lines = 0
        self._discarding = False

    def feed(self, chunk: bytes) -> Iterable[bytes]:
        if not chunk:
            return []
        if self._discarding:
            newline = chunk.find(b"\n")
            if newline < 0:
                return []
            self._discarding = False
            chunk = chunk[newline + 1:]
            if not chunk:
                return []
        data = bytes(self._buffer) + chunk
        lines = []
        start = 0
        while True:
            newline = data.find(b"\n", start)
            if newline < 0:
                remainder = data[start:]
                if len(remainder) > MAX_DIAGNOSTIC_LINE_BYTES:
                    self.oversized_lines += 1
                    self._buffer.clear()
                    self._discarding = True
                else:
                    self._buffer = bytearray(remainder)
                break
            line = data[start:newline]
            if len(line) > MAX_DIAGNOSTIC_LINE_BYTES:
                self.oversized_lines += 1
            else:
                lines.append(line.rstrip(b"\r"))
            start = newline + 1
        return lines

    def finish(self) -> Iterable[bytes]:
        if self._buffer and len(self._buffer) <= MAX_DIAGNOSTIC_LINE_BYTES:
            line = bytes(self._buffer).rstrip(b"\r")
            self._buffer.clear()
            return [line]
        if self._buffer:
            self.oversized_lines += 1
        self._buffer.clear()
        return []


class CargoDiagnosticParser:
    """Extract safe primary spans and compiler/lint codes from Cargo JSON."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self._lines = _BoundedLines()
        self._annotations: list[tuple[str, str, str, int, int, str]] = []
        self._seen: set[tuple[str, str, int, int, str]] = set()
        self._omitted_count = 0

    def feed(self, chunk: bytes) -> None:
        for line in self._lines.feed(chunk):
            self._parse_line(line)

    def finish(self) -> None:
        for line in self._lines.finish():
            self._parse_line(line)
        self._omitted_count += self._lines.oversized_lines

    def _parse_line(self, line: bytes) -> None:
        try:
            item = json.loads(line)
        except (
            UnicodeDecodeError,
            json.JSONDecodeError,
            TypeError,
            ValueError,
            RecursionError,
            MemoryError,
        ):
            return
        if not isinstance(item, dict) or item.get("reason") != "compiler-message":
            return
        message = item.get("message")
        if not isinstance(message, dict):
            return
        code = message.get("code")
        if not isinstance(code, dict):
            return
        code_value = code.get("code")
        if not isinstance(code_value, str) or len(code_value) > MAX_CODE_BYTES or not _CODE.fullmatch(code_value):
            return
        level = message.get("level")
        if level not in {"error", "warning"}:
            return
        primary = None
        spans = message.get("spans")
        if isinstance(spans, list):
            for span in spans:
                if not isinstance(span, dict) or span.get("is_primary") is not True:
                    continue
                path = _safe_source_path(span.get("file_name"), self.root)
                line_start = span.get("line_start")
                column_start = span.get("column_start")
                if path is None or not isinstance(line_start, int) or isinstance(line_start, bool):
                    continue
                if not isinstance(column_start, int) or isinstance(column_start, bool):
                    continue
                if not 1 <= line_start <= MAX_SOURCE_LINE or not 1 <= column_start <= MAX_SOURCE_COLUMN:
                    continue
                primary = (path, line_start, column_start)
                break
        if primary is None:
            return
        path, line_start, column_start = primary
        identity = (code_value, path, line_start, column_start, level)
        if identity in self._seen:
            return
        if len(self._annotations) >= MAX_ANNOTATIONS:
            self._omitted_count += 1
            return
        self._seen.add(identity)
        self._annotations.append((code_value, path, line_start, column_start, level))

    @property
    def annotation_count(self) -> int:
        return len(self._annotations)

    @property
    def omitted_count(self) -> int:
        return self._omitted_count

    def emit(self) -> None:
        for code, path, line, column, level in self._annotations:
            _emit_annotation(
                "Rust " + code,
                "Rust compiler diagnostic " + code,
                kind=level,
                path=path,
                line=line,
                column=column,
            )
        if self._omitted_count:
            _emit_annotation(
                "Rust diagnostics truncated",
                "Rust diagnostics omitted: " + str(self._omitted_count),
            )


class LibtestFailureParser:
    """Parse only the harness status phase, never captured failure bodies."""

    def __init__(self) -> None:
        self._lines = _BoundedLines()
        self._harness_running = False
        self._failure_details = False
        self._stopped_after_failure = False
        self._failures: list[str] = []
        self._seen: set[str] = set()
        self._omitted_count = 0

    def feed(self, chunk: bytes) -> None:
        for line in self._lines.feed(chunk):
            self._parse_line(line)

    def finish(self) -> None:
        for line in self._lines.finish():
            self._parse_line(line)

    def _parse_line(self, line: bytes) -> None:
        if self._stopped_after_failure:
            return
        try:
            text = line.decode("utf-8")
        except UnicodeDecodeError:
            return
        if _RUNNING.fullmatch(text):
            self._harness_running = True
            self._failure_details = False
            return
        if not self._harness_running:
            return
        if text.startswith("test result:"):
            self._harness_running = False
            self._failure_details = False
            return
        if text == "failures:":
            # Stable libtest has no authenticated machine-readable boundary
            # around captured output.  Permanently stop at the first failure
            # section so output containing a forged `running` or `test result`
            # line cannot create later public annotations.
            self._stopped_after_failure = True
            self._failure_details = True
            self._harness_running = False
            return
        if self._failure_details:
            return
        match = _FAILED_TEST.fullmatch(text)
        if not match:
            return
        name = _safe_test_name(match.group(1))
        if name is None or name in self._seen:
            return
        if len(self._failures) >= MAX_ANNOTATIONS:
            self._omitted_count += 1
            return
        self._seen.add(name)
        self._failures.append(name)

    @property
    def failures(self) -> tuple[str, ...]:
        return tuple(self._failures)

    @property
    def omitted_count(self) -> int:
        return self._omitted_count + self._lines.oversized_lines

    def emit(self) -> None:
        for name in self._failures:
            _emit_annotation("Rust test failed", "Rust test failed: " + name)
        if self.omitted_count:
            _emit_annotation("Rust tests truncated", "Rust test identifiers omitted: " + str(self.omitted_count))


class PythonUnittestFailureParser:
    """Parse verbose unittest result lines without copying failure details.

    Python's stable text runner has no authenticated machine-readable stream.
    The parser therefore accepts only the narrow verbose status grammar before
    the first failure-detail separator.  Tracebacks, exception messages,
    subtest parameters, and all later text are permanently excluded.  It
    accepts both runner forms where the parenthesized value is either the
    class/module or the already-qualified test identifier.
    """

    def __init__(self) -> None:
        self._lines = _BoundedLines()
        self._failure_details = False
        self._pending_header: Optional[tuple[str, str]] = None
        self._failures: list[tuple[str, str]] = []
        self._seen: set[str] = set()
        self._omitted_count = 0

    def feed(self, chunk: bytes) -> None:
        for line in self._lines.feed(chunk):
            self._parse_line(line)

    def finish(self) -> None:
        for line in self._lines.finish():
            self._parse_line(line)

    def _parse_line(self, line: bytes) -> None:
        if self._failure_details:
            return
        try:
            text = line.decode("utf-8")
        except UnicodeDecodeError:
            return
        if re.fullmatch(r"={10,}", text):
            # Failure headings and captured output follow this separator.
            # They are deliberately not parsed because test code controls
            # those bytes and can print arbitrary look-alike lines.
            self._failure_details = True
            self._pending_header = None
            return
        match = _PYTHON_TEST_STATUS.fullmatch(text)
        if match:
            self._pending_header = None
            self._record(match.group("method"), match.group("parent"), match.group("status"))
            return
        header = _PYTHON_TEST_HEADER.fullmatch(text)
        if header:
            self._pending_header = (header.group("method"), header.group("parent"))
            return
        if self._pending_header is None:
            return
        # TextTestResult includes the first docstring line between the header
        # and status.  Split only on the final bounded delimiter and never
        # retain or emit the description itself.
        if len(text.encode("utf-8", "ignore")) > MAX_DIAGNOSTIC_LINE_BYTES:
            self._pending_header = None
            return
        prefix, separator, status = text.rpartition(" ... ")
        pending = self._pending_header
        self._pending_header = None
        if separator and prefix and status in {"FAIL", "ERROR"}:
            self._record(pending[0], pending[1], status)

    def _record(self, method: str, parent: str, status: str) -> None:
        if parent.rsplit(".", 1)[-1] == method:
            identifier_text = parent
        else:
            identifier_text = parent + "." + method
        identifier = _safe_python_test_id(identifier_text)
        if identifier is None or identifier in self._seen:
            return
        if len(self._failures) >= MAX_ANNOTATIONS:
            self._omitted_count += 1
            return
        self._seen.add(identifier)
        self._failures.append((identifier, status))

    @property
    def failures(self) -> tuple[tuple[str, str], ...]:
        return tuple(self._failures)

    @property
    def omitted_count(self) -> int:
        return self._omitted_count + self._lines.oversized_lines

    def emit(self) -> None:
        for identifier, status in self._failures:
            _emit_annotation(
                "Python unittest " + status.lower(),
                "Python unittest failed: " + identifier,
            )
        if self.omitted_count:
            _emit_annotation(
                "Python unittest diagnostics truncated",
                "Python unittest identifiers omitted: " + str(self.omitted_count),
            )


def parser_for(kind: str, root: Path) -> Any:
    if kind == "cargo":
        return CargoDiagnosticParser(root)
    if kind == "libtest":
        return LibtestFailureParser()
    if kind == "python-unittest":
        return PythonUnittestFailureParser()
    raise ValueError("unknown CI diagnostics parser")


def github_actions_enabled() -> bool:
    return os.environ.get("GITHUB_ACTIONS") == "true"
