# Project-store SQLite sidecar budget review

Review date: 2026-09-07

Reviewer: `/root/pcap_review_luna`

Disposition: **APPROVED**

## Scope

The reviewer independently inspected the uncommitted `fix/store-sidecars` diff against its branch base and reran the focused schema suite, full project-store suite, workspace regressions, formatting, Clippy with warnings denied, and diff checks. The review covered combined resource accounting, WAL/journal/SHM file types, recovery behavior, overflow, exact boundaries, error classification, and the documented filesystem-concurrency limit.

## Findings and resolution

- BLOCKER: none.
- MAJOR: none.
- MINOR, resolved: the first revision lacked a regression where the main database and a sidecar were each below 64 MiB but exceeded the budget together. The final private regression accepts exactly 32 MiB + 32 MiB, rejects 32 MiB + 1 byte + 32 MiB, checks the exact error, and covers checked-addition overflow.
- MINOR, resolved: the first bounded-WAL regression restored the main database's original state, so it did not prove WAL consumption. The final regression writes a valid WAL-only manifest with different revision, update time, and body, then proves read-only `Bundle::open` returns that exact state.
- MINOR, resolved: writable rollback behavior was not exercised. The final regression proves a bounded non-hot rollback journal permits a read-write open.
- NIT, retained: metadata checks are subject to filesystem TOCTOU if another local process changes a bundle concurrently. The adapter explicitly documents a non-concurrent-filesystem boundary; descriptor-relative confinement remains separate work.

## Verdict

`STORE-ITER4-001` is resolved for closed bundles within the documented boundary. The implementation counts the main database, WAL, rollback journal, and shared-memory sidecar against one limit; rejects symlinks and nonregular sidecars; checks arithmetic overflow; and rechecks at public operation boundaries. It rejects the independently demonstrated 86.5 MiB valid-WAL bypass while preserving bounded WAL and rollback behavior consistent with SQLite's documented semantics.

The final focused suite passes 12 integration tests plus the private exact-boundary/overflow regression. The reviewer reported no unresolved BLOCKER, MAJOR, or MINOR findings.
