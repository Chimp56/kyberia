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
the real ureq transport with exact paths and `KISMET` cookie authentication,
local/remote source flags, absent channel facts, secret-free result bytes,
401 handling without `/session/check_session`, redirects without credential
forwarding, oversized bodies, remote-HTTP rejection and a real trickle-body
deadline. Separate injected transport tests prove
transport/status retries and bounded retry schedules without wall-clock
sleeping.

The fixture producer identity is the inspected pinned source tuple
`2026.09.0`/`2d25ad0`; a syntactically valid future date or missing/mismatched
source identity is rejected. The live transport uses ureq 2.12.1 with rustls,
redirects disabled and proxy-from-environment disabled. The acceptance run
also passes workspace tests, workspace Clippy with warnings denied, the
architecture dependency-direction check, the 181-package source inventory,
formatting, cargo-deny advisories/licenses/sources/bans, and `git diff --check`.

The golden JSON strings are independently authored test inputs, not Kismet
source or copied captures. They establish decoder and transport behavior only;
they are not live-server parity evidence. The remaining real-server gate must
record the exact binary/version/revision, TLS/authentication mode, endpoint
responses, field availability, controlled datasource state, and license/package
review. Capture control, privileged helper installation, WebSocket evidence,
packet observations, clock correlation and KismetDB/PCAPNG parity remain open.
