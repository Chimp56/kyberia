# Historical independent WIFI-001 parser review

> **Historical scope only:** this packet reviews candidate author tree
> `d97934372943abb2f60760086db0d2e062a396b6` on its old base `84d85ac`.
> It is retained as the original rejection and correction record, not as
> approval of the later adaptation based on `84a5bcb96e61d2703b648cd817e97fd68b92086a`
> or integration candidate `7133e25725cb797ccce55da766d16da57d25b68d` based on
> `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`. The correction response below
> describes that old candidate only. The current integration candidate still
> requires a fresh independent review before integration to `main`.

## Immutable scope and verdict

Reviewer candidate `c8cbea8` is tree-equivalent to author candidate
`d97934372943abb2f60760086db0d2e062a396b6`; both have tree
`8ee2fd5281a9661fd8bda4e133b80d07ebc9361b` on base `84d85ac`.

**CHANGES_REQUIRED: 0 BLOCKER, 4 MAJOR, 1 MINOR.** The slice correctly remains
`IN_PROGRESS`: no coverage-guided fuzz, external Wireshark/TShark differential,
physical capture, collector wiring, or later WIFI semantic acceptance is
claimed. The byte framing, explicit FCS choice, CRC implementation, response
fixed fields, IE offsets/order, extension-ID framing, context-sensitive empty
SSID, raw rates/DS projection, repeat summary, and unknown/vendor preservation
are otherwise appropriately narrow.

## MAJOR — DS flags are accepted while address roles are presented as precise

`parse_with` validates protocol version, frame type and subtype at
`crates/ieee80211/src/lib.rs:266-281`, but never checks the To DS and From DS
bits in the second frame-control octet. It then unconditionally publishes
Address 1 as receiver/destination, Address 2 as source/transmitter and Address 3
as the BSSID field at lines 373-377.

For the supported PV0 management frames those DS bits are zero. Accepting one
or both bits set makes malformed arbitrary bytes look like a valid normalized
Beacon/Probe record whose advertised address-role contract is authoritative.
The retained reviewer harness demonstrated that Probe Request inputs with
flag octets `0x01`, `0x02`, and `0x03` all return success.

Required correction: reject nonzero To DS or From DS before deriving address
roles, with a closed error that preserves no normalized frame. Add direct tests
for each bit and both bits on every supported subtype, plus control cases proving
other deliberately retained flag bits do not shift header/fixed/IE offsets.

## MAJOR — Country and TIM typed views accept invalid length grammar

The Country decoder at `crates/ieee80211/src/lib.rs:534-560` decides padding
only from `(payload.len() - 3) % 3`. It does not require the complete Country
information field to be even length. It therefore accepts both an unnecessary
pad after one triplet (payload length 7) and an unpadded two-triplet field
(payload length 9), even though Country padding exists only to make the element
length even. The reviewer harness observed the length-7 input becoming a typed
`CountryStructure` with `padding: Some(0)`.

The TIM decoder at lines 500-513 checks only its lower bound. A legacy PV0 TIM
information field is length 4 through 254, but a 255-byte field is returned as
a typed `TimStructure`; the reviewer harness reproduced that exact success.
These are not lossless-malformed retention problems—the crate already has
`ElementDecode::Malformed`—but false positive semantic projections.

Required correction: admit Country only at the minimum six-byte information
field with one or more complete triplets and exact even-length zero padding
when needed; reject/mark malformed every odd length, missing required pad,
unnecessary pad, nonzero pad, and residual byte. Bound TIM to 4..=254. Add
boundary tables covering Country lengths 5/6/7/8/9/10/11/12 and TIM lengths
3/4/254/255, while proving raw payload bytes and ordering remain lossless for
every malformed typed element.

## MAJOR — The validated normalized type can be forged in safe Rust

`ManagementFrame` and every one of its fields are public at
`crates/ieee80211/src/lib.rs:167-181`; nested `InformationElement` and repeat
summaries are also publicly constructible and mutable. The test at lines
903-905 proves only that a caller mutation is rejected if the caller later
chooses `canonical_bytes()`. Nothing requires an in-memory consumer to invoke
that serializer before trusting `subtype`, fixed fields, addresses, decoded
IEs, offsets, repeats, or `schema_version`.

This breaks the type-level distinction between parser-validated evidence and
an arbitrary caller-authored projection. The absence of Serde derives does not
close ordinary struct construction or mutation.

Required correction: make the validated frame's fields private and expose
read-only accessors. Keep construction confined to parse/canonical decode.
Nested values may remain ordinary value types only if callers cannot insert
them into or mutate a validated frame. Add compile-fail coverage for direct
construction/field mutation and runtime tests showing canonical decoding is the
only public reconstruction path.

## MAJOR — The advertised work cap is not cumulative across canonical operations

`parse_with` creates a fresh private `Control` and enforces
`max_work_units`. `canonical_bytes_with` calls that parser once, then allocates
and copies the complete canonical record at lines 399-429 without charging the
copy to the work cap. `from_canonical_bytes_with` parses once at line 467 and
then calls `canonical_bytes_with` at line 468, which reparses under a newly
reset counter and performs another uncharged copy. A successful canonical
decode can therefore consume multiple full per-pass budgets, and encoding or
decode verification temporarily retains the caller/first decoded frame plus a
second complete parsed frame. `max_ie_payload_bytes` does not account for this
duplicated derived/repeat structure.

The default byte/count caps keep the present implementation finite, and
cancellation remains live, but `max_work_units` is not the hard operation
boundary its public API and validation claims imply.

