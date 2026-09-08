# Kismet live status data contract

Status polling uses the versioned Kyberia schema
`kyberia.kismet-live-status/1`. The result is a Kyberia-owned operational DTO
with producer version, known server status fields, datasource driver
capabilities, datasource state, and field-availability lists. Kismet tracker
objects and arbitrary JSON maps do not cross the adapter boundary.

The three upstream read-only resources are fetched in this order:

1. `/system/status.json` — requires the Kismet fields
   `kismet.system.version` and `kismet.system.git`; the version is retained as
   both raw text and parsed numeric components and must match the pinned
   acceptance tuple `2026.09.0`/`2d25ad0` (or its full commit). Server clock
   values, when present, remain source-clock facts and do not establish
   synchronization.
2. `/datasource/types.json` — driver type and capability fields are retained
   as nullable values because an omitted capability is unknown rather than
   false.
3. `/datasource/all_sources.json` — source UUID is required; interface,
   current channel, hop plan/rate, running/error/remote state, source version,
   packet counters and warnings are optional facts. A null current channel is
   an unknown channel, not channel zero.

The wire responses are bounded before JSON decoding. Objects reject duplicate
keys, arrays and objects are checked for bounded nesting, and known fields
reject wrong JSON types. Unknown additive fields are ignored after the
availability list is built, so a future field cannot silently become a
canonical measurement. Duplicate driver types or source UUIDs fail the whole
snapshot; an HTTP authentication or authorization failure never becomes an
empty inventory.

The serialized receipt has deterministic field order and sorted inventories.
`canonical_sha256` hashes those exact bytes with SHA-256. It contains no API
token, URL, response body, or raw Kismet tracker object. It is suitable for
replay/provenance of a status snapshot only. No status field is normalized into
an `ObservationEnvelope`; live packet/event ingestion, drop telemetry, clock
correlation, dwell, pose and source identity association remain later gates.

The accepted local fixture is independently authored in
`crates/kismet-adapter/tests/live_status.rs` and covers local and remote source
flags, absent channel/clock facts, authentication, redirects, body limits,
exact paths, cookie transport and secret-free serialization. It does not claim
parity with a running Kismet installation.
