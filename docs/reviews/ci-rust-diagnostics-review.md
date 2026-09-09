# CI Rust diagnostics independent review

Reviewer: root, independent of author Russell. Sources `d6a5532` + `b501934`;
integrations `2452791` + `c9b4bee`. Disposition: APPROVED for bounded diagnostic
reporting; hosted failure diagnosis remains open until a new run executes.

The parser emits bounded compiler codes and repository-relative Rust locations,
or grammar-restricted test identifiers. Compiler messages, captured failure
bodies and environment values are excluded from annotations. Child console
output is retained under a random Actions stop-commands guard. Local developer
commands retain their original subprocess behavior and arguments.

## Findings resolved

MAJOR: the original libtest parser resumed after a captured failure body emitted
`running 1 test`, allowing its subsequent `test captured::private_identifier ...
FAILED` line into annotations. Root reproduced the exact issue after an actual
`failures:` marker. Correction `b501934` permanently stops parsing test names at
the first failure section. The original reproduction now returns only the real
harness failure identifier. Subsequent test-binary failures are intentionally
not annotated; this is a visible stable-libtest format limitation.

The streamed command wrapper also needed to kill/reap its child when output
handling raises. Root independently exercised the correction with a real Python
child writing 8,192 bytes then sleeping for 30 seconds, and an injected output
sink exception. The child was killed and reaped (return code -9), the original
exception survived, and the matching Actions command guard was restored.

## Validation

Root ran 13 diagnostic tests plus two developer-command tests successfully and
repeated the captured-output reproduction independently. After integration,
`python -m unittest discover -s tests -p 'test_*.py'` ran 202 tests with 19 skipped
and no failures. `GITHUB_ACTIONS=true python tools/dev.py typecheck` passed with
real streamed Cargo JSON output. Source inventory (241 locked packages),
traceability and diff checks passed. Integration logs are
`.tools/ci-diagnostics-integrated-python.log` and
`.tools/ci-diagnostics-integrated-typecheck.log`.

No credential lookup or authenticated log access was used. The helper reports
locations/identifiers, not the full explanation behind a compiler error or test
panic. It cannot authenticate arbitrary native writes that bypass libtest's
capture mechanism; the current default captured test workflow is the supported
boundary. Existing Rust runtime checks and their exit statuses remain required.
