#!/usr/bin/env python3
"""Portable commands for the implemented Kyberia foundation and CLI workflows."""
import argparse
import os
import secrets
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PYTHON = ROOT / ".tools/venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
TOOLS = Path(__file__).resolve().parent
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from rust_diagnostics import github_actions_enabled, parser_for


def _cargo_json_command(command):
    """Add Cargo's bounded machine-readable diagnostics before `--` args."""
    if any(str(value).startswith("--message-format") for value in command):
        return list(command)
    command = list(command)
    try:
        separator = command.index("--")
    except ValueError:
        separator = len(command)
    command.insert(separator, "--message-format=json")
    return command


def _write_raw(chunk):
    output = getattr(sys.stdout, "buffer", None)
    if output is not None:
        output.write(chunk)
        output.flush()
    else:
        sys.stdout.write(chunk.decode("utf-8", "replace"))
        sys.stdout.flush()


def _run_streamed(command, diagnostic_kind=None):
    parser = parser_for(diagnostic_kind, ROOT) if diagnostic_kind is not None else None
    command_guard = None
    if github_actions_enabled():
        # Child output is still retained verbatim in the Actions log, but the
        # guard prevents a compiler/test message from becoming a workflow
        # command or an untrusted annotation.
        command_guard = "kyberia-rust-diagnostics-" + secrets.token_hex(16)
        print("::stop-commands::" + command_guard, flush=True)
    process = None
    returncode = None
    last_byte = ord("\n")
    try:
        process = subprocess.Popen(
            command,
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        assert process.stdout is not None
        while True:
            chunk = process.stdout.read(8192)
            if not chunk:
                break
            _write_raw(chunk)
            if parser is not None:
                parser.feed(chunk)
            last_byte = chunk[-1]
        returncode = process.wait()
    except BaseException:
        if process is not None:
            try:
                process.kill()
            except BaseException:
                pass
            try:
                process.wait()
            except BaseException:
                pass
        raise
    finally:
        if parser is not None:
            parser.finish()
        if command_guard is not None:
            if last_byte != ord("\n"):
                print(flush=True)
            print("::" + command_guard + "::", flush=True)
    if parser is not None and github_actions_enabled():
        parser.emit()
    if returncode:
        raise subprocess.CalledProcessError(returncode, command)


def run(*args, diagnostics=None):
    command = list(map(str, args))
    actions = github_actions_enabled()
    if actions and diagnostics == "cargo":
        command = _cargo_json_command(command)
    print("+ " + " ".join(command), flush=True)
    if actions:
        _run_streamed(command, diagnostics)
    else:
        subprocess.run(command, cwd=ROOT, check=True)


def python(*args):
    if not PYTHON.is_file():
        raise SystemExit("Run python3 tools/dev.py bootstrap to create the local tooling environment")
    run(PYTHON, *args)


def supply_chain(*args):
    python("tools/supply_chain.py", *args)


def command(name):
    if name == "bootstrap":
        run(sys.executable, "-m", "venv", ROOT / ".tools/venv")
        python("-m", "pip", "install", "--require-hashes", "--only-binary=:all:", "--no-cache-dir", "-r", "tools/requirements.txt")
        run("rustup", "show", "active-toolchain")
        run("cargo", "fetch", "--locked")
    elif name == "clean":
        # The clean helper is stdlib-only and deliberately runs with the
        # invoking interpreter, so it remains available before bootstrap.
        run(sys.executable, ROOT / "tools/trash_clean.py", "--root", ROOT, "--json")
    elif name == "build":
        run("cargo", "build", "--workspace", "--locked", "--offline")
    elif name == "format":
        run("cargo", "fmt", "--all")
    elif name == "lint":
        run("cargo", "fmt", "--all", "--check")
        run("cargo", "clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings", diagnostics="cargo")
        python("tools/architecture.py")
    elif name == "typecheck":
        run("cargo", "check", "--workspace", "--all-targets", "--locked", "--offline", diagnostics="cargo")
    elif name == "unit":
        run("cargo", "test", "-p", "kyberia-domain", "--locked", "--offline")
        python("-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py")
    elif name == "integration":
        run("cargo", "test", "-p", "kyberia-project-store", "--locked", "--offline")
    elif name == "e2e":
        # Executable CLI workflows. Desktop/browser E2E joins this command when
        # the application exists; no current GUI acceptance is implied.
        run("cargo", "test", "-p", "kyberia-cli", "--test", "project_workflow", "--locked", "--offline")
        run("cargo", "test", "-p", "kyberia-cli", "--test", "stored_analysis", "--locked", "--offline")
    elif name == "test":
        run("cargo", "test", "--workspace", "--locked", "--offline", diagnostics="libtest")
        python("-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py")
    elif name == "source-check":
        python("tools/source_inventory.py", "check")
    elif name == "evidence-check":
        python("tools/ledger.py", "check")
        python("tools/validation/fixtures.py", "check")
    elif name == "benchmark":
        run("cargo", "run", "--release", "--locked", "--offline", "-p", "kyberia-domain", "--example", "project_benchmark")
    elif name == "sbom":
        supply_chain("sbom")
    elif name == "audit":
        supply_chain("audit")
    elif name == "supply-chain-bootstrap":
        python("-m", "pip", "install", "--require-hashes", "--only-binary=:all:", "--no-cache-dir", "-r", "tools/supply-chain/requirements.txt")
        supply_chain("bootstrap")
    elif name == "supply-chain-refresh":
        supply_chain("refresh-advisories")
    elif name == "check":
        for step in ["lint", "typecheck", "test", "source-check", "evidence-check"]:
            command(step)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["bootstrap", "clean", "build", "format", "lint", "typecheck", "unit", "integration", "e2e", "test", "source-check", "evidence-check", "benchmark", "sbom", "audit", "supply-chain-bootstrap", "supply-chain-refresh", "check"])
    args = parser.parse_args()
    try:
        command(args.command)
    except subprocess.CalledProcessError as error:
        if github_actions_enabled():
            # Only identify repository-defined developer commands. Never emit
            # child output or environment values into public annotations.
            returncode = error.returncode if isinstance(error.returncode, int) and not isinstance(error.returncode, bool) else 1
            message = f"Validation command failed (exit {returncode})"
            message = message.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
            print("::error title=Validation command failed::" + message, flush=True)
        raise SystemExit(error.returncode)


if __name__ == "__main__":
    main()
