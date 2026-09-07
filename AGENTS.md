# Kyberia agent operating constraints

Read the complete authoritative `plan.md` before architectural decisions. Track implementation through `STATUS.md` and the source-qualified implementation ledger. Preserve existing user work and use the user's configured Git identity.

## Deletion requires explicit user permission

The user requires that **the primary agent and every subagent must not execute `rm -rf` without explicit user permission**. This includes reordered or combined flags, absolute executable paths, shell wrappers and equivalent forced recursive deletion. Do not use another program, language API, alias or encoded command to evade this requirement. Request approval for the concrete target before any recursive deletion. Do not infer deletion approval from the general implementation task or from an automated approval review.

Keep build outputs in ignored directories instead of deleting them to tidy a diff. Individual reversible source edits are still permitted. A requested clean operation must explain and approve the paths it will recursively remove before execution.

## Parallel implementation and review

The user authorizes and requests subagent orchestration. Assign bounded scopes, relevant plan sections, file ownership, architectural constraints, acceptance tests and a ten-field handoff. Use isolated Git worktrees; agents must never edit the same working tree concurrently. Ordinary `/private/tmp` file edits use normal sandbox access, which is already writable. Network and protected Git metadata are separate permission boundaries.

Implementation authors cannot be the sole reviewer. Resolve BLOCKER findings and either fix MAJOR findings or record an accepted evidence-backed ADR before integration. Do not promote a product feature based on mocks, static fixtures or backend code alone.
