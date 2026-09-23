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
DLT_IEEE802_11 PCAP from three checked-in fixtures and invokes
`/usr/sbin/tcpdump` without a skip branch. Its original pre-review version
decoded the fixtures and asserted expected display strings independently; it
did not compare Kyberia's typed parser outputs against tcpdump's parsed fields.
The current review-corrected test performs those direct comparisons for the
stable fields listed in the latest rerun below. The assigned-worktree authority
was tcpdump 4.99.1 (Apple 161) with libpcap 1.10.1. Tcpdump is validation-only
and cannot become a runtime dependency. Exact commands, hashes, counters and
limitations are in `wifi-ie-fuzz-differential-run.json`.

Runtime record (2026-09-14, local macOS arm64 worktree):

- `cargo test -p kyberia-ieee80211 --locked --offline`: 28 passed, 0 failed,
  0 ignored after the final source revision.
- `cargo run -p kyberia-ieee80211 --example deterministic_mutation --locked
  --offline`: 619 deterministic mutation cases, exit 0.
- Pinned cargo-fuzz/libFuzzer campaign: 2,438,625 executions, 515 edges, 2,106
  features, 371 final in-memory corpus inputs, zero crashes/timeouts, exit 0.
- Retained campaign corpus: 373 files, 360,002 bytes, including 24 accepted
  canonical documents, 219 parsed frames, and one CRC-valid FCS-present frame.
- Explicit tcpdump/libpcap run: three fixture decodes and expected output
  strings passed, but this pre-review test did not compare typed parser fields
  directly; the independent review found that evidence gap.
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

## Superseded pre-review integration-candidate rerun (2026-09-23)

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
- The explicitly ignored tcpdump test decodes all three checked-in fixtures
  using tcpdump 4.99.1 (Apple 161)/libpcap 1.10.1. That test did not compare
  typed parser outputs against tcpdump's parsed fields; the independent review
  correctly rejected it as differential acceptance evidence. Retained PCAP,
  normalized output, and version output are under
  `.trash/test-runs/wifi-ie-tcpdump-88435-1790159155406696000/`; their hashes
  are in `wifi-ie-fuzz-differential-run.json`.
- `python3 tools/ledger.py check`, `python3 tools/architecture.py check`,
  `python3 tools/source_inventory.py check`, and `git diff --check main..HEAD`
  pass (5,396 source blocks; 522 locked external packages).

The prior 2,438,625-execution coverage-guided campaign remains bound to the
exact parser, fuzz library, target, and seed-contract hashes but was not rerun;
the pinned nightly and `cargo-fuzz` are unavailable in this environment. The
independent review of `7133e25725cb797ccce55da766d16da57d25b68d` found the
external-comparison gap and a cancellation-documentation mismatch. Both are
addressed in the correction below; re-review remains pending. This bounded
candidate does not complete `INS-005`, other WIFI items, Phase 2,
Kismet/physical capture, or broader TST-002 gates.

## Independent-review correction rerun (2026-09-23)

The exact code correction is commit `529f76b157044025d11f3dad63c0edf9f0ec8d9d`
on isolated integration branch `integrate/wifi-ie-main-v1`. The tcpdump test
now parses Kyberia's typed values and compares them directly with the
corresponding fields in each tcpdump output line. It compares management
subtype; receiver/destination, transmitter/source, and BSSID field; printable
or empty SSID display; supported rates when tcpdump emits them (Beacon and
Probe Request in this build, but not Probe Response); DS channel when shown;
and Beacon ESS/privacy flags. Hidden-versus-wildcard SSID meaning remains an
internal parser assertion because tcpdump renders both as an empty display.

The same candidate narrows the cancellation statement: per-IE rate/Country
value loops are each bounded by the one-byte IE length and do not poll inside
each value-copy iteration; cancellation arriving mid-IE is observed at the
next parser checkpoint. The parser source itself was not changed, preserving
the exact source-hash binding to the prior bounded libFuzzer campaign.

From code commit `529f76b` the following checks pass:

- `cargo test -p kyberia-ieee80211 --locked --offline`: 22 unit tests, three
  internal differential tests, and three compile-fail doctests pass.
- The explicitly gated tcpdump differential passes for three fixtures with
  direct typed-field comparisons using tcpdump 4.99.1 (Apple 161)/libpcap
  1.10.1. Retained PCAP, normalized output, and version artifacts are under
  `.trash/test-runs/wifi-ie-tcpdump-94905-1790172181428203000/`; SHA-256 values
  are recorded in `wifi-ie-fuzz-differential-run.json`.
- `cargo clippy -p kyberia-ieee80211 --locked --offline --all-targets -- -D
  warnings` and `cargo fmt --all -- --check` pass.
- `cargo run -p kyberia-ieee80211 --example deterministic_mutation --locked
  --offline` passes all 619 cases.
- The four parser/fuzz/target/seed-contract hashes still match the recorded
  campaign. That 2,438,625-execution campaign was not rerun because the pinned
  nightly and `cargo-fuzz` are unavailable.

Independent re-review of the exact corrected code/evidence candidate passed;
the report is `docs/reviews/wifi-ie-parser-correction-20260923.md` at review
commit `622452b6b68074d15c43b9374068fe7dbb449115`. Its sole remaining MINOR
finding was the stale `STATUS.md` summary row, now corrected in this integration
candidate. The reviewer independently passed direct `rustc` unit/differential
checks, explicit tcpdump comparisons, 619 mutations, doctests, formatting,
ledger, architecture, inventory, and artifact hash checks. The review's Cargo
test/Clippy invocation could not resolve uncached workspace dependency `mio`
offline; the candidate's recorded Cargo test/Clippy run remains separate
author-worktree evidence. The pinned nightly and `cargo-fuzz` remain unavailable,
so the coverage-guided campaign was not rerun. No Phase 2 exit, `INS-005`, other
WIFI backlog item, Kismet/physical capture, or broader TST-002 gate is closed.

## Mainline promotion verification (2026-09-23)

After promotion, the exact `main` revision `1d8e68fada38bf63222ced87cd0ff78ed22181ae`
(tree `da0c01ee4fb1718f71de482900bcd345d328db67`) passed the same focused
locked/offline package tests (22 unit, 3 fixture-differential, 3 compile-fail
doctests), strict package Clippy, workspace formatting check, explicit
three-fixture tcpdump typed-field differential, 619-case deterministic mutation
run, ledger, architecture, source-inventory, and whitespace checks. The explicit
tcpdump artifacts are retained at
`.trash/test-runs/wifi-ie-tcpdump-29392-1790188232992388000/`; their PCAP,
normalized output, and version hashes match the corrected candidate evidence.
This is focused local validation, not a full workspace build or cross-platform
run; the coverage-guided campaign remains prior exact-hash-matched evidence.
