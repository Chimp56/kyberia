# Kismet live status security boundary

This adapter is read-only and uses a caller-supplied Kismet API token only for
the three status/capability GET requests. The token is stored in an opaque
`ApiToken`; it has no Serde implementation, and its debug output is redacted.
`Endpoint` also has no secret-bearing display or serialization. `LiveError`
uses fixed messages and status categories, so URLs, headers, response bodies
and tokens cannot escape through errors. The safe status receipt excludes all
of them as well.

The pinned reqwest 0.12.28 blocking client uses rustls TLS, disables redirects,
and calls `no_proxy()`. Thus a configured proxy cannot silently receive a
cookie, and a redirect cannot forward it to another host.
Endpoint validation rejects credentials, query strings, fragments, control
characters and whitespace in the authority, and permits plaintext HTTP only
for literal loopback addresses. Remote endpoints require HTTPS. TLS
certificate validation uses reqwest's pinned rustls/webpki-roots feature;
deployments requiring private roots must provide a separately reviewed
transport configuration rather than disabling verification. Reqwest's
`resolve_to_addrs` maps only the validated URL host to the explicit destination
set, preserving the URL hostname for TLS SNI, certificate verification and
HTTP authority without invoking system DNS.

Response bytes, content-length declarations, JSON nesting, known string sizes,
inventory counts and retry/backoff are bounded before values are exposed to
callers. Reqwest's total request timeout and the per-request remaining poll
budget bound a slow or trickled response, including TLS negotiation. A
cooperative cancellation token is checked before and after each request, after
each decode, and in retry backoff; it cannot cancel a kernel socket operation
already in progress, so the transport timeout is part of the bound. GET
retries are limited to transient status/transport failures and never retry
authentication, redirects or malformed successful payloads.

The adapter does not invoke Kismet capture helpers, list local interfaces,
write configuration, open WebSockets, persist secrets, or expose datasource
aggregates as observations. Kismet is an external GPL process. No Kismet
implementation is copied or linked; this crate owns only the typed status
projection. A real-server security gate still needs controlled TLS roots,
credential handling, remote authorization, network policy, and parity tests.