Required correction: carry one checked work/allocation admission across the
entire selected public operation, or make an unforgeable parsed type remove the
need for a second decoded model and validate the compact canonical envelope
without resetting work. Charge canonical output before allocation/copy and
either measure full retained/scratch structure or publish a checked aggregate
bound derived from byte and element caps. Add exact-limit and limit-minus-one
tests for parse, canonical encode, and canonical decode; cancellation tests in
CRC, TLV, repeat grouping, canonical copy and canonical replay; and a maximum
element/payload case proving peak retained/scratch admission.

## MINOR — An exhaustive-sounding test name covers a small deterministic matrix

`every_bounded_byte_string_is_panic_free` at
`crates/ieee80211/src/lib.rs:1019-1030` covers 3,104 deliberately generated
inputs, not every byte string of lengths 0 through 96. The validation document
describes the matrix honestly, so this is a naming defect rather than a product
claim. Rename it to identify the deterministic matrix and retain the explicit
statement that coverage-guided fuzzing remains open.

## Evidence and gates

- `cargo test -p kyberia-ieee80211 --locked --offline`: **PASS**, 19/19.
- `cargo run -p kyberia-ieee80211 --example deterministic_mutation --locked
  --offline`: **PASS**, exactly 619 cases.
- Full serialized `cargo test --workspace --locked --offline --
  --test-threads=1`: **PASS**, zero failures with the repository's explicit
  ignored runtime/benchmark cases retained.
- Strict crate Clippy with all targets and `-D warnings`: **PASS**. Strict full
  workspace Clippy with all targets and `-D warnings`: **PASS**.
- `cargo fmt --all -- --check`, architecture, source inventory and diff checks:
  **PASS**. Inventory contains 522 locked external packages; the dependency-free
  new crate adds no external package. Cargo.lock SHA-256 and inventory digest
  both equal `dc905f28fc15a873961493c170c1cf95d5e96cdb7745aa0e8909111a5f2de521`.
- Independent reviewer harness retained under
  `.trash/test-runs/wifi-ie-review-adversarial-20260914/`: all three nonzero DS
  flag combinations were accepted, invalid Country length 7 became typed, and
  TIM length 255 became typed.

Fresh graph project `kyberia-wifi-ie-parser-review`, generation
`2026-09-14T20:24:30Z`, contains 11,267 nodes and 59,222 edges with zero skipped
and zero partial files. The crate source/config/tests metadata-match with no
recorded issue. The bounded crate scope reports only the deliberately excluded
example directory, which was read and executed directly. Cargo.lock, tools,
docs and fixture bytes excluded by design were read directly. Coverage is
best-effort and is not proof of standards conformance.

## Ten-field handoff

1. Scope/outcome: independent review of the bounded WIFI-001 foundation;
   changes required with four MAJOR and one MINOR finding.
2. Commit/base: reviewer `c8cbea8`, author `d979343`, shared tree
   `8ee2fd5281a9661fd8bda4e133b80d07ebc9361b`, base `84d85ac`.
3. Files: all fourteen candidate paths reviewed; reviewer changes only this
   chronological rejection packet.
4. Design/invariants: explicit FCS provenance, CRC and lossless IE ordering are
   sound; DS validity, typed TIM/Country grammar, validated-type closure and
   cumulative resource admission require correction.
5. Tests/results: parser 19/19, mutation 619, full workspace and static gates
   pass; passing tests do not cover the reproduced invalid-success branches.
6. Runtime/external gates: no physical capture, Wireshark/TShark differential,
   coverage-guided fuzz campaign or cross-platform runtime is claimed; WIFI-001
   correctly remains in progress.
7. Graph/coverage: generation `2026-09-14T20:24:30Z`, 11,267 nodes, 59,222
   edges, zero skipped/partial; excluded example/docs/lock/fixtures read directly.
8. Routing/status: reviewed cherry-pick was clean and tree-equivalent; review
   and integration worktrees were clean before this packet edit.
9. Docs/traceability: candidate changes architecture/validation/source ledger
   and inventory only; reviewer does not edit STATUS, implementation ledger or
   generated TRACEABILITY.
10. Reviewer focus: correct all invalid-success and forgeability branches, then
    prove one cumulative work/allocation boundary without expanding into
    WIFI-002 through WIFI-007 or claiming external fuzz/differential acceptance.

## Author correction response (2026-09-14)

The follow-up commit after this immutable review packet rejects every nonzero
ToDS/FromDS combination before constructing address roles; implements exact
legacy Country even-length/zero-pad grammar and the TIM 4..=254 bound while
retaining malformed raw IEs; and makes the complete normalized frame graph
read-only to external safe Rust, with compile-fail evidence for direct mutation.

Each parse, canonical encode, or canonical replay now uses one cumulative work
and logical-allocation admission. Encoding relies on the unforgeable parsed
type instead of reparsing a duplicate model; replay validates the closed compact
envelope and reparses once under the same control. Exact/limit-minus-one,
maximum-frame payload/scratch, and CRC/TLV/grouping/copy/replay cancellation
tests cover the corrected boundary. The deterministic mutation matrix remains
described only as regression evidence; no coverage-guided fuzzing, external
parser truth, or WIFI-002 through WIFI-007 completion is claimed.

Correction gates: the crate suite passes 22 unit, 3 differential, and 3
compile-fail doctests; the mutation harness reports exactly 619 cases; the full
serialized workspace suite, strict crate/workspace Clippy, rustfmt,
architecture, source inventory (522 locked packages), and diff checks pass.
The parent-owned ledger diagnostic retains its pre-existing stale Cargo.lock
and source-ledger evidence digests and was not rewritten in this slice. Fresh
graph project `kyberia-wifi-ie-parser-correction`, generation
`2026-09-14T20:46:39Z`, has 11,288 nodes and 59,446 edges with zero skipped or
partial files; parser source/tests metadata-match, while docs and the executed
example remain deliberately excluded and were read directly.
