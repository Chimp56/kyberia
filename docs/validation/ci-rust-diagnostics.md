# CI Rust diagnostics

The developer validation commands retain the complete child process stream in
the normal GitHub Actions log while emitting a small, safe set of workflow
annotations. This makes a failure actionable from the check summary without
copying arbitrary compiler text, test output, command arguments, environment
values, or credentials into a public annotation.

Cargo check and Clippy runs use Cargo's JSON message stream only when
`GITHUB_ACTIONS=true`. The parser accepts a grammar-validated Rust error/lint
code and one primary span whose path is repository-relative, whose extension
is `.rs`, and whose line and column are bounded. It emits at most 20
diagnostics. Codes, paths, test identifiers, and numeric locations have
explicit bounds. Input lines are capped at 64 KiB, and oversized or additional
diagnostics are summarized by count. The child stream is placed between a
GitHub `stop-commands` guard and its matching re-enable token so a child line
such as `::error ...` cannot become a workflow command; the bytes remain in
the normal log.

The pinned stable toolchain exposes libtest's `--format json` spelling but
rejects it at runtime because that format still requires nightly
`-Z unstable-options`. Test validation therefore parses only the stable
harness status lines (`running`, `test ... ... FAILED`, and `test result:`).
It accepts bounded, grammar-validated test identifiers and stops parsing at
the `failures:` section, where captured test output is untrusted. This gives
failed test names without claiming authenticated provenance for arbitrary
test output. A future toolchain migration may replace this parser with a
stable machine-readable harness format after the version and output contract
are tested.

If a developer command fails outside one of those structured streams, the
fallback annotation contains only the numeric exit status. The original
`CalledProcessError` status is still returned by `tools/dev.py`; local runs
continue to use the existing `subprocess.run` path and do not add JSON flags
or stream guards.

Validation evidence for this increment:

```text
PYTHONPYCACHEPREFIX=.trash/test-runs/ci-diagnostics/pycache \
  .tools/venv/bin/python -m unittest \
  tests.test_ci_rust_diagnostics tests.test_dev_commands
14 tests passed
```

The parser tests cover split input chunks, malformed and oversized lines,
annotation truncation, path traversal and Windows path normalization, hostile
compiler codes, captured-output annotation injection, raw stream retention,
exit-status preservation, and the unchanged local execution path. Full
hosted Windows validation remains dependent on the runner's native MinGW
SQLite toolchain; that environment requirement is separate from this
diagnostic formatter.
