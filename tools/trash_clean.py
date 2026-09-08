#!/usr/bin/env python3
"""Move explicitly known Kyberia outputs into an ignored trash run.

This module deliberately has no dependency on the project build graph.  The
clean operation is a conservative filesystem operation: it renames known
root-level outputs into a newly reserved directory below ``.trash`` and
records every move in a manifest.  It never deletes files and it never
dereferences an output symlink.
"""

from __future__ import annotations

import argparse
import json
import os
import stat
import sys
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional, Sequence


# This is the complete command allowlist.  Keep it root-level and explicit:
# .tools/.venv and dependency stores are intentionally absent because they are
# bootstrap environments or caches that may contain user-managed state.
KNOWN_OUTPUTS = (
    Path("target"),
    Path("dist"),
    Path("coverage"),
    Path("test-results"),
    Path("playwright-report"),
)

MAX_RUN_ID_ATTEMPTS = 100


class CleanConfigurationError(ValueError):
    """The requested clean root or trash location cannot be used safely."""


@dataclass
class CleanReport:
    """A complete, serializable account of one clean attempt."""

    root: str
    moved: List[Dict[str, str]] = field(default_factory=list)
    failures: List[Dict[str, str]] = field(default_factory=list)
    run_path: Optional[str] = None
    manifest_path: Optional[str] = None

    @property
    def succeeded(self) -> bool:
        return not self.failures

    @property
    def partial(self) -> bool:
        return bool(self.moved) and bool(self.failures)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "schema_version": 1,
            "root": self.root,
            "status": "success" if self.succeeded else ("partial" if self.partial else "failed"),
            "moved": list(self.moved),
            "failures": list(self.failures),
            "run_path": self.run_path,
            "manifest_path": self.manifest_path,
        }


MoveFunction = Callable[[Path, Path], None]
RunIdFactory = Callable[[], str]


def _default_run_id() -> str:
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return f"{timestamp}-{os.getpid()}-{uuid.uuid4().hex}"


def _lexists(path: Path) -> bool:
    """Return whether a path exists, including a broken symlink."""

    return os.path.lexists(str(path))


def _ensure_non_symlink_directory(path: Path, *, create: bool) -> None:
    if _lexists(path) and path.is_symlink():
        raise CleanConfigurationError(f"refusing symlink directory: {path}")
    if _lexists(path) and not path.is_dir():
        raise CleanConfigurationError(f"trash location is not a directory: {path}")
    if create and not _lexists(path):
        path.mkdir(parents=True, exist_ok=False)


def _reserve_run_directory(trash_root: Path, run_id_factory: RunIdFactory) -> Path:
    """Reserve a unique run directory without replacing an existing one."""

    _ensure_non_symlink_directory(trash_root.parent, create=True)
    _ensure_non_symlink_directory(trash_root, create=True)
    for _ in range(MAX_RUN_ID_ATTEMPTS):
        run_id = run_id_factory()
        if not run_id or Path(run_id).name != run_id or run_id in {".", ".."}:
            raise CleanConfigurationError("run id factory returned an unsafe name")
        run_path = trash_root / run_id
        try:
            run_path.mkdir(exist_ok=False)
            return run_path
        except FileExistsError:
            continue
    raise CleanConfigurationError("could not reserve a unique clean run directory")


def _relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def _failure(root: Path, relative: Path, reason: str, error: Optional[BaseException] = None) -> Dict[str, str]:
    result = {
        "path": str(root / relative),
        "relative_path": relative.as_posix(),
        "reason": reason,
    }
    if error is not None:
        result["error_type"] = type(error).__name__
        result["error"] = str(error)
    return result


