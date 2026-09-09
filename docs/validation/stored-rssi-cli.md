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
file with no final artifact. Signal cancellation is covered on Unix through
the production `signal-hook` adapter; non-Unix signal delivery remains an
explicit platform capability gap.

The Unix SIGINT subprocess test waits for the production `analysis_started`
stderr barrier, sends a real signal during a maximum-size valid grid, waits
with a bounded deadline, and verifies the structured cancellation error and
absence of `analysis.json`. Unit tests inject cancellation before destination
creation, after pending-file sync, and at the final-link commit point. The
nearest and IDW subprocess cases assert numeric `-55 dBm` output and the
interpolated cell class, so method coverage checks values as well as counts.

Dispatch tests verify that SIGINT registration is selected only for a valid
four-argument analysis invocation; help, project, verification, recovery,
export, and malformed analysis invocations use the normal command path without
installing the analysis handler. A fault-injected directory-sync failure after
the final hard link verifies the structured `publication_durability` error,
`committed: true`, and preservation of both final and pending bytes.

The artifact's selected observation and snapshot provenance is independently
validated by `kyberia-stored-analysis` before the CLI report is emitted. The
CLI does not claim a whole-bundle integrity scan or a durable derived-artifact
index. Those are separate storage and job-graph follow-ups.
