# ADR-0031: Bounded active-measurement foundation

- Status: Proposed bounded foundation; independent review and product wiring remain open
- Related: plan §§7.17–7.18, 10.1/10.5/10.17, 15.5, 16.8; `ACT-001`, `ACT-005`, `ACTB-001`–`ACTB-003`, `ACTB-009`

## Context

Active measurements can easily turn a useful survey into an unsafe or
ambiguous network test.  An Internet transfer does not identify a Wi-Fi
problem, an unlabelled `ping` value cannot be compared with a TCP handshake,
and a failed attempt must not be encoded as zero milliseconds.  The first
production increment needs a narrow contract that is safe to execute and
honest about what it measures while the gateway/LAN/Internet/application
topology grows in later iterations.

## Decision

RF Atlas owns a versioned active domain contract in
[`crates/domain/src/active.rs`](../../../crates/domain/src/active.rs).  The
contract includes distinct endpoint, run, interval, sample, and result
identities; endpoint tiers (`gateway`, `lan_reference`, `internet_control`,
and `application`); literal IPv4/IPv6 socket targets; TCP plus the explicit
`TcpConnectTiming` method; and endpoint attribution for interface, route, and
BSSID.  Attribution uses `Evidence<T>` and remains unknown when the source
cannot provide it.

An active run requires explicit user consent and a tier allow-list.  Target
validation rejects unspecified, multicast, and broadcast addresses.  Loopback
Loopback targets require both the corresponding authorization flag and a local
gateway/LAN tier.  IPv4 link-local targets are likewise tier- and
authorization-gated.  Gateway/LAN targets must be local-only addresses;
Internet-control targets must not be local-only.  The literal address contract
rejects IPv6 link-local addresses because this version has no zone/interface
identifier or scoped socket representation.  Endpoint
and target tiers must match, IPv4-mapped IPv6 addresses are rejected, and the
only accepted protocol/method pair is TCP connect timing.  The target port is
an explicit numeric TCP port in `1..=65535`.

`kyberia-active-measurement` provides three separated surfaces:

1. `pure` builds a deterministic endpoint-id-then-ordinal schedule with
   SHA-256-derived unique sample identities, no randomization, a bounded sample
   count, minimum spacing, timeout, duration, and concurrency declaration.
   The schedule carries the authorization, sample-limit, and provenance
   contract used to build it.  Canonical result admission also rejects a
   repeated sample identity, even when ordinals differ.
   The canonical run registers every interval and admits their cumulative
   endpoint-by-sample count against one run-wide budget. Duplicate interval
   identities and cumulative overflow are rejected during both construction
   and deserialization. A schedule is valid only for an interval present in
   that registry, and carries the complete registry in its execution contract.
   Older v1 run JSON without the additive registry field decodes to an empty
   registry and therefore cannot execute a detached interval accidentally.
2. `executor` is generic over monotonic clock, cancellation, and TCP connector
   ports.  It re-derives and compares the complete run/interval schedule
   contract before executing, so a same-ID schedule with a different target,
   authorization, limit, or provenance is rejected.  It executes serially in
   this increment, preserving the configured
   concurrency ceiling, and emits one canonical sample for every scheduled
   sample.  Cancellation emits `Cancelled`; an overall deadline emits
   `Timeout`; neither silently drops the remainder.
3. `adapter` supplies a real `StdTcpConnector` backed by Mio's nonblocking OS
   TCP stream and a literal address.  It polls connection readiness and
   cancellation in 25 ms bounded slices, then requires both a clear socket
   error and a connected peer before recording success.  This prevents
   pending/WouldBlock state from becoming a false connection.  The clock
   adapter uses the same bounded cancellation-aware sleep port.  It never
   resolves names, spawns a process, accepts command fragments, or contacts a
   target outside the endpoint contract.

Successful attempts record monotonic TCP connect duration/timing in
milliseconds.  Refusal, timeout, unreachable, permission, generic error, and
cancellation are typed outcomes with unknown TCP connect duration evidence.
Statistics are computed from successful connect timings using linear
interpolation at `q * (n - 1)` for median, p90, p95, and p99, with max as the
largest successful sample.  TCP attempt failure percentage uses non-cancelled
attempts; consecutive non-cancelled failures are retained as burst lengths,
while a run with no failed attempt has `NotApplicable` burst percentiles.
Packet-loss percentage remains `Unknown(NotMeasured)` because TCP connection
outcomes do not measure packet loss.  No failure contributes a zero duration.
Known connect duration must equal the sample's monotonic window. Standalone
wire admission rechecks the sample ceiling, evidence reasons, ordered
percentile shape, one-sample and one-burst feasibility, success/failure and
cancellation counts, the exact attempt-failure percentage, and the rule that
TCP connect supplies no packet-loss evidence. A complete result additionally
recomputes its statistics from retained samples.

