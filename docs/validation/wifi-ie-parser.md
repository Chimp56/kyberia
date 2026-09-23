# WIFI-001 foundational parser validation

The crate suite covers Beacon, Probe Response, and Probe Request fixtures;
address roles; exact header/fixed-field truncation; explicit validated FCS;
ordered, repeated, vendor, unknown, and extension IEs; binary/empty SSIDs;
rates; DS; TIM; Country; malformed typed structures; broken TLVs; singleton
conflicts; exact Country lengths 5 through 12 and TIM lengths 3/4/254/255;
all ToDS/FromDS combinations; immutable public evidence; cumulative exact and
limit-minus-one resource admission; phase-specific cancellation; maximum-frame
payload/scratch admission; canonical tampering and exact round trips. A
deterministic arbitrary-byte matrix checks 3,104 bounded inputs under
`catch_unwind`.

`tests/differential.rs` uses an independently written minimal fixture oracle to
compare header address fields and raw TLV slicing. It is independence evidence
inside Kyberia, not external Wireshark/TShark or hardware truth.

`examples/deterministic_mutation.rs` is a dependency-free reusable parser
entrypoint over three checked-in seed corpora. Its current deterministic run
executes every truncation and three one-byte XOR mutations at every seed
position. This remains mutation regression evidence only.

The separately pinned `fuzz/` project now supplies a genuine coverage-guided
libFuzzer target over parse, arbitrary canonical replay, and successful
parse/canonical/replay equality under three fixed resource profiles. The first
review-correction campaign executed 2,438,625 units in 61 seconds, grew six
reviewed seeds to a 371-input in-memory corpus, reached 515 reported edges and
2,106 features, and finished with no crash or timeout. The retained filesystem
corpus has 373 files because it is measured after process exit rather than
conflated with libFuzzer's in-memory counter. Generated corpora and the complete
final-stat stream are retained under the worktree's ignored `.trash/test-runs/`;
the six seeds, target, reachability contract, deterministic corpus inspector,
lock, toolchain pin, tracked final-stat excerpt, and ten-package source
inventory are checked in.

The explicit `tcpdump_differential` test constructs a deterministic
DLT_IEEE802_11 PCAP from three checked-in fixtures, invokes `/usr/sbin/tcpdump`
without a skip branch, and compares only fields emitted stably by both parsers:
management subtype, three address roles, SSID display, rates, channel and
selected capability flags. The original accepted local authority was tcpdump
4.99.1 (Apple 158) with libpcap 1.10.1. The assigned-worktree rerun used
tcpdump 4.99.1 (Apple 161) with libpcap 1.10.1. Tcpdump is validation-only and cannot become a
runtime dependency. Exact commands, hashes, counters and limitations are in
`wifi-ie-fuzz-differential-run.json`.

Runtime record (2026-09-14, local macOS arm64 worktree):

- `cargo test -p kyberia-ieee80211 --locked --offline`: 28 passed, 0 failed,
  0 ignored after the final source revision.
- `cargo run -p kyberia-ieee80211 --example deterministic_mutation --locked
  --offline`: 619 deterministic mutation cases, exit 0.
- Pinned cargo-fuzz/libFuzzer campaign: 2,438,625 executions, 515 edges, 2,106
  features, 371 final in-memory corpus inputs, zero crashes/timeouts, exit 0.
- Retained campaign corpus: 373 files, 360,002 bytes, including 24 accepted
  canonical documents, 219 parsed frames, and one CRC-valid FCS-present frame.
- Explicit tcpdump/libpcap differential: three fixture decodes and all closed
  shared-field comparisons pass, exit 0.
- This is the corrected bounded WIFI-001 acceptance candidate and remains
  promotion-gated on independent re-review. Physical capture, Kismet/Wireshark parity,
  cross-platform campaigns and broader TST-002 parser families remain open.

## Author-worktree adaptation rerun (2026-09-23)

On base `84a5bcb96e61d2703b648cd817e97fd68b92086a`, the isolated adaptation
passes 22 unit tests, three checked-in fixture differential tests, and three
compile-fail doctests. The explicitly ignored three-frame tcpdump differential
also passes on Apple tcpdump 4.99.1 build 161/libpcap 1.10.1; retained PCAP,
normalized output, version output, and hashes are recorded in
`wifi-ie-fuzz-differential-run.json` and `.trash/test-runs/`.

The reusable deterministic mutation runner still passes all 619 cases. The
separately recorded 2,438,625-execution libFuzzer campaign is prior evidence,
not a rerun on this worktree; its parser, fuzz library, target, and seed-contract
SHA-256 values match this adaptation exactly. A new coverage-guided campaign
was not run here because this environment has neither the pinned nightly nor
`cargo-fuzz`. Independent review of this assigned-worktree adaptation remains
required. No Phase 2 exit, `INS-005`, other WIFI backlog item, Kismet/physical
capture, or broader TST-002 completion is implied.

## Latest-main integration-candidate rerun (2026-09-23)

The author commit was cherry-picked without conflict onto integration base
`05953134d24666e8483cbfdb7d9aacd0ce4e6e48`, producing candidate
`7133e25725cb797ccce55da766d16da57d25b68d`. From that exact isolated tree:

- `cargo test -p kyberia-ieee80211 --locked --offline`: 22 unit tests, three
  internal differential tests, and three compile-fail doctests pass; the
  separately gated tcpdump test is intentionally ignored in the default run.
- `cargo clippy -p kyberia-ieee80211 --all-targets --locked --offline -- -D
  warnings` and `cargo fmt --all -- --check` pass.
- `cargo run -p kyberia-ieee80211 --example deterministic_mutation --locked
  --offline` passes all 619 cases.
- The explicitly ignored tcpdump differential passes for all three checked-in
  fixtures using tcpdump 4.99.1 (Apple 161)/libpcap 1.10.1. Retained PCAP,
  normalized output, and version output are under
  `.trash/test-runs/wifi-ie-tcpdump-88435-1790159155406696000/`; their hashes
  are in `wifi-ie-fuzz-differential-run.json`.
- `python3 tools/ledger.py check`, `python3 tools/architecture.py check`,
  `python3 tools/source_inventory.py check`, and `git diff --check main..HEAD`
  pass (5,396 source blocks; 522 locked external packages).

The prior 2,438,625-execution coverage-guided campaign remains bound to the
exact parser, fuzz library, target, and seed-contract hashes but was not rerun;
the pinned nightly and `cargo-fuzz` are unavailable in this environment.
Independent review of candidate `7133e25725cb797ccce55da766d16da57d25b68d`
is pending. This bounded candidate does not complete `INS-005`, other WIFI
items, Phase 2, Kismet/physical capture, or broader TST-002 gates.
