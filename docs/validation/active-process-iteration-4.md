# Bounded active process integration validation

Date: 2026-09-07. Scope: Phase 0 iperf3 process boundary, strict result normalization and macOS loopback interoperability. This is not a completed active-survey capability or full Gate D.

The [independent Luna xhigh review](../reviews/active-luna-review.md) approved the final 18-file freeze after 20 tests and independent malformed-input/process probes. Root-authored corrections resolve numeric-host coercion, surviving descendants, UDP source-denominator semantics, zombie-only macOS groups, structured cleanup failures and server-thread acceptance propagation. No unresolved BLOCKER or MAJOR remains.

| Executed command | Result |
|---|---|
| `python3 -m unittest tests.test_active_process -q` | PASS: 20 parser/process/acceptance tests on macOS |
| `python3 research/active/acceptance.py --binary /Users/vincent/code/kyberia/.worktrees/active-proof/.tools/iperf-install/bin/iperf3 --output research/active/evidence/loopback-macos-arm64.json` | PASS: eight actual checks after explicit socket approval |
| `.tools/venv/bin/python -m unittest discover -s tests -p 'test_*.py'` | PASS: 130 tests and 18 optional skips (148 discovered) |

The actual checks cover one-stream TCP/UDP upload/download at 4 Mbps for one second, cancellation, timeout, recovery and connection refusal. Their [report](../../research/active/evidence/loopback-macos-arm64.json) preserves source binary hashes, topology, clocks and matching endpoint byte summaries. Initial direct 2 Mbps runs are separate evidence. A former false success caused by an unpropagated thread exception is retained as [rejected evidence](../../research/active/evidence/loopback-rejected-thread-error.json); it does not count as acceptance.

UDP percentage uses the sender count denominator. Receiver packet count is the highest sequence observed, while received datagrams are derived from bytes and the verified payload size. Source gap estimates omit trailing loss and can be distorted by duplicates; raw placeholders are retained separately from unknown loss/jitter evidence. Synthetic loss tests do not establish controlled-loss runtime validation.

Open: Linux waitid and other OS execution, parallel/bidirectional/QUIC/IPv6 support, authenticated LAN/remote targets, route/interface/BSSID attribution, shaped-loss and independent packet counters, CPU/memory quotas, bundling and user-facing survey integration. The adapter reports unsupported states and never turns failure into zero throughput or Wi-Fi attribution.
