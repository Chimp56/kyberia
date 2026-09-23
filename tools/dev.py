#!/usr/bin/env python3
"""Portable commands for the implemented Kyberia foundation and CLI workflows."""
import argparse
import os
import secrets
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PYTHON = ROOT / ".tools/venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
PYTHON = Path(os.environ.get("KYBERIA_TOOL_PYTHON", DEFAULT_PYTHON))
TOOLS = Path(__file__).resolve().parent
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from rust_diagnostics import github_actions_enabled, parser_for


# These identifiers are part of the public CI failure contract.  Keep the
# set closed so an exception can never cause arbitrary command metadata to be
# copied into a workflow annotation.
BOOTSTRAP_STAGE_IDS = (
    "bootstrap.python-venv",
    "bootstrap.python-dependencies",
    "bootstrap.rust-toolchain",
    "bootstrap.cargo-fetch",
    "bootstrap.lab-pnpm",
)
_ACTIVE_BOOTSTRAP_STAGE = None


def _corepack_command():
    """Return the Corepack launcher that CreateProcess can start per OS."""
    # The Windows Node distribution exposes Corepack as a .cmd launcher.  A
    # bare `corepack` name is resolved by the interactive shell but is not a
    # portable executable name for subprocess.Popen(shell=False).
    return "corepack.cmd" if os.name == "nt" else "corepack"


def _run_bootstrap_stage(stage, operation):
    """Run one bootstrap operation while retaining only its safe stage ID."""
    if stage not in BOOTSTRAP_STAGE_IDS:
        raise ValueError("unknown bootstrap stage")
    global _ACTIVE_BOOTSTRAP_STAGE
    previous = _ACTIVE_BOOTSTRAP_STAGE
    _ACTIVE_BOOTSTRAP_STAGE = stage
    try:
        return operation()
    except (subprocess.CalledProcessError, OSError) as error:
        # These exceptions retain command/path details. The main handler must
        # use only this allowlisted attribute when reporting hosted failures.
        error.kyberia_stage = stage
        raise
    finally:
        _ACTIVE_BOOTSTRAP_STAGE = previous


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


def _run_streamed(command, diagnostic_kind=None, env=None, emit_child_output=True):
    parser = parser_for(diagnostic_kind, ROOT) if diagnostic_kind is not None else None
    command_guard = None
    if github_actions_enabled():
        # The guard prevents child text from becoming a workflow command or
        # an untrusted annotation. Bootstrap stages additionally suppress the
        # child stream entirely.
        if diagnostic_kind == "python-unittest":
            guard_prefix = "kyberia-python-diagnostics-"
        elif diagnostic_kind in ("cargo", "libtest"):
            guard_prefix = "kyberia-rust-diagnostics-"
        else:
            guard_prefix = "kyberia-command-output-"
        command_guard = guard_prefix + secrets.token_hex(16)
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
            env=env,
        )
        assert process.stdout is not None
        while True:
            chunk = process.stdout.read(8192)
            if not chunk:
                break
            if emit_child_output:
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


def run(*args, diagnostics=None, env=None):
    command = list(map(str, args))
    actions = github_actions_enabled()
    if actions and diagnostics == "cargo":
        command = _cargo_json_command(command)
    if actions and _ACTIVE_BOOTSTRAP_STAGE is not None:
        # Bootstrap diagnostics expose only a fixed logical stage.  In
        # particular, do not echo executable paths, arguments, or child text
        # into the hosted workflow log.
        print("+ Kyberia validation stage " + _ACTIVE_BOOTSTRAP_STAGE, flush=True)
    else:
        print("+ " + " ".join(command), flush=True)
    if actions:
        _run_streamed(
            command,
            diagnostics,
            env,
            emit_child_output=_ACTIVE_BOOTSTRAP_STAGE is None,
        )
    elif env is None:
        subprocess.run(command, cwd=ROOT, check=True)
    else:
        subprocess.run(command, cwd=ROOT, check=True, env=env)


def run_in(directory, *args):
    command = list(map(str, args))
    print("+ (cd {} && {})".format(directory, " ".join(command)), flush=True)
    subprocess.run(command, cwd=directory, check=True)


def python(*args, diagnostics=None):
    if not PYTHON.is_file():
        raise SystemExit("Run python3 tools/dev.py bootstrap to create the local tooling environment")
    run(PYTHON, *args, diagnostics=diagnostics)


def supply_chain(*args):
    python("tools/supply_chain.py", *args)


def lab_pnpm(*args):
    local_pnpm = ROOT / ".tools/pnpm/package/bin/pnpm.mjs"
    if local_pnpm.is_file():
        run("node", local_pnpm, "--dir", "tools/lab-mcp", *args)
        return
    environment = dict(os.environ)
    environment["COREPACK_HOME"] = str(ROOT / "tools/lab-mcp/.tools/corepack")
    run(
        _corepack_command(),
        "pnpm@12.3.4",
        "--dir",
        "tools/lab-mcp",
        *args,
        env=environment,
    )


