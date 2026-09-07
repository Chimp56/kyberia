# ADR-0007: Use an isolated iperf3 process for throughput research

Status: Accepted for the bounded research process boundary after independent correction review. Corrected macOS wrapper runtime acceptance passed eight checks. Full Gate D and ACT/ACTB product capabilities remain open.

## Context

Plan ACT-002, §15.5, §16.8 and Gate D call for mature iperf3 interoperability, explicit topology and safe measurement limits. Throughput to an endpoint does not establish Wi-Fi capacity. Kyberia must own direction, endpoint attribution, units, provenance and failure semantics.

## Decision

Use an installed, version-detected ESnet iperf3 process behind a Kyberia-owned research adapter. Pin the proof to the official 3.20 source archive and bundled cJSON 1.7.15; do not treat other package versions as verified. The executable path is trusted local configuration. Requests contain no command fragments or arbitrary arguments, and processes are launched without a shell.

The bounded research contract is separately versioned and does not modify the canonical domain. It admits explicit literal targets, topology labels, requested rates, durations, stream counts, protocol, direction and authorization. Execution currently allows only owned IPv4 loopback, single-stream TCP or UDP upload/download. LAN/remote execution requires a future authenticated-agent implementation; bidirectional, QUIC, IPv6 and parallel modes return unsupported. Request validation preserves these unsupported choices without emitting traffic.

Use one client process per test, finite deadlines, bounded output and process-group cancellation/reaping. A failed, absent, cancelled or malformed result has no throughput measurement. Validate source version, requested mode, connected endpoints, summary windows, rates and counters before returning normalized values. Retain sender and receiver summaries independently. TCP's native `sender` flag alone is insufficient for endpoint attribution. UDP sender loss/jitter zero placeholders become unknown; receiver statistics retain source units and meaning.

## Alternatives

| Approach | Assessment |
| --- | --- |
| Installed/bundled process | Matches Gate D's initial recommendation, exercises the mature CLI and JSON, and isolates native process failure. Startup cost, deployment and version compatibility still need broader measurements. |
| `libiperf` FFI | Might reduce startup overhead but couples native lifetime, global state, threading and failure to the caller. No FFI integration or comparative performance claim is made by this proof. |
| Custom throughput protocol | Adds interoperability and scientific-validation work without evidence that replacing iperf3 is warranted. No competing throughput protocol is implemented. |

A future lightweight continuous-survey probe remains a separate requirement. This process proof does not choose its protocol or validate RTT/loss distributions.

## Evidence and limits

The [independent Luna xhigh review](../../reviews/active-luna-review.md) approved the corrected 18-file freeze, verified the actual wrapper report and ran 20 parser/process tests plus separate hostile/lifecycle probes. No BLOCKER or MAJOR findings remain. Linux waitid and other POSIX execution remain open platform checks.

The [adapter procedure](../../adapters/active-process.md), [source inventory](../../licenses/active-research-sources.json) and [initial runtime report](../../../research/active/evidence/initial-loopback-macos-arm64.json) distinguish four actual one-second, 2 Mbps, single-stream TCP/UDP loopback direction runs from deterministic/fake-child tests. Both endpoint byte counters agree for these runs. Those initial commands launched iperf3 directly; the corrected wrapper subsequently passed eight actual checks, including cancellation, timeout, recovery and refused connection, after explicit socket approval. An earlier false success from an uncaught server-thread exception is retained as rejected evidence. The corrected helper fails on thread or cleanup errors. See the [corrected wrapper report](../../../research/active/evidence/loopback-macos-arm64.json).

The initial executed binary hash was not retained before a same-source rebuild. The current installed binary hash is recorded separately and must not be retroactively attributed to those initial runs. Parallel streams remain explicitly unsupported pending actual execution. Upstream `make check` reports 5/5, but the authentication test's no-OpenSSL branch performs no authentication validation.

The pinned server JSON repeats `start.target_bitrate`; an acceptance-only reader permits exactly one identical duplicate matching the requested rate at that location. The client parser rejects all duplicate keys. No network endpoint is accepted as an independent RF counter oracle.

## Consequences and remaining gates

The accepted runtime scope binds only owned loopback servers and records exact executable hashes. Other platform execution remains open. Controlled Ethernet/shaped-loss reference tests, authentication, LAN/remote consent, interface/route/BSSID attribution, real load/CPU limits, independent captured counters, multistream/bidirectional modes, platform packaging and canonical integration remain open. Process separation provides no filesystem/network sandbox or hard memory quota. Rate pacing can briefly exceed its requested average. Source and third-party notices require release review before bundling; a process boundary is not a license exemption.

## Reversibility

Keep the research wire contract separate from canonical observations. A production adapter or FFI experiment must preserve explicit units, source/build identity, topology and unknown/failure evidence, with independent review and acceptance for every supported mode. Do not silently change tool versions in reproducible measurements.
