# Active-measurement foundation validation

This record covers the bounded TCP-connect active-measurement increment. It
does not close ACT-001/ACT-005, Phase 1, iPerf Gate D, persistent active-test
storage, or product/UI acceptance.

## Focused checks

```text
cargo fmt --all -- --check
cargo test -p kyberia-active-measurement --locked --offline
cargo clippy -p kyberia-active-measurement --all-targets --locked --offline -- -D warnings
python3 tools/architecture.py check
git diff --check
```

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
- successful RTT median/p90/p95/p99/max and consecutive-loss-burst invariants;
- run-wide schedule limits against forged maximum-per-endpoint intervals;
- schedule admission against same-ID target, authorization, limit, and
  provenance substitutions;
- rejection of IPv4-mapped loopback/private/multicast/broadcast addresses;
- rejection of unscoped IPv6 link-local addresses;
- standalone RTT, loss-burst, statistics, timestamp, and window wire
  revalidation;
- a real std TCP connector against a bounded loopback listener and a local
  refused port. The integration case exits early when the host sandbox denies
  listener creation; no test contacts an external address.

The canonical sample retains an outcome for every scheduled item. Successful
samples contain measured TCP connect RTT; failed and cancelled samples contain
unknown RTT evidence with a reason. Statistics exclude cancelled items from
loss percentage and terminate loss bursts at cancellation boundaries. When no
loss burst exists, its percentile evidence is `NotApplicable`.

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
