# ADR 0019: Bounded Kismet live status boundary

- Status: Accepted bounded adapter contract; Kismet runtime gate remains open
- Date: 2026-09-08

## Context

Plan §16.13, §18.11, OSS-001 and Appendix I require Kismet integration through
an external authenticated REST/WebSocket/DB boundary. The first live increment
must establish server/version/capability and datasource health facts without
turning Kismet aggregates into survey observations or invoking capture helpers.
The Kismet API uses a `KISMET` cookie for API tokens; its session-check route
validates browser sessions and is not an API-key admission test.

## Decision

`kyberia-kismet-adapter::live` exposes a read-only `KismetLiveClient` whose
only requests are GETs to `/system/status.json`, `/datasource/types.json`, and
`/datasource/all_sources.json`, in that order. It uses pinned reqwest 0.12.28
blocking HTTP with the `rustls-tls` feature, redirects disabled, environment
proxies disabled, and the remaining shared poll budget as the per-request
total timeout. Reqwest documents this timeout as covering connection
establishment through completion of the response body, including TLS
negotiation. Hostname endpoints require a caller-supplied list of at most
`MAX_ENDPOINT_ADDRESSES` explicit `SocketAddr` values. The adapter uses
reqwest's `resolve_to_addrs` for the exact URL host, so the system resolver is
never reached. The original URL host is still passed to HTTP and TLS for
authority, SNI and certificate verification; addresses select only TCP
destinations. Literal IP endpoints are bound automatically. Plaintext HTTP is
permitted only for literal loopback fixture addresses; remote endpoints require
HTTPS. The caller supplies an opaque
`ApiToken`; secret values never enter
URLs, errors, debug output or serialized status receipts.

Each response is bounded by bytes, nesting, string/list/inventory counts and a
shared poll deadline. Duplicate JSON object keys and malformed or unsupported
known shapes fail closed. Transient status and transport errors use a finite,
injected-clock-testable retry schedule. 401, 403, redirects, transport,
malformed payload, unsupported version/schema, body, inventory and cancellation
errors remain distinct. The producer version is required to match the
inspected acceptance tuple from the pinned Kismet source (`2026.09.0` with
source identity `2d25ad0`, or its full 40-character commit). Kismet's date
version is a build date, so future or otherwise syntactically valid versions
remain unsupported until a new pinned server review; the producer version is
stored separately from the Kyberia status schema version.

The projection contains only operational source facts and field-availability
lists. Missing channel, hop, source-error or clock values remain unknown.
Driver/source inventories are sorted and duplicate identities are rejected.
`canonical_bytes`/`canonical_sha256` provide deterministic, secret-free
provenance. No live status value becomes an observation, clock synchronization
model, regulatory claim, or final RF metric.

## Alternatives

- Calling `/session/check_session` was rejected because it is a browser-session
  endpoint and does not validate API keys.
- Calling `/datasource/list_interfaces.json` was rejected because it invokes
  capture helpers and requires administrative access.
- A generic JSON map was rejected because it permits unknown fields to become
  semantic data and cannot provide a bounded canonical receipt.
- ureq 2.12.1 was rejected after its pinned source and an independent fixture
  showed that its raw rustls handshake could outlive the socket inactivity
  timeout while receiving a slow, incomplete TLS record. A post-hoc elapsed
  check would leave the blocking operation running and was rejected.
- A detached worker or process wrapper was rejected because it would leave
  uncancellable I/O and complicate ownership of the authenticated connection.
- Reqwest 0.12.28 blocking HTTP was selected because its mature rustls client
  exposes explicit address overrides and a total request timeout. The pinned
  crate source documents the timeout from connect through body completion, and
  the retained TLS and trickle-body fixtures assert the wall-clock bound. The
  source contracts are [RequestBuilder::timeout](https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.RequestBuilder.html#method.timeout)
  and [ClientBuilder::resolve_to_addrs](https://docs.rs/reqwest/0.12.28/reqwest/blocking/struct.ClientBuilder.html#method.resolve_to_addrs).
- A Kismet tracker/device aggregate was rejected as a survey observation;
  time-resolved packet evidence requires a later, separately validated path.

## Consequences and open gates

The adapter can safely inspect local or remote Kismet operational state and
record deterministic capability evidence without a Kismet dependency in the
domain. The local fixture proves exact request/cookie behavior, retry and
decoder limits. It does not prove live-server parity, hardware capture,
WebSocket framing, source hopping/dwell/drop telemetry, clock correlation,
packet normalization, remote TLS deployment, or GPL distribution clearance.
The explicit address list avoids any system DNS call inside the bounded poll.
Automatic DNS acquisition remains a separate open capability until a mature
cancellable resolver integration is selected and reviewed; it is not hidden
inside a detached thread or an uncancellable helper. The blocking API retains
cooperative cancellation between requests; a token cannot interrupt a syscall
already in progress, so the reqwest total request timeout supplies that
operation's hard bound. The transport decision is reversible because the
`HttpGet` boundary and wire DTO are unchanged and the dependency is isolated to
the adapter.
Those remain Gate H/OSS-001 acceptance work and require a controlled pinned
Kismet runtime. The source package is tracked only as an external process; no
GPL source or helper is bundled.

## Reversibility

The HTTP client and producer admission tuple are confined to the outward
Kismet adapter. A replacement transport can preserve the versioned status
projection and its contract tests without changing canonical observations.
An expanded producer allowlist requires new source and runtime evidence;
incompatible projection changes require a new status schema version. Stored
receipts retain their original adapter and producer identities.

## Validation plan

Focused validation is recorded in
[`docs/validation/kismet-live-status.md`](../../validation/kismet-live-status.md).
Workspace architecture, source inventory, formatting and clippy checks must
pass before integration. The real-server gate remains separately open.
