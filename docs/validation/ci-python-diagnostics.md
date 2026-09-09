# CI Python unittest diagnostics

`tools/dev.py unit` and `tools/dev.py test` run the repository Python suite in
verbose mode and attach the bounded `python-unittest` parser when
`GITHUB_ACTIONS=true`. The parser emits at most 20 test annotations plus one truncation notice. Each
test annotation contains
only a grammar-validated dotted unittest identifier. It accepts the standard
`method (module.Class)` form and the already-qualified
`method (module.Class.method)` form without appending the method twice.
Identifiers are limited to 256 UTF-8 bytes and ASCII Python identifier
components; malformed, non-ASCII, duplicate, and oversized identifiers are
ignored or counted as omitted.

The parser accepts only `FAIL` and `ERROR` status lines in the verbose runner's
status phase. Some `unittest.TextTestRunner` versions put the first test
docstring line between the test header and its status; the parser consumes only
the bounded final ` ... FAIL` or ` ... ERROR` suffix and discards that
description. At the first failure-detail separator it stops permanently. This
keeps traceback text, exception messages, subtest parameters, and subsequent
captured output out of annotations. A standard text runner does not provide a
trustworthy single-line status for subtest detail records, so subtest failures
are deliberately omitted from annotations. It does not claim that Python's
text runner is an authenticated machine-readable protocol; a future runner
with a stable structured result stream can replace this bounded adapter.

The complete child stream remains in the normal Actions log for local
debugging. It is enclosed by a random `stop-commands` guard so child output
cannot execute workflow commands. The public annotations contain no child
output, environment values, command arguments, or exception text. Local runs
continue to use the ordinary `subprocess.run` path.

Focused validation:

```text
PYTHONPYCACHEPREFIX=.trash/test-runs/python-ci-diagnostics-20260909/pycache \
  .tools/venv/bin/python -m unittest -v \
  tests.test_ci_python_diagnostics tests.test_ci_rust_diagnostics \
  tests.test_dev_commands
25 tests passed
```

The focused tests cover chunk-split input, strict identifier grammar, malformed
and oversized records, duplicate and annotation-count bounds, failure/error
status handling from an actual `TextTestRunner`, parenthesized identifier
compatibility, deliberate subtest/detail exclusion, guarded raw output, exit
status preservation, and developer-command wiring. Hosted Windows execution
of the full workflow remains a separate runtime validation gate.
