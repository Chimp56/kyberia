# Independent WIFI-001 correction re-review

## Scope and verdict

**PASS for the bounded parser/test correction, with one MINOR status-table
follow-up.** The prior MAJOR typed-parser-to-tcpdump evidence gap and MINOR
cancellation-guarantee wording mismatch are both resolved. The bounded increment
is suitable for review integration once the stale `STATUS.md` row below is
updated alongside the review metadata. No further parser re-review is needed
for that status-only correction.

- Requirement: `plan.md` §18.3, `WIFI-001` at line 4252: Beacon/probe IE
  parser with fuzzed and differential fixtures.
- Exact checkout: `/private/tmp/kyberia-wifi-ie-rereview-20260923`, branch
  `review/wifi-ie-parser-correction-20260923`.
- Exact reviewed candidate: evidence commit
  `6c32224d3e457b87115a8e2a13b8cd7d932bc98e`, tree
  `17af1f7c46c2592cdc508d50656a51c79a32814e`.
- Candidate code correction: `529f76b157044025d11f3dad63c0edf9f0ec8d9d`.
- Comparison base: `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`.
- Re-reviewed prior findings in
  `docs/reviews/wifi-ie-parser-current-main-review.md`, the correction diff,
  parser/test source, README and architecture notes, validation markdown/JSON,
  architecture/dependency declarations, supply-chain inventory, `STATUS.md`,
  `TRACEABILITY.md`, and the WIFI-001 ledger entry.

## Resolution of prior findings

### Prior MAJOR: direct typed-parser-to-tcpdump comparison — RESOLVED

The corrected `crates/ieee80211/tests/tcpdump_differential.rs` parses all three
fixtures once and compares each parsed record to its corresponding tcpdump line
(lines 174-180 and 216-263): subtype, receiver/destination,
transmitter/source, BSSID field, SSID display, channel presence/value, and
Beacon ESS/privacy flags. Where tcpdump emits a rate list, it compares that
list to the parser's combined Supported Rates/Extended Supported Rates values
(lines 74-92, 242-249). Fixture-level exact output assertions remain as a
separate sanity check, not as the asserted differential itself.

Scope is now explicit and accurate. The docs state that the chosen printable or
empty SSIDs are compared by display, while hidden-versus-wildcard meaning is
checked internally because tcpdump renders both as empty; rates are compared
only on frames where this tcpdump build emits them; comparison is tcpdump/libpcap
only, host-specific, and does not establish Wireshark/TShark, hardware,
collector, cross-platform, PCAPNG, or advanced-semantics parity. The evidence
JSON labels the earlier 7133e25 run as pre-review/decode-only and records zero
typed comparisons for that run; the corrected run is separately identified at
code revision `529f76b...`. I found no overstatement of the corrected external
comparison's scope in the reviewed README, architecture note, validation note,
or JSON.

The correction preserves the earlier fuzz binding. The four checked-in source
hashes exactly match the prior campaign manifest:

- parser: `b05accc08b4fd202b258e46dcc59aba10c3f856fcbab26a3db0b46bf998af5ff`
- fuzz library: `7829f182b83838e98bd265bbc0024acb1c1e9c8f405a8a537f90797ea3d58141`
- fuzz target: `b313f272adfdd3b9d0ca428939aac08dd363c1e853a43173eacb9fab4a296a5b`
- seed contract: `0c0d0035bed4ae9bb50fa450221d1e9ff92561f1e74ee66748ec96bcbc4474af`

The source itself was not modified by the code correction. The earlier bounded
coverage-guided campaign remains historical hash-matched evidence, not a rerun
of this candidate; the docs state this, and do not imply fuzzing is exhaustive.

### Prior MINOR: cancellation guarantee vs bounded IE loops — RESOLVED

`crates/ieee80211/README.md` and `docs/architecture/ieee80211-parser.md` now
say cancellation is checked at typed-IE dispatch/allocation/parser
checkpoints, while per-IE decoding can process up to the one-byte IE payload
maximum (255 bytes) without polling inside its value-copy loop. They explicitly
state that cancellation arriving mid-IE is observed at the next parser
checkpoint. This matches `decode_element`, `decode_rates`, and `decode_country`
in `crates/ieee80211/src/lib.rs`; the implementation makes no mid-IE
interruptibility promise. The bounded worst-case behavior is stated without a
wall-clock guarantee.

## Remaining MINOR — stale STATUS summary row

`STATUS.md:37-41` correctly names correction `529f76b` and says re-review is
pending. However, the summary table at `STATUS.md:175` still names `7133e25` as
the integration candidate and describes its old TCPDump acceptance evidence.
That row is stale relative to the corrected code/evidence candidate
`529f76b`/`6c32224`. Update the summary row before integrating this review so
the authoritative status view points at the reviewed candidate and corrected
direct-comparison evidence. This is documentation/traceability only; it does
not invalidate the parser or test results and does not require another parser
re-review.

