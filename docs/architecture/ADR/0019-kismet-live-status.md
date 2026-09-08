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
`/datasource/all_sources.json`, in that order. It uses ureq 2.12.1 with the
rustls feature, redirects disabled, proxy-from-environment disabled, and a
bounded overall attempt timeout plus the remaining shared poll budget on each
request. Plaintext HTTP is permitted only for literal loopback fixture
addresses; remote endpoints require HTTPS. The caller supplies an opaque
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
- A new HTTP implementation was rejected because mature ureq transport and
  rustls already provide TLS, status handling and socket timeouts.
- A Kismet tracker/device aggregate was rejected as a survey observation;
  time-resolved packet evidence requires a later, separately validated path.

## Consequences and open gates

The adapter can safely inspect local or remote Kismet operational state and
record deterministic capability evidence without a Kismet dependency in the
domain. The local fixture proves exact request/cookie behavior, retry and
decoder limits. It does not prove live-server parity, hardware capture,
WebSocket framing, source hopping/dwell/drop telemetry, clock correlation,
packet normalization, remote TLS deployment, or GPL distribution clearance.
Those remain Gate H/OSS-001 acceptance work and require a controlled pinned
Kismet runtime. The source package is tracked only as an external process; no
GPL source or helper is bundled.

## Validation

Focused validation is recorded in
[`docs/validation/kismet-live-status.md`](../../validation/kismet-live-status.md).
Workspace architecture, source inventory, formatting and clippy checks must
pass before integration. The real-server gate remains separately open.
