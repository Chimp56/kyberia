# Independent Luna review: final active process proof

Review date: 2026-09-07

## Scope and freeze verification

I reviewed the 18 files in `/Users/vincent/code/kyberia/.worktrees/active-process-fix/.tools/active-final-freeze.json`. I verified every source-tree hash before copying the files into this review worktree, then verified every copied hash against both the manifest and the source tree. The author worktree and parent fix worktree were not modified. The exact final SHA-256 inventory is:

| File | SHA-256 |
| --- | --- |
| `docs/adapters/active-process.md` | `bafb54ddd55d235fe70446f1fca92dce8735e07eb3973aec825f8568b7f7d394` |
| `docs/architecture/ADR/0007-active-process.md` | `1fd3e92ffff3bbe93275a7028eb0c71e0bd56873260f456582d2925fbfa0172d` |
| `docs/licenses/active-research-sources.json` | `ff9aa576382117787b39f00aab5d62acd3066223a9bc4c024c390cd433051214` |
| `research/active/acceptance.py` | `8ebdd00b67b016e3a8ba6187fb7ed2b65109329533320931cfc0b262fc2a74ad` |
| `research/active/cli.py` | `9d03e656688f1da395c5fa3fdad7fc18356f0bda968320c2914aaefe9e4d8592` |
| `research/active/contract.py` | `d24a93d35dc2e8e461616aef5140d22734b34ff88a6256f5177088e5ca29a9d4` |
| `research/active/evidence/initial-loopback-macos-arm64.json` | `d7a26139e1e7ee59504f17c6ee408538c1b415b34b516ce402c245679c483202` |
| `research/active/evidence/loopback-macos-arm64.json` | `fd197ae63d2b530f3703f99035363bf86dea0c4117020d66cf6c6939c3ebe1d9` |
| `research/active/evidence/loopback-rejected-thread-error.json` | `5cddc3644c2e56754bcd2df79d083fc182e67da00081c2fad91cd4d5111f40db` |
| `research/active/fixtures/README.md` | `dc03440f8955aa735615775914115feaebe7663f5507dd389556da65317eafb0` |
| `research/active/fixtures/tcp-download.json` | `f71621c7457a52e9de9b577bf24e7f583f675600988aec1a330d656070792e79` |
| `research/active/fixtures/tcp-upload.json` | `a9ba0157cb8073c659c40e883bd4330c5200d68f18eeaf18158b5a1a3d63a51d` |
| `research/active/fixtures/udp-download.json` | `8a22e3b6bf5c83783ac7217b36f6686479b01b89410fbe32e501f04c777bb727` |
| `research/active/fixtures/udp-upload.json` | `19353fa17a33676290441b000b790a31e55022e22241eb1400d2bd543bf5375c` |
| `research/active/iperf-3.20-LICENSE` | `7c9ba0385fcf35c4c9e08ad0c0cba72d4315f36f0f742fef4de826a28cc33a97` |
| `research/active/process.py` | `e26256d80e8997a5f1e48df60b83f70b90b8be39c3355a1d00e55a9665909bb5` |
| `research/active/request.json` | `23bca10f04f1aecbba51f50e43c9c09fb0ade0ed73624b58dbeef1eac95e2df8` |
| `tests/test_active_process.py` | `1eac27b341ba6fe2e3968c1e37616bb66d0acfb333bb99a962a2fe5e0d14de78` |

## Tests and independent probes

The final copied tree passes all 20 tests:

```text
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -p 'test_active_process.py' -v
Ran 20 tests in 2.007s
OK
```

I also ran independent probes against the copied final files. They passed strict duplicate-key rejection, integer `local_host` and `remote_host` rejection, ordinary successful child completion, cleanup of a real forked descendant that closed standard streams, zombie-only `EPERM` handling, and fail-closed persistent `EPERM` handling. The cleanup probe confirmed that the post-reap path makes only a signal-zero probe and never sends a second destructive signal.

