# Phase 2 bounded PCAPNG / IEEE 802.11 replay increment

This increment adds the `kyberia-pcap-ie-replay` composition crate. It joins
the existing streaming PCAPNG adapter with the independent IEEE management
parser while preserving the adapter boundary: no parser-to-container
dependency was added.

Integrated source commit: `efdf514eb4e4669ab3ae89e67f4b29f54e426321`;
staged-frame cancellation regression: `1332608fb2b2819d2cdcb71161640e73ae8445fa`.
Current-main source review approved the bounded bridge; its one MINOR finding
was stale integration wording in this record and `STATUS.md`, now corrected.
See [`phase2-pcapng-ie-replay-current-review-20260924.md`](../reviews/phase2-pcapng-ie-replay-current-review-20260924.md).

The bridge requires explicit FCS framing, interprets only raw 802.11 link type
105, and accounts for unsupported link types (including radiotap/127),
non-management frames, unsupported protocol versions, and unsupported
management subtypes. Supported malformed frames fail closed. Successful
results preserve raw MPDUs and packet/interface/timestamp provenance in
container order. All derived results remain private staging state until the
complete PCAPNG reader succeeds with a final receipt; an error/cancellation
returns no capture.

The public `Limits` can reduce but not raise the hard ceilings: 512 MiB input,
2,000,000 container blocks, 100,000 packets, 50,000 parsed frames, 50,000,000
aggregate parser work units, 256 MiB conservative retained logical bytes,
and the underlying parser's per-frame limits. Geometric vector capacity growth
is checked before allocation, including the old and new capacities during a
move. The result exposes distinct replay/parser release identifiers and the
PCAPNG decoder receipt; the IEEE record schema version is separate.

## Validation

Focused checks on the integrated current-main implementation passed:

- `CARGO_TARGET_DIR=/private/tmp/kyberia-phase2-ie-replay-20260923/target cargo test -p kyberia-pcap-ie-replay --locked --offline` — 14 unit tests passed; 0 doctests.
- `CARGO_TARGET_DIR=/private/tmp/kyberia-phase2-ie-replay-20260923/target cargo clippy -p kyberia-pcap-ie-replay --all-targets --locked --offline -- -D warnings` — passed.
- `cargo fmt --all -- --check` — passed.
- `python3 tools/architecture.py` — passed.
- `python3 tools/source_inventory.py check` — passed, 522 locked external packages.
- `python3 tools/ledger.py check` — passed, 5,396 source blocks, 438 explicit ID occurrences, 447 headings.
- `git diff --check` — passed.

Build artifacts were retained in the worktree-local `target/` directory. No
workspace-wide Cargo test or Clippy run was performed.

The synthetic PCAPNG tests exercise a real Beacon SSID IE, deterministic
ordering across capacity growth, timestamp and interface identity, link type
105 versus 127, valid and invalid FCS under the explicit policy,
non-management/protocol-version/unsupported-subtype accounting, malformed
supported frames and malformed trailing container blocks after staged
prefixes, exact packet/frame/work/retained-byte boundaries, pre-read and
reader-triggered cancellation, and cancellation triggered only after a valid
frame has been staged. The packet importer’s limit and all-or-error behavior
are checked without fixtures or mocks standing in for the parser.

This is not physical capture validation or desktop/product promotion. It does
not implement radiotap, live Kismet/capture integration, standards references,
identity graph, UI/IPC, channel scheduling, full Phase 2 deliverables, or
cross-platform runtime acceptance. `INS-005` and Phase 2 remain in progress.
