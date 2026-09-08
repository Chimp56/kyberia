# Kismet live status validation

This increment proves the bounded read-only REST status/capability boundary.
It does not close the Kismet Linux runtime or hardware gate.

## Focused checks

```text
cargo test -p kyberia-kismet-adapter --locked --offline
cargo clippy -p kyberia-kismet-adapter --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
```

The focused suite includes unit tests for duplicate keys, producer-version
shape and policy rejection, unknown optional fields, deterministic ordering,
inventory/depth limits, cancellation, global deadline checks and injected
retry clocks. The local TCP fixture tests
the real reqwest transport with exact paths and `KISMET` cookie authentication,
local/remote source flags, absent channel facts, secret-free result bytes,
401 handling without `/session/check_session`, redirects without credential
forwarding, oversized bodies, remote-HTTP rejection and a real trickle-body
deadline. Separate injected transport tests prove
transport/status retries and bounded retry schedules without wall-clock
sleeping.

The fixture producer identity is the inspected pinned source tuple
`2026.09.0`/`2d25ad0`; a syntactically valid future date or missing/mismatched
source identity is rejected. The live transport uses reqwest 0.12.28 with
rustls, redirects disabled and environment proxies disabled. Its request
timeout covers connection establishment, TLS negotiation and completion of the
response body. Hostname endpoints must provide a bounded explicit address list;
literal IP endpoints are bound automatically. `resolve_to_addrs` maps the
validated URL hostname to that list without invoking system DNS, while the URL
hostname remains the TLS/HTTP authority. The explicit-address TLS fixture also
observes that hostname in the ClientHello. A pinned reqwest source probe and
the real incomplete-TLS-record fixture demonstrate that a slow handshake
cannot outlive the shared deadline. Automatic DNS acquisition remains a
separate open capability because obtaining addresses outside this adapter still
needs a mature cancellable resolver. The acceptance run passes workspace
`cargo check`, workspace Clippy with warnings denied, the
architecture dependency-direction check, the generated source inventory,
formatting, cargo-deny advisories/licenses/sources/bans, and `git diff --check`.

The live deadline tests assert wall-clock completion as well as the typed
error: one server sends an incomplete TLS record in 40 ms fragments and another
sends a response byte every 10 ms. Both must terminate within a 350 ms test
bound despite request budgets of 100 ms and 80 ms respectively. These are
transport-boundary tests, not evidence of Kismet server parity. The blocking
API retains cooperative cancellation: a token set by another thread cannot
interrupt a syscall already in progress, so its transport timeout remains the
hard operation bound.

The golden JSON strings are independently authored test inputs, not Kismet
source or copied captures. They establish decoder and transport behavior only;
they are not live-server parity evidence. The remaining real-server gate must
record the exact binary/version/revision, TLS/authentication mode, endpoint
responses, field availability, controlled datasource state, and license/package
review. Capture control, privileged helper installation, WebSocket evidence,
packet observations, clock correlation and KismetDB/PCAPNG parity remain open.

## Integrated reqwest correction regression

At integration `4ff7569`, root independently ran `python3 tools/dev.py lint`
successfully across the workspace. The pinned cargo-deny 0.20.2 executable
also passed `--manifest-path Cargo.toml --config deny.toml --locked --offline
check`: advisories, bans, licenses and sources all passed for the workspace,
including the newly added reqwest closure. This is broader dependency coverage
than the current release CLI audit command; it does not validate other-language
release environments. The 40 adapter tests separately passed after integration.
Full workspace tests remain paused only for the native pipeline's pending
retained-directory integration.
