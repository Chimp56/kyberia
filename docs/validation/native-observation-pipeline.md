# Native observation pipeline validation

The focused suite in
crates/observation-pipeline/src/tests.rs
normalizes the repository macOS collector fixture and exercises the complete
composition path. It proves:

* normalized envelopes are associated through receipt timing while capture
  time, pose, dwell, cache age and strict RSSI progress remain unknown or
  unchanged;
* capture schema, build hash, capability evidence, terminal status, source
  references and source-order observation IDs survive manifest serialization;
* mismatched adapter completion counts are rejected before any publication,
  and tampering with the persisted manifest bytes is detected on read;
* the durable observation chunk and association snapshot reopen and replay
  through kyberia-project-store, including the durable manifest-to-chunk/
  snapshot link;
* exact duplicate retry is idempotent for chunk, snapshot and publication
  links;
* same-count chunks with different observation IDs, same-project snapshots
  without the manifest's associations, and same-ID snapshots with changed
  copied envelope metadata are rejected before link mutation;
* canonical observations with a substituted collector identity or capture
  mode are rejected against the survey configuration;
* every association is checked before a port write, preserving state and
  row counts on source/quality rejection;
* cancellation before publication leaves the port untouched, and
  cancellation or failure after chunk publication returns an explicit
  partial-publication receipt that can be retried;
* cancellation after an empty capture manifest is committed returns partial
  progress before a survey snapshot is written, and the retry completes the
  terminal link without a duplicate snapshot;
* retained raw references have verified immutable artifact closure, while
  discarded capture policy is explicit and does not claim raw retention;
* partial, error, permission and empty captures persist their terminal and
  capability evidence without manufacturing observations;
* duplicate observation IDs, negative publication timestamps and future
  pipeline schema versions fail at the boundary.

Run the focused and repository checks from the workspace root:

    cargo test -p kyberia-observation-pipeline --offline
    cargo test -p kyberia-project-store --offline
    cargo clippy --offline --all-targets -- -D warnings
    cargo fmt --all -- --check
    /Users/vincent/code/kyberia/.tools/venv/bin/python tools/architecture.py
    /Users/vincent/code/kyberia/.tools/venv/bin/python tools/source_inventory.py check

Use the workspace's configured validation interpreter when running the full
source-inventory gate. The focused tests use only canonical fixture input and
do not call CoreWLAN, synthesize capture timestamps or persist raw packet
payloads.

Open gates are native CoreWLAN transport/authorization supervision, the
project-store co-transaction optimization for chunk and snapshot metadata, UI
command wiring and strict point completion from actual hardware capture
evidence. The current recoverable link protocol is the accepted Phase 0
behavior; those remaining gates require platform and product integration work
outside this composition increment.
