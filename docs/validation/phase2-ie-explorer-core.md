# Phase 2 information-element explorer core increment

This isolated source increment adds a zero-copy `ManagementFrame::ie_explorer`
view and a bounded `diff_information_elements` API to
`crates/ieee80211/src/lib.rs`. The view exposes ordered IE entries, each exact
raw TLV and payload, the current typed `ElementDecode`, and malformed or
repeated-singleton evidence, including identical duplicates, and contradictory
repeat evidence. The numeric standards identity is the IEEE
802.11 element identifier plus an extension identifier when the IE uses the
extension element ID. It is an identifier only: this increment adds no clause
citations, registry display names, or external standards links. An IE the
decoder does not interpret retains that numeric identity, its raw bytes, and
`ElementDecode::Unknown`.

The diff pairs entries by `(element ID, extension ID, zero-based occurrence)`
and compares raw payload bytes, so duplicate elements are not collapsed.
`wire_ie_sequence_changed` separately reports an ordered raw IE sequence
difference, including a reorder-only difference for which the content change
list is empty. The before/after explorer views retain their original order and
the diff reports a management-subtype change separately. Diff indexing uses
bounded vectors, an in-place heap sort, checked allocation/work budgets, and
cancellation checkpoints during sorting and comparison.

The parser currently accepts Beacon, Probe Request, and Probe Response
management frames. Association Request/Response parsing is not implemented.
This core increment does not add desktop Lab UI or IPC, registered element
names, standards clause/help links, or full `INS-005` acceptance. The complete
INS-005 requirement and Phase 2 remain in progress.

## Validation

On base `bc51e80b14e30f927628f4ba9f2e92a4773423fe` in isolated branch
`feat/phase2-ie-explorer-current`:

- `cargo test -p kyberia-ieee80211 --locked --offline`: 31 unit tests, 3
  fixture differential tests, and 3 compile-fail doctests passed; one explicit
  tcpdump differential test remains intentionally ignored by the default run.
- `cargo clippy -p kyberia-ieee80211 --locked --offline --all-targets -- -D
  warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `python3 tools/ledger.py check`, `python3 tools/architecture.py check`,
  `python3 tools/source_inventory.py check`, and `git diff --check`: passed
  (5,396 source blocks and 522 locked external packages).

The added regressions cover no change, payload modification, addition, removal,
reorder-only differences, duplicate occurrence modification/addition,
malformed, contradictory, and identical-singleton cardinality warnings,
unknown and extension identities,
subtype context change, exact work/allocation limits, cancellation, and a diff
at the default 1,024-element boundary. These are core API tests, not an
end-to-end product or association-frame test.
