# Developer tooling validation

Executed on macOS ARM64 with the pinned Rust 1.98.1 and Python 3.9.6 local venv:

- `python3 tools/dev.py bootstrap`: PASS using the existing local environment and cached locked packages. This verifies repeat invocation, not a fresh-machine installation.
- `python3 tools/dev.py build`: PASS.
- `python3 tools/dev.py e2e`: PASS, two real executable CLI workflows.
- `python3 tools/dev.py clean`: PASS in retained isolated fixtures; the
  allowlisted-output, protected-path, collision, symlink, partial-failure and
  repeat-run cases are covered by `tests/test_trash_clean.py`. The command was
  not run against this checkout's actual build outputs.
- `python3 tools/dev.py check`: PASS, formatting, all-target Clippy and type checking, 47 Rust tests, seven compile-fail doctests, 75 Python tests, dependency boundaries, 86-package locked source inventory, complete source ledger and fixture generation parity.
- `python3 tools/dev.py benchmark`: PASS; deterministic metadata workload reaches the requested final revision. Integrated release profile results: 100 operations 1.612 ms, 1,000 operations 46.420 ms, 10,000 operations 2,204.728 ms. These are one-run observations on a concurrently used developer machine, not stable performance thresholds. The current clone/validation strategy exhibits quadratic total replay work.
- `git -c core.autocrlf=true check-attr text eol -- plan.md Cargo.lock tools/architecture.py .github/workflows/ci.yml`: PASS, each path uses automatic text detection with LF checkout.

Independent review verifies stop-on-first-failure behavior and child exit-code propagation. [developer-tooling-review.md](../reviews/developer-tooling-review.md) binds review to exact implementation hashes. Hosted CI, actual Linux/Windows execution, fresh bootstrap, packaging and binary SBOM remain open gates.