def command(name):
    if name == "bootstrap":
        _run_bootstrap_stage(
            "bootstrap.python-venv",
            lambda: run(sys.executable, "-m", "venv", ROOT / ".tools/venv"),
        )
        _run_bootstrap_stage(
            "bootstrap.python-dependencies",
            lambda: python(
                "-m",
                "pip",
                "install",
                "--require-hashes",
                "--only-binary=:all:",
                "--no-cache-dir",
                "-r",
                "tools/requirements.txt",
            ),
        )
        _run_bootstrap_stage(
            "bootstrap.rust-toolchain",
            lambda: run("rustup", "show", "active-toolchain"),
        )
        _run_bootstrap_stage(
            "bootstrap.cargo-fetch",
            lambda: run("cargo", "fetch", "--locked"),
        )
        _run_bootstrap_stage(
            "bootstrap.lab-pnpm",
            lambda: lab_pnpm(
                "install", "--frozen-lockfile", "--store-dir", ".tools/pnpm-store"
            ),
        )
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
        python("-m", "unittest", "discover", "-v", "-s", "tests", "-p", "test_*.py", diagnostics="python-unittest")
    elif name == "integration":
        run("cargo", "test", "-p", "kyberia-project-store", "--locked", "--offline")
    elif name == "e2e":
        run("cargo", "test", "-p", "kyberia-cli", "--test", "project_workflow", "--locked", "--offline")
        run("cargo", "test", "-p", "kyberia-cli", "--test", "stored_analysis", "--locked", "--offline")
        run_in(ROOT / "apps/desktop", "npm", "run", "e2e")
    elif name == "test":
        run("cargo", "test", "--workspace", "--locked", "--offline", diagnostics="libtest")
        python("-m", "unittest", "discover", "-v", "-s", "tests", "-p", "test_*.py", diagnostics="python-unittest")
    elif name == "source-check":
        python("tools/source_inventory.py", "check")
    elif name == "evidence-check":
        python("tools/ledger.py", "check")
        python("tools/validation/fixtures.py", "check")
    elif name == "desktop":
        desktop = ROOT / "apps/desktop"
        run_in(desktop, "npm", "run", "typecheck")
        run_in(desktop, "npm", "run", "build")
        run_in(desktop, "npm", "test", "--", "--run")
        run_in(desktop, "npm", "run", "e2e")
        run("cargo", "fmt", "--manifest-path", desktop / "src-tauri/Cargo.toml", "--", "--check")
        run("cargo", "test", "-p", "kyberia-desktop", "--locked", "--offline")
        run("cargo", "clippy", "-p", "kyberia-desktop", "--all-targets", "--locked", "--offline", "--", "-D", "warnings", diagnostics="cargo")
        run_in(desktop, "npm", "run", "tauri:build")
        desktop_binary = ROOT / "target/release" / ("kyberia-desktop.exe" if os.name == "nt" else "kyberia-desktop")
        if not desktop_binary.is_file():
            raise SystemExit(f"Tauri build did not produce the expected desktop binary: {desktop_binary}")
        python("tools/architecture.py")
        python("tools/source_inventory.py", "check")
        python("tools/ledger.py", "check")
    elif name == "benchmark":
        run("cargo", "run", "--release", "--locked", "--offline", "-p", "kyberia-domain", "--example", "project_benchmark")
    elif name == "sbom":
        supply_chain("sbom")
    elif name == "audit":
        supply_chain("audit")
    elif name == "supply-chain-bootstrap":
        python("-m", "pip", "install", "--require-hashes", "--only-binary=:all:", "--no-cache-dir", "-r", "tools/supply-chain/requirements.txt")
        supply_chain("bootstrap")
    elif name == "validate-antenna-schema":
        python("tools/validate_antenna_schema.py")
    elif name == "supply-chain-refresh":
        supply_chain("refresh-advisories")
    elif name == "lab-mcp-bootstrap":
        lab_pnpm("install", "--frozen-lockfile", "--store-dir", ".tools/pnpm-store")
    elif name == "lab-mcp-build":
        lab_pnpm("run", "build")
    elif name == "lab-mcp-check":
        lab_pnpm("run", "check")
        run(sys.executable, "-m", "unittest", "-v", "tools/lab-mcp/test/test_windows_process.py")
    elif name == "check":
        for step in ["lint", "typecheck", "test", "lab-mcp-check", "source-check", "evidence-check", "desktop"]:
            command(step)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["bootstrap", "clean", "build", "format", "lint", "typecheck", "unit", "integration", "e2e", "test", "source-check", "evidence-check", "desktop", "benchmark", "sbom", "audit", "supply-chain-bootstrap", "supply-chain-refresh", "validate-antenna-schema", "lab-mcp-bootstrap", "lab-mcp-build", "lab-mcp-check", "check"])
    args = parser.parse_args()
    try:
        command(args.command)
    except (subprocess.CalledProcessError, OSError) as error:
        if github_actions_enabled():
            # Only identify repository-defined developer commands. Never emit
            # child output or environment values into public annotations.
            returncode = (
                error.returncode
                if isinstance(error, subprocess.CalledProcessError)
                and isinstance(error.returncode, int)
                and not isinstance(error.returncode, bool)
                else 1
            )
            stage = getattr(error, "kyberia_stage", None)
            if stage not in BOOTSTRAP_STAGE_IDS:
                stage = None
            stage_text = f" at stage {stage}" if stage is not None else ""
            message = f"Validation command failed{stage_text} (exit {returncode})"
            message = message.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
            print("::error title=Validation command failed::" + message, flush=True)
            raise SystemExit(returncode)
        if isinstance(error, subprocess.CalledProcessError):
            raise SystemExit(error.returncode)
        raise


if __name__ == "__main__":
    main()
