# Kyberia agent operating constraints

Read the complete authoritative `plan.md` before architectural decisions. Track implementation through `STATUS.md` and the source-qualified implementation ledger. Preserve existing user work and use the user's configured Git identity.

## Deletion requires explicit user permission

The user requires that **the primary agent and every subagent must not execute `rm -rf` without explicit user permission**. This includes reordered or combined flags, absolute executable paths, shell wrappers and equivalent forced recursive deletion. Do not use another program, language API, alias or encoded command to evade this requirement. Request approval for the concrete target before any recursive deletion. Do not infer deletion approval from the general implementation task or from an automated approval review.

Keep build outputs in ignored directories instead of deleting them to tidy a diff. Individual reversible source edits are still permitted. A requested clean operation must explain and approve the paths it will recursively remove before execution.

## Trash instead of deletion

Move files or directories that need removal into the assigned worktree's
ignored `.trash/` directory instead of deleting them. The user will manually
empty this bin. Use a unique destination, preserve the original name/path in
a note when it is not obvious, and never overwrite a prior trash entry.
Never automatically empty the bin or treat moving an unrelated tracked
change there as permission to discard user work. The Git approval policy
below remains in force.

Test fixtures must not recursively clean up directories on scope exit or
failure. Create retained test directories under `.trash/test-runs/` and
disable automatic directory cleanup immediately (for example, Rust
`TempDir::keep()` or Python `mkdtemp` without recursive teardown). Existing
retained directories may be moved into the bin when their exact paths and
ownership are known; do not collect unrelated system temporary directories.

## Git approval policy

Routine Git inspection, staging, new commits and reviewed cherry-picks are authorized without repeated permission. The primary agent and every subagent must obtain the user's explicit permission before `git reset`, `git clean`, working-tree discard or overwrite, history rewriting (including amend and rebase), branch/tag deletion or forced replacement, worktree removal/pruning, destructive maintenance or force-push. Publishing requires task authorization. Apply this policy regardless of flag order, aliases, global options, executable paths, wrappers or existing broad allow rules. Never evade approval. Use explicit command working directories and ordinary Git subcommands for routine work. Prefix rules cannot classify every argument combination; assess actual effects before executing.

## Parallel implementation and review

The user authorizes and requests subagent orchestration. Assign bounded scopes, relevant plan sections, file ownership, architectural constraints, acceptance tests and a ten-field handoff. Use isolated Git worktrees; agents must never edit the same working tree concurrently. Ordinary `/private/tmp` file edits use normal sandbox access, which is already writable. Network and protected Git metadata are separate permission boundaries.

Subagents must use absolute paths inside their assigned worktree for every patch target. A shell command’s `workdir` does not change the patch tool’s base directory. Set `workdir` explicitly for every shell command and keep package-manager stores and generated outputs inside the assigned worktree. Check both worktree and integration Git status after initial edits; stop and report any routing mistake before continuing.

Implementation authors cannot be the sole reviewer. Resolve BLOCKER findings and either fix MAJOR findings or record an accepted evidence-backed ADR before integration. Do not promote a product feature based on mocks, static fixtures or backend code alone.
