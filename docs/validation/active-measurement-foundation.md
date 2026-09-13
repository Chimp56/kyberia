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
- deterministic endpoint-id-then-ordinal ordering and hash-derived sample IDs;
- total sample, duration, spacing, timeout, and concurrency bounds;
- serial fake-connector outcomes for success, refusal, timeout, error,
  cancellation, and overall deadline;
- successful RTT median/p90/p95/p99/max and consecutive-loss-burst invariants;
- a real std TCP connector against a bounded loopback listener and a local
  refused port. The integration case exits early when the host sandbox denies
  listener creation; no test contacts an external address.

The canonical sample retains an outcome for every scheduled item. Successful
samples contain measured TCP connect RTT; failed and cancelled samples contain
unknown RTT evidence with a reason. Statistics exclude cancelled items from
loss percentage and terminate loss bursts at cancellation boundaries.

## Boundaries and known gaps

`kyberia-domain` has no networking, clock, storage, process, UI, or foreign
schema dependency. `kyberia-active-measurement/src/pure.rs` has no socket or
process API; `src/adapter.rs` is the only production module that imports
`TcpStream`. The shipped executor is deliberately serial even though the
schedule records a maximum concurrency bound. DNS, ICMP, UDP, HTTP/TLS,
QUIC, throughput, roaming, Wi-Fi link telemetry, remote authenticated agents,
and persistence remain open requirements.

The loopback evidence demonstrates executable lifecycle and typed results,
not cross-platform socket parity or Internet performance. A connector is
expected to honor the supplied per-attempt timeout; the std adapter uses the
OS bounded `connect_timeout` call. An outer application still must enforce
user/session policy and persist the canonical records through its own reviewed
boundary.
