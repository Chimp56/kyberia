# Independent WIFI-001 current-main parser review

## Candidate and scope

- Verdict: **CHANGES REQUIRED**; do not integrate/promote this candidate until
  the MAJOR finding below is corrected and independently re-reviewed.
- Exact review checkout: `/private/tmp/kyberia-wifi-ie-review`, branch
  `review/wifi-ie-current-main`.
- Candidate commit: `bab113dbabc7515eda5becb9a549c8395fbc1901`.
- Candidate tree: `ad4c61ca2d7783b2bf041f4a6bc689c3b724f494`.
- Comparison base: `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`.
- Parser implementation commit in candidate history:
  `7133e25725cb797ccce55da766d16da57d25b68d`.
- Requirement reviewed: `plan.md` line 4252, `WIFI-001` (Beacon/probe IE
  parser with fuzzed and differential fixtures).
- Reviewed parser, tests, fuzz target and corpus, evidence manifests, docs,
  architecture/dependency declaration, source inventory/license records,
  `STATUS.md`, traceability and ledger. No implementation files were edited.

## Findings

### MAJOR — External differential test does not compare parsed IE semantics

`crates/ieee80211/tests/tcpdump_differential.rs:73-79` calls Kyberia's parser
but compares only the frame subtype to a local expected enum. Lines 103-113
then assert that tcpdump's output contains hard-coded expected strings; those
assertions are not compared with `ManagementFrame::elements()` or its typed
`ElementDecode` values. The checked-in internal oracle in
`crates/ieee80211/tests/differential.rs:11-33, 50-78` independently checks
addresses and raw IE `(id, payload)` slices, but does not check typed SSID,
rates, DS, TIM, Country, or extension decoding against an independent oracle.

This means the current run proves tcpdump independently decoded the fixture
strings, but does not prove Kyberia's IE interpretation agrees with tcpdump.
The current claims that the test compares SSID/rates/channel/capability fields
are unsupported (`docs/architecture/ieee80211-parser.md:65-72` and
`docs/validation/wifi-ie-parser.md:36-44`). This weakens the named
`WIFI-001` differential acceptance evidence. Add actual comparisons between
the candidate parser's stable decoded fields and the oracle output (or narrow
the test/docs claims and provide a separate independent typed-IE oracle), then
rerun and refresh evidence hashes. Independent re-review is required.

### MINOR — Typed-decoder cancellation is not polled within decoder loops

`crates/ieee80211/README.md:22-23` says cancellation is polled throughout
typed-decode loops. In `crates/ieee80211/src/lib.rs:788-794`, the decoder polls
once before dispatch; the per-rate loop at lines 846-864 and Country triplet
loop at lines 891-900 do not poll again. Their inputs are individually bounded
by the one-byte IE length, so this is not an unbounded-work issue, but a
cancellation request arriving during either loop is only observed after that
IE finishes. Either poll within the loops and test mid-loop cancellation, or
make the documented guarantee precise. This does not change the MAJOR verdict.

## Independent commands and observations

In the assigned review tree, I independently ran:

- `rustc --edition=2024 --test crates/ieee80211/src/lib.rs -o target/ieee80211-unit-tests`
  followed by `target/ieee80211-unit-tests` — 22 passed.
- `rustc --edition=2024 --crate-name kyberia_ieee80211 --crate-type lib crates/ieee80211/src/lib.rs -o target/libkyberia_ieee80211.rlib`
  followed by
  `CARGO_MANIFEST_DIR="$PWD/crates/ieee80211" rustc --edition=2024 --test crates/ieee80211/tests/differential.rs --extern kyberia_ieee80211=target/libkyberia_ieee80211.rlib -o target/differential-tests`
  and `target/differential-tests` — 3 passed.
- `CARGO_MANIFEST_DIR="$PWD/crates/ieee80211" rustc --edition=2024 --test crates/ieee80211/tests/tcpdump_differential.rs --extern kyberia_ieee80211=target/libkyberia_ieee80211.rlib -o target/tcpdump-differential-test`, then
  `target/tcpdump-differential-test --ignored --exact checked_in_management_frames_match_tcpdump_stable_fields --nocapture`
  — passed against tcpdump 4.99.1 (Apple 161)/libpcap 1.10.1.
- `rustc --edition=2024 --deny warnings --crate-name kyberia_ieee80211 --crate-type lib crates/ieee80211/src/lib.rs -o target/libkyberia_ieee80211.rlib`
  and `rustc --edition=2024 --deny warnings --crate-name deterministic_mutation crates/ieee80211/examples/deterministic_mutation.rs --extern kyberia_ieee80211=target/libkyberia_ieee80211.rlib -o target/deterministic-mutation`, then
  `target/deterministic-mutation` — 619 cases passed.
- `rustdoc --edition=2024 --test --crate-name kyberia_ieee80211 crates/ieee80211/src/lib.rs -L dependency=target --extern kyberia_ieee80211=target/libkyberia_ieee80211.rlib --out-dir target/doctests-bound --test-args --nocapture`
  — 3 compile-fail tests passed with the crate linked.
- `cargo fmt --all -- --check`; `python3 tools/ledger.py check`,
  `python3 tools/architecture.py check`, `python3 tools/source_inventory.py check`,
  `python3 -m json.tool docs/validation/wifi-ie-fuzz-differential-run.json`,
  and `git diff --check`
  — passed (5,396 source blocks, 522 locked external packages).
- SHA-256 checks confirmed the candidate parser
  (`b05accc08b4fd202b258e46dcc59aba10c3f856fcbab26a3db0b46bf998af5ff`), fuzz
  library (`7829f182b83838e98bd265bbc0024acb1c1e9c8f405a8a537f90797ea3d58141`),
  fuzz target (`b313f272adfdd3b9d0ca428939aac08dd363c1e853a43173eacb9fab4a296a5b`),
  and seed contract (`0c0d0035bed4ae9bb50fa450221d1e9ff92561f1e74ee66748ec96bcbc4474af`)
  match the recorded campaign source hashes. My fresh tcpdump PCAP,
  normalized output and version hashes also match the candidate evidence
  manifest: `533a525eaa804856198f24182525d029d320c36a8cf312cbf537eaf69309740b`,
  `99d170422ce4e618a47eff939ab310a6c30d3276d7052a99a7a0a1cbe00da13a`, and
  `b700fd67878107ec1cd0a87902891548adc3e24bca353b9bb80ada30b51cf7ee`.

`cargo test -p kyberia-ieee80211 --locked --offline` could not independently
resolve this checkout's whole Cargo workspace: its local Cargo cache lacks
`mio`, required by the unrelated `kyberia-active-measurement` workspace member.
I did not retry online. The direct source-level tests above are supplementary;
the integration-candidate Cargo results recorded by the author are not an
independent Cargo rerun by this reviewer. Strict Cargo Clippy was likewise not
independently rerun.

The codebase-memory graph tools were unavailable in this session, so this
review uses source-level evidence only and makes no graph/coverage claim. The
coverage-guided campaign was not rerun here; its source hashes match the
candidate and its prior bounded results remain historical evidence. Nothing in
this review claims `WIFI-001`, Phase 2, or broader plan completion.

## Required follow-up

Fix the MAJOR differential-test/evidence gap, update the evidence and any
affected traceability references, and request a fresh independent review on
the exact revised candidate. The MINOR cancellation/documentation mismatch
should be resolved in the same follow-up if practical. No other BLOCKER or
MAJOR parser-correctness finding was identified in this bounded review.
