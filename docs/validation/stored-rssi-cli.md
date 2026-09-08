# Stored RSSI analysis CLI validation

Focused validation for this increment runs from the repository root:

```text
cargo test -p kyberia-cli --locked --offline
cargo clippy -p kyberia-cli --all-targets --locked --offline -- -D warnings
cargo fmt --all -- --check
```

`apps/cli/tests/stored_analysis.rs` builds real read-only project bundles with
the project-store survey snapshot and observation-chunk APIs, then invokes the
compiled `kyberia` binary as a subprocess. It verifies a measured point and a
separate unsupported grid gap, the exact canonical artifact hash/byte length,
repeated metric binding, synthetic evidence rejected as unknown, malformed and
oversized requests, a future project revision, and an existing destination
whose sentinel remains unchanged. The Unix FIFO case invokes the command
without opening the FIFO and verifies prompt rejection with no output
directory, exercising the bounded regular-file preflight.

The existing CLI workflow tests use the same retained-directory policy. All
fixtures are created below the ignored `.trash/test-runs` directory and are
left available for manual inspection; tests do not recursively remove them.

Publication is tested through the failed existing-destination path, a final
path conflict that preserves existing bytes, and the successful pending-file
and final-link paths. The publication implementation fsyncs the complete
pending artifact before the same-filesystem hard link and fsyncs its parent
directory. A crash between those steps is represented by a retained pending
file with no final artifact; crash-injection and signal cancellation remain
integration gates because this command intentionally uses `NeverCancel`.

The artifact's selected observation and snapshot provenance is independently
validated by `kyberia-stored-analysis` before the CLI report is emitted. The
CLI does not claim a whole-bundle integrity scan or a durable derived-artifact
index. Those are separate storage and job-graph follow-ups.