The retained final wrapper report records eight actual macOS ARM64 loopback checks passing: TCP and UDP upload/download, cancellation, timeout, recovery and connection refusal. Every final result records the current executable hash `251a081d992ee0e9d69d6f585b332633b49feb8ed502f7febb2ad59d9812df12` and no cleanup error. The earlier apparent eight-check success is explicitly marked `FAILED` in `loopback-rejected-thread-error.json` because a server-thread cleanup exception was not propagated by the old harness. Linux `waitid(WNOWAIT)` and other POSIX paths were not executed in this review.

## Correctness and security assessment

The UDP correction now matches the pinned iperf3 3.20 source. `src/iperf_api.c:4167-4175` selects the sender packet count as the loss denominator when present, while `src/iperf_api.c:4430-4444` emits sender and receiver summaries separately. For a single stream, receiver `packets` is the highest sequence number, not the number of datagrams received. `src/iperf_udp.c:92-97` counts received bytes, including duplicates; `src/iperf_udp.c:125-170` accounts for forward gaps and duplicate or reordered arrivals. The parser now exposes these roles separately, derives `received_datagrams` only from verified 1,200-byte payloads, validates sender bytes and placeholders, and labels loss as `iperf_sequence_gap_estimate`. The synthetic 210-sent/highest-200/10-gap/190-arrival case, duplicate arrivals, zero/one-arrival timing, trailing-loss limits and contradictory counters are covered. No controlled-loss runtime claim is made.

The process fix preserves non-reaping exit observation, kills the owned process group before reaping, and handles macOS zombie-only `EPERM` by reaping the owned direct child and requiring a non-mutating `killpg(..., 0)` `ESRCH`. Persistent permission or liveness ambiguity remains a structured `process_error`; the post-reap path avoids PID/PGID reuse by sending no further signal. Pipe closure, output limits, timeout, cancellation, nonzero exits and surviving descendants are covered by tests. Acceptance propagates both server-thread exceptions and structured cleanup failures before recording success.

Request execution is limited to trusted local argv, literal IP targets, explicit topology and authorization, and owned IPv4 loopback at runtime. Non-loopback, IPv6, QUIC, bidirectional and parallel modes return unsupported before process creation. Duplicate JSON keys, non-finite values, structural limits, forged endpoints and integer host coercion are rejected. Failed processes and malformed evidence cannot become zero throughput. The source version, request mode, endpoint, payload size, summary windows, rates and counters are checked before measurement normalization.

## Findings

**BLOCKER:** none.

**MAJOR:** none. The prior UDP denominator/semantics finding and macOS zombie-only cleanup finding are resolved in the final hashes and covered by the final tests and probes.

**MINOR:** the Linux `waitid(WNOWAIT)` path and other POSIX implementations still require execution on those platforms before making a cross-platform runtime claim. This is documented in the adapter procedure and does not weaken the observed macOS result.

**NIT:** the ADR remains `Proposed` pending independent review and release notices still require a later bundling/SBOM review. The evidence correctly keeps the initial binary hash distinct from the current rebuilt binary hash. Runtime hashing assumes the trusted operator-configured executable path is not replaced by another local actor during execution; immutable executable-file pinning is outside this adapter.

## Limits and verdict

The final evidence is a bounded macOS loopback proof. It establishes no RF or Wi-Fi attribution, LAN/Internet behavior, authenticated active agent, shaped-loss behavior, independent packet capture, multistream/bidirectional/QUIC/IPv6 support, hard CPU/memory quota or product Gate D completion. The source/license inventory retains the full iperf3 and bundled-component notices and explicitly keeps release gates open.

**Approved for integration of this frozen research proof:** no unresolved BLOCKER or MAJOR findings remain. Approval is limited to the documented loopback scope and macOS-observed lifecycle path; it does not promote the unsupported modes or claim final product capability.
