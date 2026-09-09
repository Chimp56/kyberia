# Operation-store validation evidence

The project-store persistence adapter is exercised by
`crates/project-store/tests/operation_store.rs` and the private fault-hook
fixture in `crates/project-store/src/operation_log.rs`.

## Executable coverage

The focused tests demonstrate:

- exact canonical wire/canonical-byte/hash round trips and deterministic
  ordered replay after reopen;
- the outward typed replay API preserving an explicit unknown calibration undo
  through SQLite append, reopen, and replay while the legacy mutation-only API
  returns `TypedPriorRequired` explicitly;
- a resolved V2 unknown-calibration conflict persisted and replayed after
  reopen, proving the resolution value is not reduced to a sentinel or
  silently discarded;
- idempotent duplicate appends, including a retry with the stale revision held
  before an uncertain successful commit;
- tamper rejection for changed digest, canonical bytes, malformed/future wire
  versions, project identity, missing parents, and oversized rows;
- admission rejection for sequential repeated undo/redo toggles before any SQL
  write, while unresolved concurrent edit conflicts remain admissible;
- operation graph validation for concurrent branches and explicit replay
  refusal while a semantic conflict remains;
- scalar SQLite BLOB-length preflight rejects a malicious 4 MiB operation row
  before the adapter selects its bytes into Rust;
- a separate operation `ProjectVersion` that remains distinct from the bundle
  manifest revision when an artifact is committed between operation appends;
- immediate-transaction rollback when projection publication fails;
- a deterministic after-projection fault proving that a projection ahead of
  SQLite is detectable and repaired with `recover_manifest`;
- cancellation before mutation through the optimistic project-revision
  precondition, with unchanged operation state and manifest;
- read-only append rejection and additive migration from a valid bundle with
  no operation tables;
- fail-closed operation inventory and state corruption reporting through
  `verify`.

Run the focused adapter suite offline with:

```text
cargo test -p kyberia-project-store --locked --offline
```

The adapter stores no observation payloads. Observation chunks remain in their
own immutable artifact/table group and can be referenced by typed operation
payloads through the inward operation-log contract.

## Remaining validation

The current fixtures prove transactional behavior and bounded recovery paths,
but cannot simulate every filesystem power-loss ordering or platform SQLite
durability mode. Release validation must add process termination during actual
append/finalization, reopen on each supported platform, backup and migration
fixture review, and disk-full behavior. Application integration must apply
typed mutations to the canonical project and validate entity references.
Collaboration integration must add signatures, authorization, replay
protection, reordered/duplicated delivery, chunk availability, and bounded
multi-device merge stress.
