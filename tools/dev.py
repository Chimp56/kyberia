#!/usr/bin/env python3
"""Portable commands for the implemented Kyberia foundation and CLI workflows."""
import argparse
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PYTHON = ROOT / ".tools/venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def run(*args):
    print("+ " + " ".join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=ROOT, check=True)


def python(*args):
    if not PYTHON.is_file():
        raise SystemExit("Run python3 tools/dev.py bootstrap to create the local tooling environment")
    run(PYTHON, *args)


def command(name):
    if name == "bootstrap":
        run(sys.executable, "-m", "venv", ROOT / ".tools/venv")
        python("-m", "pip", "install", "--require-hashes", "--only-binary=:all:", "--no-cache-dir", "-r", "tools/requirements.txt")
        run("rustup", "show", "active-toolchain")
        run("cargo", "fetch", "--locked")
    elif name == "build":
        run("cargo", "build", "--workspace", "--locked", "--offline")
    elif name == "format":
        run("cargo", "fmt", "--all")
    elif name == "lint":
        run("cargo", "fmt", "--all", "--check")
        run("cargo", "clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings")
        python("tools/architecture.py")
    elif name == "typecheck":
        run("cargo", "check", "--workspace", "--all-targets", "--locked", "--offline")
    elif name == "unit":
        run("cargo", "test", "-p", "kyberia-domain", "--locked", "--offline")
        python("-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py")
    elif name == "integration":
        run("cargo", "test", "-p", "kyberia-project-store", "--locked", "--offline")
    elif name == "e2e":
        # Executable CLI workflows. Desktop/browser E2E joins this command when
        # the application exists; no current GUI acceptance is implied.
        run("cargo", "test", "-p", "kyberia-cli", "--test", "project_workflow", "--locked", "--offline")
    elif name == "test":
        run("cargo", "test", "--workspace", "--locked", "--offline")
        python("-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py")
    elif name == "source-check":
        python("tools/source_inventory.py", "check")
    elif name == "benchmark":
        run("cargo", "run", "--release", "--locked", "--offline", "-p", "kyberia-domain", "--example", "project_benchmark")
    elif name == "check":
        for step in ["lint", "typecheck", "test", "source-check"]:
            command(step)
        python("tools/ledger.py", "check")
        python("tools/validation/fixtures.py", "check")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["bootstrap", "build", "format", "lint", "typecheck", "unit", "integration", "e2e", "test", "source-check", "benchmark", "check"])
    args = parser.parse_args()
    try:
        command(args.command)
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode)


if __name__ == "__main__":
    main()