The WIFI-001 ledger row remains `IN_PROGRESS`, correctly scopes its own claims,
and references the corrected validation artifacts. The review list is empty
before this report is integrated, as expected. `TRACEABILITY.md` likewise
keeps this requirement in progress. `STATUS.md` continues to leave Phase 2,
`INS-005`, other WIFI items, Kismet/physical capture, and broader TST-002 gates
open; no broader completion claim is present.

The crate remains dependency-free in the release workspace; the fuzz project
is separately isolated. `tools/architecture.py check` and
`tools/source_inventory.py check` pass, with 522 locked external packages.
Cargo.lock and the source-inventory boundary are unchanged by this correction.

## Independent verification

In this review worktree I independently ran:

- `rustc --edition=2024 --test crates/ieee80211/src/lib.rs -o target/ieee80211-unit-tests`
  and the resulting binary — 22 unit tests passed.
- Direct `rustc` build/link/run of `crates/ieee80211/tests/differential.rs`
  against the compiled crate — 3 differential tests passed.
- Direct `rustc` build/link of `crates/ieee80211/tests/tcpdump_differential.rs`
  and explicit `--ignored --exact checked_in_management_frames_match_tcpdump_stable_fields`
  execution — passed; stdout showed Beacon, Probe Request and Probe Response
  and all newly wired direct comparisons succeeded against tcpdump 4.99.1
  (Apple 161)/libpcap 1.10.1.
- Direct deterministic-mutation example — 619 cases passed.
- Linked `rustdoc --test` compile-fail suite — 3 passed.
- `cargo fmt --all -- --check`, direct crate and tcpdump-test compilation with
  `rustc --deny warnings`, ledger check (5,396 source blocks), architecture
  check, source-inventory check (522 packages), evidence JSON parse, and
  `git diff --check` — passed.
- Fresh tcpdump artifacts generated in this review worktree match the recorded
  corrected run hashes: PCAP
  `533a525eaa804856198f24182525d029d320c36a8cf312cbf537eaf69309740b`, output
  `99d170422ce4e618a47eff939ab310a6c30d3276d7052a99a7a0a1cbe00da13a`, version
  `b700fd67878107ec1cd0a87902891548adc3e24bca353b9bb80ada30b51cf7ee`.

`cargo test -p kyberia-ieee80211 --locked --offline` and strict Cargo Clippy
could not resolve the full workspace because this isolated Cargo cache lacks
`mio`, required by unrelated workspace member `kyberia-active-measurement`.
No online retry was attempted. Parent-provided exact-code-commit Cargo test,
strict Clippy and fmt results are consistent with these independent focused
checks, but only the direct rustc/differential runs above are independently
verified here. The pinned nightly and `cargo-fuzz` are unavailable; the
coverage-guided campaign was not rerun. Graph MCP tools were unavailable, so
this report makes no graph or coverage claim.

## Ten-field handoff

1. **Verdict/scope:** PASS for bounded WIFI-001 correction; one non-blocking
   status-table update is requested before review metadata integration.
2. **Plan anchor:** `plan.md` §18.3, line 4252, WIFI-001 only.
3. **Exact commits:** candidate `6c32224d3e457b87115a8e2a13b8cd7d932bc98e`,
   tree `17af1f7c46c2592cdc508d50656a51c79a32814e`, code correction
   `529f76b157044025d11f3dad63c0edf9f0ec8d9d`, base
   `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`.
4. **Reviewed scope:** parser-to-tcpdump comparison, correction docs/evidence,
   hash binding, architecture/dependency/supply-chain boundary, STATUS,
   traceability and ledger; no implementation/docs edits.
5. **Prior findings:** typed-field comparison and explicit limitations fixed;
   cancellation wording now matches bounded per-IE polling behavior.
6. **Independent results:** 22 unit, 3 internal differential, 1 explicit
   tcpdump differential, 619 mutations, 3 doctests, fmt, ledgers/architecture/
   source inventory and hash checks passed; Cargo resolver limitation disclosed.
7. **Status/evidence:** WIFI-001 stays `IN_PROGRESS`; broader phase and product
   gates stay open; one stale `STATUS.md:175` row needs updating.
8. **Review artifact:** this report is the only file this reviewer will add;
   it will be committed separately in the assigned review worktree.
9. **Residual risks:** no coverage-guided rerun, cross-platform run, Wireshark/
   TShark/hardware/collector parity, or advanced WIFI semantic acceptance.
10. **Blockers/limitations:** full Cargo checks cannot resolve uncached `mio`
    offline; no external state or user input is needed to continue the bounded
    engineering work.
