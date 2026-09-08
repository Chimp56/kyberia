# Kismet live control/status implementation packet

Status: NOT_STARTED implementation; primary-document investigation completed 2026-09-08.
Scope: plan Appendix I required adapter designs, live control/status input; backlog OSS-001.
This is an executable work specification, not evidence of implemented functionality.

## Verified upstream contract

Kismet documents API tokens in a `KISMET` cookie and recommends API keys for recurring tools. Critically, `/session/check_session` validates login sessions and does **not** accept API keys. Do not use that endpoint as the admission gate for a read-only API token. Validate token access through the requested read-only resource. Roles are not inherited. Source: [official authentication documentation](https://www.kismetwireless.net/docs/api/login/).

Read-only endpoints are `/system/status.json`, `/datasource/types.json`, and `/datasource/all_sources.json`. The system timestamp endpoint is `/system/timestamp.json`; its returned server time alone cannot establish client/server clock synchronization. Sources: [system API](https://www.kismetwireless.net/docs/api/system/) and [datasource API](https://www.kismetwireless.net/docs/api/datasources/).

Datasource listing reports configured sources and their state. `/datasource/list_interfaces.json` instead invokes capture helpers and requires admin access. Routine status polling must not accidentally invoke that hardware probe. Source: [datasource API](https://www.kismetwireless.net/docs/api/datasources/).

## Bounded implementation task

Implement authenticated transport and deterministic versioned status decoding inside the outward Kismet adapter. Select/pin a mature HTTP/TLS library after dependency review; do not implement HTTP or crypto. Keep secrets outside serializable/debuggable DTOs. Disable cross-origin redirects and never put API tokens into query strings or provenance. Bound response bytes, nesting, datasource count, connection/read/total duration, retry count and backoff. Support cancellation and expose authentication, transport, unsupported-schema and malformed-payload failures distinctly.

Negotiate supported producer versions using real pinned server output and public field definitions before evidence ingestion. Preserve source version and field availability; a database schema version is not a producer version. Missing channel/dwell/calibration/time facts remain unknown. Datasource/device status aggregates must never become packet observations. Model disconnect/reconnect and dropped-event telemetry explicitly when live evidence consumption is added.

## Acceptance tests before integration

1. An independent local HTTP fixture verifies exact requested paths and cookie authentication; token is absent from URLs, errors, logs and result serialization.
2. Token authentication succeeds on a read-only resource without calling the incompatible session-check endpoint; 401/403 do not become empty datasource success.
3. Redirects, oversized/slow/truncated bodies, malformed JSON, duplicate keys, unknown semantic versions and oversized source inventories fail with structured errors and bounded work.
4. Cancellation, reconnect attempts and retry budgets are deterministic under an injected clock. Tests do not use wall-clock sleeping for backoff assertions.
5. Golden fixtures are independently constructed and carry fixture provenance. They cover local/remote sources and unknown optional channel/clock facts without claiming real-server parity.
6. A separate real-server gate records binary version, source commit, authentication mode, exact endpoint contract, field capabilities and controlled datasource results. No capture control or privileged helper installation occurs implicitly.

The current repository has no live transport implementation. KismetDB normalization and no-follow source-open tests are independently reviewed offline capabilities. Their approval does not close this packet or the complete Kismet runtime gate.

## Local runtime preparation

The plan-pinned upstream archive `kismetwireless/kismet@2d25ad004e9216ac963c4f156e9077331717959c` was fetched from GitHub and inspected before extraction into ignored `.tools/kismet-runtime/`. Archive SHA-256: `38074b597d80f1566331efd845f88e1f876c5efec46b9d2f8c9986b7d0b6de4c`. All 5,203 archive entries were regular files/directories with relative paths and no traversal components. No Kismet runtime is currently installed or validated.

`configure --help` was inspected. Configuration/build has not run: the Autoconf script executes `rm -rf`/`rm -f -r` on generated `conftest*`, `confdefs*`, `conf<PID>*`, compiler probe outputs and its temporary configuration directory. User policy requires explicit target-scoped permission before those recursive deletions. No privileged helper or system installation is planned. This permission gate applies only to the local external build; Kyberia implementation and fixture tests continue.
