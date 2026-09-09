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
