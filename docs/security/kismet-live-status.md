# Kismet live status security boundary

This adapter is read-only and uses a caller-supplied Kismet API token only for
the three status/capability GET requests. The token is stored in an opaque
`ApiToken`; it has no Serde implementation, and its debug output is redacted.
`Endpoint` also has no secret-bearing display or serialization. `LiveError`
uses fixed messages and status categories, so URLs, headers, response bodies
and tokens cannot escape through errors. The safe status receipt excludes all
of them as well.

The pinned ureq 2.12.1 agent uses rustls TLS, disables redirects, and leaves
proxy-from-environment disabled (`default-features = false`; the
`proxy-from-env` feature is not selected). Thus a configured proxy cannot
silently receive a cookie, and a redirect cannot forward it to another host.
Endpoint validation rejects credentials, query strings, fragments, control
characters and whitespace in the authority. TLS certificate validation uses
ureq's rustls/webpki-roots path; deployments requiring private roots must
provide a separately reviewed transport configuration rather than disabling
verification.

Response bytes, content-length declarations, JSON nesting, known string sizes,
inventory counts and retry/backoff are bounded before values are exposed to
callers. ureq's overall request timeout and the per-request remaining poll
budget bound a slow or trickled response. A cooperative cancellation token is
checked before and after each request, after each decode, and in retry
backoff; it cannot cancel a kernel socket operation already in progress, so
the transport timeout is part of the bound. GET retries are limited to
transient status/transport failures and never retry authentication, redirects
or malformed successful payloads.

The adapter does not invoke Kismet capture helpers, list local interfaces,
write configuration, open WebSockets, persist secrets, or expose datasource
aggregates as observations. Kismet is an external GPL process. No Kismet
implementation is copied or linked; this crate owns only the typed status
projection. A real-server security gate still needs controlled TLS roots,
credential handling, remote authorization, network policy, and parity tests.