Hard limits in this increment are 32 endpoints, 4,096 total samples, eight
concurrent operations (the shipped executor uses one), 60 seconds per
attempt, 3,600 seconds per run, and 3,600 seconds minimum-spacing ceiling.
The application must choose smaller values when appropriate.  Throughput,
ICMP, UDP echo, DNS, HTTP/TLS, QUIC, roaming, Wi-Fi telemetry, and external
Internet claims are outside this module.

## Alternatives

1. **Invoke `ping`, `iperf3`, or a shell command.** Rejected because command
   fragments and process behavior would expand the attack surface and blur
   protocol semantics.  iPerf remains a later separately reviewed Gate D
   decision.
2. **Resolve hostnames in the active adapter.** Rejected because destination
   identity and timing could change with DNS, search domains, or resolver
   policy.  A future resolver adapter must have its own bounded contract.
3. **Represent failures as zero-valued metrics.** Rejected because zero ms is
   a valid-looking measurement.  Failed samples retain typed outcomes and
   `Evidence::Unknown` TCP connect duration; packet loss remains explicitly
   unmeasured.
4. **Put socket I/O in the domain crate.** Rejected because clocks, network
   APIs, and cancellation are side effects that must remain behind ports and
   adapters.
5. **Parallelize all endpoints immediately.** Rejected for this bounded
   increment because deterministic serial ordering makes lifecycle and rate
   impact independently checkable.  The schedule carries a bounded
   concurrency contract for a future executor.

## Evidence

The focused suite in
[`crates/active-measurement/tests/active.rs`](../../../crates/active-measurement/tests/active.rs)
checks malformed ports and units, target/tier and authorization mismatch,
unknown attribution, deterministic ordering and identities, bounded sample
limits, typed success/refusal/timeout/cancellation outcomes, deadline
completion, connect-timing/failure-burst invariants, and a loopback integration path
when the sandbox permits listener creation.  The integration test uses only
127.0.0.1 and skips when the host denies local listener creation.  Fake clock
and connector ports cover all lifecycle branches without external network
access.

The domain crate contains no adapter or storage dependency.  The architecture
policy records the active crate as an adapter depending on the canonical
domain, SHA-256 identity derivation, and the narrowly scoped nonblocking socket
adapter.  Active timestamp and window wire wrappers deny unknown nested fields
so permissive global monotonic-time decoding cannot widen this record schema.
The contract is a foundation and does not claim completion of ACT-001/ACT-005,
Phase 1, iPerf, multi-tier probes, or the product definition of done.
No persisted independent review packet exists for this Rust foundation. The
semantic correction and acceptance assertions are explicit inferences from
the related plan sections and current `STATUS.md`; independent review remains
required before integration.

## Consequences

Each result can be interpreted without guessing protocol, target tier,
endpoint identity, or interface/route/BSSID attribution.  Connect-timing
distributions and TCP attempt failure bursts remain independently recomputable
from retained samples, while packet loss stays explicitly unmeasured.
Explicit target safety and consent reduce accidental scans of unsafe address
classes.  A serial connector limits test-induced load and makes schedule
ordering reproducible, at the cost of lower throughput for multi-endpoint
runs.  Literal addresses require an outer application or user to establish
and attest target identity; this increment intentionally does not perform
discovery.

## Security and privacy

The adapter has no resolver or process execution path, and the domain rejects
unsafe address classes before execution.  Loopback/link-local access is opt-in
and tier constrained.  Endpoint attribution may retain a BSSID only as
evidence supplied by an authorized source; unknown values remain unknown.
Application integration must add authenticated LAN-agent enrollment and
project/session retention policy before exposing remote targets.

## Reversibility and follow-up

The std connector and generic ports can be replaced without changing the
canonical records.  A future UDP/ICMP/application protocol requires a new
method/version and its own failure/units contract rather than reusing
`TcpConnectTiming`.  Follow-up work must add authenticated LAN agents, gateway
and LAN/application attribution views, iPerf Gate D evidence, persistent
active-test storage, and independent review before making broader product
claims.
