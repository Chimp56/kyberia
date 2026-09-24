# Phase 2 association/reassociation IE-framing increment

This bounded parser increment is integrated from author commit `50c483e` as
`fefcce6`. The independent review report is
[`phase2-association-ie-current-review.md`](../reviews/phase2-association-ie-current-review.md)
(report commit `70e24ae`).

The implementation extends `crates/ieee80211/src/lib.rs` to locate and expose
information elements in Association Request, Association Response,
Reassociation Request, and Reassociation Response management frames. After
the 24-byte management header, the parser skips the subtype-specific fixed
body before starting IE traversal: 4, 6, 10, and 6 bytes respectively. These
association fixed-body bytes are retained in the original MPDU but are not
decoded as `ResponseFixedFields`; `fixed()` remains populated only for Beacon
and Probe Response frames.

The focused regressions cover all four subtypes, exact IE and payload offsets,
raw IE order/bytes, repeated vendor-element handling, addresses and sequence
fields, canonical encode/decode round trips, and every truncated fixed-body
prefix. Existing Beacon/Probe tests remain in the same suite, while unsupported
unrelated management subtypes remain rejected. No association fixed-field
semantics are claimed.

## Validation

- `cargo test -p kyberia-ieee80211 --locked --offline`: 33 unit tests, 3
  fixture differential tests, and 3 compile-fail doctests pass. The explicit
  external tcpdump comparison remains intentionally ignored in the default
  test run.
- `cargo clippy -p kyberia-ieee80211 --all-targets --locked --offline -- -D
  warnings`: pass.
- `cargo fmt --all -- --check`: pass.
- `python3 tools/architecture.py`: pass.
- `python3 tools/source_inventory.py check`: pass (522 locked external
  packages).
- `python3 tools/ledger.py generate` and `python3 tools/ledger.py check`: pass
  (5,396 source blocks, 438 explicit ID occurrences, and 447 headings).
- `git diff --check`: pass.

This bounded parser increment does not add Association/Reassociation fixed-
field semantics, Authentication/Disassociation/Action parsing, standards
clause/help links, desktop Lab UI/IPC, Kismet/physical capture validation, or
other Phase 2 deliverables. `INS-005` and Phase 2 remain `IN_PROGRESS`.

## Review note

The independent review approved this bounded increment with a nonblocking
pre-release API compatibility note: adding variants to the public exhaustive
management-frame enum may require downstream exhaustive matches to be updated.
No in-repository production consumer requires such a change. The wire parser's
existing Beacon/Probe behavior is preserved; `INS-005` and Phase 2 remain open.
