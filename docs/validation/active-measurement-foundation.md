# Active-measurement foundation validation

This record covers the bounded TCP-connect-timing method version
`rf-atlas-active-tcp-connect-timing/v1`. It
does not close ACT-001/ACT-005, Phase 1, iPerf Gate D, persistent active-test
storage, or product/UI acceptance.

## Focused checks

```text
cargo fmt --all -- --check
cargo test -p kyberia-active-measurement --locked --offline
cargo clippy -p kyberia-domain -p kyberia-active-measurement --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
python3 tools/architecture.py check
<Python 3.11> tools/source_inventory.py check
<Python 3.11> tools/ledger.py check
git diff --check
```

On 2026-09-13 the focused suite passed 17/17 tests, affected-package Clippy
passed with warnings denied, and the complete workspace suite passed with all
non-ignored tests green. The initial sandboxed workspace run reached five
unrelated Kismet loopback fixtures and failed because listener creation was
denied; the authorized loopback rerun passed. Architecture, the 241-package
source inventory, the 5,392-block traceability ledger, formatting, and diff
checks passed.

Hosted run `34768828271` at integrated `1d4d62f` passed Ubuntu but failed
Windows in the then-combined
`real_loopback_adapter_records_success_and_refusal_without_external_network`
test. macOS failed separately in the then-combined
`supervised_process_enforces_timeout_cancellation_and_bounded_descendant_drain`
test. Public annotations expose only those test names and exit status; the
hosted log is not available without administrator authentication. Mio's
cross-platform TCP contract requires writable readiness before checking
`take_error` and `peer_addr`; its Windows AFD adapter also reports
receive/close events through readable readiness. The connector now registers
writable interest only and rearms it when the peer query observes a transient
not-connected state. The repeated loopback regression exercises successful
connects whose peer is closed immediately after acceptance. The production
loopback integration is now split into separately named success and refusal
tests, retaining the bind-then-drop refused-port check, so the next public
annotation identifies the failing active stage. A separate mixed integration
also executes success followed by refusal through one `StdTcpConnector` and
asserts canonical endpoint ordering and typed outcomes. The process regression
is similarly split into timeout/cancellation, descendant-pipe-drain, and
escaped-descendant-pipe-drain tests. These changes preserve literal target
admission, typed refusal/error outcomes, cancellation polling, and bounded
timeouts.

The correction passes the authorized macOS loopback checks and
`cargo check -p kyberia-active-measurement --target x86_64-pc-windows-gnu
--locked --offline`. Native Windows execution remains pending until the
corrected source is pushed to hosted CI; no Windows pass is claimed here.

The focused Rust suite covers:

- finite, nonnegative, unit-safe timing values and nonzero ports;
- unspecified/multicast/broadcast rejection;
- loopback authorization and endpoint-tier/target-tier mismatch;
- explicit consent/tier allow-list enforcement;
- unknown interface/route/BSSID attribution preservation;
- deterministic endpoint-id-then-ordinal ordering and unique hash-derived
  sample IDs, with canonical result admission rejecting repeated IDs;
- total sample, duration, spacing, timeout, and concurrency bounds;
- serial fake-connector outcomes for success, refusal, timeout, error,
  cancellation, and overall deadline, including cancellation-aware 25 ms
  spacing/connector polling;
- successful TCP connect duration median/p90/p95/p99/max and consecutive
  TCP-attempt-failure-burst invariants;
- run-wide cumulative schedule limits across all registered intervals and
  endpoints, including duplicate, detached, overflow, and deserialization
  admission paths;
- schedule admission against same-ID target, authorization, limit, and
  provenance substitutions;
- rejection of IPv4-mapped loopback/private/public/multicast/broadcast
  addresses;
- rejection of IPv6 link-local addresses (scoped representation is not
  supported in this method version);
- standalone connect-timing, TCP-attempt-failure-burst, statistics, timestamp,
  and window wire revalidation, including evidence-reason, count, percentile,
  percentage, resource-ceiling, packet-loss-not-measured, and rejection of the
  prior misleading TCP-connect method version, with exact small-count timing
  and failure-burst feasibility regressions;
- separately named real std TCP connector success and refusal integrations
  against a bounded loopback listener and a local refused port, plus a mixed
  success-then-refusal sequence through one connector with canonical result
  ordering assertions. A repeated-connect regression closes each accepted
  peer immediately to exercise writable completion and readiness rearming. The
  integration cases exit early when the host sandbox denies listener creation;
  no test contacts an external address.

The canonical sample retains an outcome for every scheduled item. Successful
samples contain measured TCP connect duration; failed and cancelled samples
contain unknown duration evidence with a reason. Statistics exclude cancelled
items from TCP-attempt-failure percentage and terminate failure bursts at
cancellation boundaries. Packet loss remains `Unknown(NotMeasured)` because
this TCP probe has no packet-loss measurement. When no failure burst exists,
its percentile evidence is `NotApplicable`.
The known duration is required to equal the monotonic sample window.

The repository has no persisted independent review packet for this Rust
foundation. The semantic correction and the acceptance assertions above are
therefore documented inferences from plan §§5.16, 7.17–7.18, 15.5, 16.8,
16.14, ACTB-001/002, Iteration 5, and the current `STATUS.md`; independent
review remains required before integration.

## Boundaries and known gaps

`kyberia-domain` has no networking, clock, storage, process, UI, or foreign
schema dependency. `kyberia-active-measurement/src/pure.rs` has no socket or
process API; `src/adapter.rs` is the only production module that imports the
Mio TCP adapter. Active timestamp/window wire wrappers explicitly
reject unknown nested fields even though the shared monotonic types remain
additively decodable elsewhere. The shipped executor is deliberately serial
even though the schedule records a maximum concurrency bound. DNS, ICMP, UDP,
HTTP/TLS, QUIC, throughput, roaming, Wi-Fi link telemetry, remote
authenticated agents, and persistence remain open requirements.

The loopback evidence demonstrates executable lifecycle and typed results,
not cross-platform socket parity or Internet performance. The endpoint port is
an explicit numeric TCP destination in `1..=65535`; no service-name lookup is
performed. A connector must honor the supplied per-attempt timeout and
cancellation port; the std adapter uses a nonblocking socket with 25 ms
readiness/cancellation polling. An outer application still must enforce
user/session policy and persist the canonical records through its own reviewed
boundary. If the host denies loopback
listener creation, the integration test records no pass and exits through an
explicit environment-gated skip path; fake-port assertions remain separate.
