# Publication CI checkpoint

Source commit: `d5050f3a6512e1a91413a0b900c2994d75ddd491`.
Evidence: [GitHub Actions run 34322869724](https://github.com/Chimp56/kyberia/actions/runs/34322869724),
queried through its public jobs API on 2026-09-09.

| Native runner | Bootstrap | Full foundation/CLI build | Complete validation |
| --- | --- | --- | --- |
| Windows | Passed | Passed | Failed; detailed cause not yet available |
| macOS | Passed | Passed | Passed |
| Linux | Passed | Passed | Passed |

The successful Windows build is native CI compilation evidence for the complete
workspace at this commit. It supersedes the earlier local MinGW limitation for
the build requirement. Windows complete validation failed with public annotation exit code 1, line
195; that code alone does not identify the failed subcommand. Native macOS
and Linux complete validation passed.

The preceding [run for 4a55d35](https://github.com/Chimp56/kyberia/actions/runs/34321706533)
passed Linux but failed the validation step on macOS and Windows. Public
annotations report only exit codes 101 and 1 respectively. Detailed log access
requires authentication; an explicit credential-use approval request is pending.
Do not infer the cause from these exit codes or treat local passing tests as
proof that those CI failures are resolved.

Retained public API responses are under `.tools/github-actions-*-jobs.json`;
those local checkpoint files may be refreshed. The immutable run links and
source commit identify the authoritative evidence. The run is terminal: macOS and Linux passed; Windows failed. The Windows
failure remains actionable once its diagnostic evidence is available.

## Validation-stage checkpoint

For source `b489ef0f55569089965dfe4bf6fae5ada462744c`,
[run 34324557956](https://github.com/Chimp56/kyberia/actions/runs/34324557956)
exposes the individual validation stages. The public jobs response on 2026-09-09
shows Windows bootstrap and full build passing, followed by failure in
“Check formatting, lint and dependency boundaries”; subsequent checks were
skipped. This narrows the failure to that group, but does not establish which
subcommand failed. Linux passed every stage. macOS subsequently passed all stages; the run is terminal.

The Windows stored-analysis request reader has a separately confirmed unsupported
production path under review. Its local tests do not prove the hosted lint
failure is fixed, and the two issues are not being conflated.

Reviewed diagnostic source `1870390`, integrated as `b937905`, adds an escaped
public annotation naming the failed developer command and exit code. It never
includes child output or environment values. Focused tests and independent
review passed; a new hosted run is required to observe the Windows diagnosis.

## Exact Windows failing command

Public annotations for Windows check `102381576107` in
[run 34325462822](https://github.com/Chimp56/kyberia/actions/runs/34325462822),
source `b937905268aa96954fe14375bda90e3727c8ad9c`, now identify:

```text
Validation command failed (exit 101): cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```

The enclosing Windows shell step reports exit 1. The inner Clippy command
and exit 101 are the useful diagnosis; the individual compiler diagnostic is
not yet present in public annotations. The request-reader correction `c3dd163`
has passed local CLI Clippy and Windows-target file-adapter Clippy, but remains
unmerged pending independent review and cannot yet be claimed to fix this run.