def _move_entry(source: Path, destination: Path, move: MoveFunction) -> None:
    # The run directory is exclusively reserved by this invocation.  The
    # existence check makes a pre-existing destination an explicit failure;
    # Path.rename then performs the same-filesystem move without walking the
    # source tree or dereferencing symlinks inside a directory.
    if _lexists(destination):
        raise FileExistsError(f"destination already exists: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    move(source, destination)


def _write_manifest(run_path: Path, payload: Dict[str, Any]) -> Path:
    manifest = run_path / "manifest.json"
    pending = run_path / ".manifest.json.pending"
    if _lexists(manifest) or _lexists(pending):
        raise FileExistsError("manifest destination already exists")
    encoded = (json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")
    with pending.open("xb") as handle:
        handle.write(encoded)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(pending, manifest)
    try:
        directory_fd = os.open(str(run_path), os.O_RDONLY)
    except OSError:
        directory_fd = None
    if directory_fd is not None:
        try:
            # Directory fsync is supported on Unix filesystems.  Some
            # standard-library Windows handles reject it even though the
            # manifest rename itself succeeded; portability must not report a
            # false move failure in that case.
            try:
                os.fsync(directory_fd)
            except OSError:
                pass
        finally:
            os.close(directory_fd)
    return manifest


def _validate_known_outputs(known_outputs: Iterable[Path]) -> Sequence[Path]:
    result = []
    for value in known_outputs:
        relative = Path(value)
        if relative.is_absolute() or relative == Path(".") or ".." in relative.parts:
            raise CleanConfigurationError(f"known output is outside the root: {relative}")
        result.append(relative)
    return tuple(result)


def clean(
    root: Path,
    *,
    run_id_factory: RunIdFactory = _default_run_id,
    move: Optional[MoveFunction] = None,
) -> CleanReport:
    """Move allowlisted outputs under ``root`` into a unique trash run.

    The optional ``move`` hook exists only for deterministic failure testing;
    production callers use the default same-filesystem rename.
    """

    root = Path(root).resolve(strict=True)
    if not root.is_dir():
        raise CleanConfigurationError(f"clean root is not a directory: {root}")
    # Do not expose an allowlist override: callers cannot turn this safe
    # command into a generic path mover for .git, stores, or user data.
    relative_outputs = _validate_known_outputs(KNOWN_OUTPUTS)
    report = CleanReport(root=str(root))
    trash_root = root / ".trash" / "clean-runs"
    move_entry = move or (lambda source, destination: source.rename(destination))

    movable: List[Path] = []
    for relative in relative_outputs:
        source = root / relative
        try:
            source_stat = source.lstat()
        except FileNotFoundError:
            continue
        except OSError as error:
            report.failures.append(_failure(root, relative, "could not inspect known output", error))
            continue

        mode = source_stat.st_mode
        if stat.S_ISLNK(mode):
            report.failures.append(_failure(root, relative, "refusing symlink output; target was not inspected or moved"))
        elif stat.S_ISDIR(mode) or stat.S_ISREG(mode):
            movable.append(relative)
        else:
            report.failures.append(_failure(root, relative, "refusing special-file output"))

    if not movable:
        return report

    try:
        run_path = _reserve_run_directory(trash_root, run_id_factory)
    except (OSError, CleanConfigurationError) as error:
        report.failures.append(_failure(root, Path(".trash/clean-runs"), "could not reserve trash run", error))
        return report
    report.run_path = str(run_path)

    for relative in movable:
        source = root / relative
        destination = run_path / relative
        try:
            # Re-check immediately before the rename so a root output that
            # changed into a symlink after the initial scan is still refused.
            latest_mode = source.lstat().st_mode
            if stat.S_ISLNK(latest_mode):
                raise ValueError("output became a symlink before move")
            if not (stat.S_ISDIR(latest_mode) or stat.S_ISREG(latest_mode)):
                raise ValueError("output became a special file before move")
            entry_kind = "directory" if stat.S_ISDIR(latest_mode) else "file"
            _move_entry(source, destination, move_entry)
        except (OSError, ValueError) as error:
            report.failures.append(_failure(root, relative, "could not move output into trash", error))
            continue
        report.moved.append(
            {
                "original_path": str(source),
                "original_relative_path": relative.as_posix(),
                "destination_path": str(destination),
                "destination_relative_path": _relative(destination, root),
                "kind": entry_kind,
            }
        )

    payload = {
        "schema_version": 1,
        "status": "success" if not report.failures else ("partial" if report.moved else "failed"),
        "root": str(root),
        "run_path": str(run_path),
        "moved": report.moved,
        "failures": report.failures,
    }
    try:
        report.manifest_path = str(_write_manifest(run_path, payload))
    except (OSError, ValueError) as error:
        report.failures.append(_failure(root, Path(".trash/clean-runs/manifest.json"), "could not write clean manifest", error))
    return report


def _print_report(report: CleanReport, *, as_json: bool) -> None:
    if as_json:
        print(json.dumps(report.to_dict(), ensure_ascii=False, indent=2, sort_keys=True))
        return
    if not report.moved and not report.failures:
        print("clean: no known build or package outputs found")
        return
    for entry in report.moved:
        print(f"moved {entry['original_path']} -> {entry['destination_path']}")
    if report.manifest_path:
        print(f"manifest {report.manifest_path}")
    for failure in report.failures:
        print(f"could not clean {failure['path']}: {failure['reason']}", file=sys.stderr)


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    default_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--root", type=Path, default=default_root, help="repository root to clean")
    parser.add_argument("--json", action="store_true", help="print the complete machine-readable report")
    args = parser.parse_args(argv)
    try:
        report = clean(args.root)
    except (OSError, CleanConfigurationError) as error:
        print(f"clean failed: {error}", file=sys.stderr)
        return 2
    _print_report(report, as_json=args.json)
    return 0 if report.succeeded else 2


if __name__ == "__main__":
    raise SystemExit(main())
