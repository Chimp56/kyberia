# Repository clean command

`python3 tools/dev.py clean` moves known root-level build and test outputs into
an ignored, uniquely named directory below `.trash/clean-runs/`. It reports
each source and destination and writes `manifest.json` after the moves. The
manifest records the repository root, every original relative and absolute
path, every destination, the entry kind, and any failure. A partial run keeps
the successful moves and records the failed entries so the user can recover
them manually.

The command's complete, non-configurable allowlist is:

- `target/` (Cargo build output)
- `dist/` (packaging and frontend output)
- `coverage/` (coverage output)
- `test-results/` (test runner output)
- `playwright-report/` (browser test output)

The command preserves `.tools/`, `.venv/`, `node_modules/`, `.pnpm-store/`,
`.worktrees/`, `.git/`, research data, and every other path. Unknown paths are
never inferred to be disposable. A root-level allowlisted symlink is refused
and its target is neither inspected nor moved. The move is a same-filesystem
rename, so the cleaner does not walk an output tree or dereference links inside
it. The cleaner reserves each run directory with exclusive creation and
refuses any pre-existing run or destination it observes; it does not claim to
coordinate with an unrelated process that mutates the filesystem concurrently.

The operation does not attempt to discover processes holding files open. Stop
active builds, test runners, and package managers before cleaning; a process
that still writes after a rename can recreate an output path. No automatic
trash emptying is provided. Users manually inspect and remove old trash only
under their own repository policy.

For automation, `python3 tools/trash_clean.py --json` emits the complete report
and returns zero only when every discovered entry was moved or no known output
was present. Symlink refusals, move errors, trash reservation errors, and
manifest errors return status 2 with the report preserved where possible.
