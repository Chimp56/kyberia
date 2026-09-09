# Kyberia

Kyberia is being implemented as a local-first Wi-Fi survey and planning instrument. [plan.md](plan.md) is the authoritative specification. [STATUS.md](STATUS.md) and the [requirement ledger](docs/implementation/TRACEABILITY.md) report the current scope and evidence; the full application is not yet delivered.

The initial foundation includes canonical unit-safe evidence contracts, deterministic research fixtures, runtime evidence gates, and a transactional directory project store with an executable CLI. Live collectors and the desktop survey workflow are under implementation.

## Developer setup

Use `python3 tools/dev.py bootstrap` once, then `python3 tools/dev.py check` for all current checks. Individual commands include `clean`, `build`, `format`, `lint`, `typecheck`, `unit`, `integration`, `e2e`, `benchmark`, `source-check`, and `evidence-check`. `clean` moves only the documented root build and test outputs into an ignored `.trash/clean-runs/` directory and never deletes files; see the [clean command guide](docs/development/clean-command.md). `e2e` currently runs the real CLI workflow; browser/native desktop acceptance joins it with the desktop implementation. Packaging, distributable SBOM and complete application benchmarks remain delivery work.

Install Rust through [rustup](https://rustup.rs/) and Python 3.9 or newer. The repository pins Rust 1.98.1 with rustfmt and Clippy. Cargo uses the committed dependency lockfile. A C compiler is required for the bundled SQLite build; on macOS use the Xcode Command Line Tools. Node 24.20.0 and pnpm 12.3.4 are the selected forthcoming desktop toolchain.

```sh
python3 -m venv .tools/venv
.tools/venv/bin/python -m pip install --require-hashes --only-binary=:all: -r tools/requirements.txt
source .tools/venv/bin/activate
cargo fetch --locked
cargo build --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
python3 -m unittest discover -s tests -p 'test_*.py'
python3 tools/ledger.py check
python3 tools/validation/fixtures.py check
python3 tools/source_inventory.py check
```

After dependencies are cached, Cargo commands support `--offline`. Source-inventory checking includes conditional dependencies, so run `cargo fetch --locked` on a fresh machine first. Commands fail on errors and do not reinterpret absent external runtimes as passing gates.

On Windows, use `.tools\venv\Scripts\python.exe` and activate with `.tools\venv\Scripts\Activate.ps1`. Python 3.11+ uses standard-library `tomllib`; Python 3.9/3.10 uses the hash-pinned MIT-licensed Tomli parser. This dependency supports lockfile validation without a custom TOML parser.

## Project CLI

```sh
cargo run -p kyberia-cli -- new /private/tmp/home.rfatlas Home
cargo run -p kyberia-cli -- query-canonical-project /private/tmp/home.rfatlas
cargo run -p kyberia-cli -- inspect /private/tmp/home.rfatlas
cargo run -p kyberia-cli -- verify /private/tmp/home.rfatlas
```

`new` requires a path that does not already exist and registers an empty canonical baseline. The read-only query reports the verified baseline or materialized state, or explicit absence for a legacy bundle. Read the [canonical project CLI guide](docs/development/canonical-project-cli.md) and [bundle format and recovery guide](docs/architecture/project-bundle.md) before diagnosing damaged data. Keep original evidence when verification fails. No recursive cleanup is automatic; repository agent instructions require explicit user permission for recursive deletion.

On Unix, `analyze-stored-rssi` computes numerical RSSI artifacts from committed
survey evidence. See the [stored RSSI CLI guide](docs/development/stored-rssi-cli.md)
for request fields, method selection, unknown cells and cancellation outcomes.

## Engineering references

- [Architecture decisions](docs/architecture/ADR/README.md)
- [Canonical contracts](docs/architecture/domain-contracts.md)
- [Source/license inventory](docs/licenses/SOURCE_LEDGER.md)
- [Synthetic fixture methodology](docs/validation/synthetic-fixtures.md)
- [External runtime gates](docs/validation/runtime-gates.md)
- [External blockers](docs/implementation/BLOCKERS.md)

Kismet remains an external adapter integration. Sionna RT is adopted behind an isolated worker boundary. Neither external runtime is currently represented as validated by fixture-only tests.
