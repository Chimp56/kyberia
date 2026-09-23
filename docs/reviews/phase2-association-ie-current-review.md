# Phase 2 association/reassociation IE-framing independent review

## Scope and verdict

- Candidate: `50c483ecf63ca372558160ab4528e519bd01d2a5`.
- Base: `eef8d897050835146621749c14609c870009bbd6`.
- Reviewed plan sections: §6.1 / INS-005, §7.8, and Phase 2 roadmap/exit criteria.
- **APPROVE WITH A MINOR PRE-RELEASE API COMPATIBILITY NOTE.** No parser correctness blocker was found. The note below matters before treating this crate as a stable external API, but is not a blocker for this pre-1.0 isolated increment.
- Codebase graph tools were unavailable; no graph coverage claim is made.

## Parser review

The four new fixed-body lengths are correct for IE framing: Association Request 4 bytes, Association Response 6, Reassociation Request 10, and Reassociation Response 6. `parse_controlled` selects the appropriate fixed length before `preflight_elements` and ordinary TLV traversal, so the fixed body cannot be mistaken for an IE. The body is retained in `raw_mpdu`; `fixed()` is `None` for these subtypes, while Beacon/Probe Response continue to expose `ResponseFixedFields` and Probe Request continues to have no fixed body.

The existing header handling remains shared: address roles, sequence/fragment extraction, explicit FCS validation, management DS-flag rejection, input/resource bounds, and raw-frame retention are not special-cased or weakened. The new regression checks all four subtypes, expected addresses and sequence fields, exact first/second IE offsets and raw TLVs, duplicate vendor-IE retention, `fixed() == None`, and canonical encode/decode equality. Every shorter fixed-body prefix is rejected as `Truncated("management fixed fields")`. The unsupported-subtype test now covers all remaining management subtype values (6, 7, and 9–15).

The candidate does not decode association/reassociation fixed-body semantics or claim RSN/capability interpretation. That is accurately reflected in the README and validation note. The existing tcpdump differential test was updated to name the new enum variants, but its checked-in/executed packet set still contains only Beacon, Probe Request, and Probe Response. Thus there is no external tcpdump differential evidence for the new subtypes; the focused association cases are synthetic unit inputs. The documentation does not claim otherwise.

## Minor compatibility note

`ManagementSubtype` is a public exhaustive enum, and this change adds four variants. That is source-breaking for an external downstream crate with an exhaustive match over the previous variants. No in-repository production consumer was found, the workspace package is still version `0.1.0`, and the plan defers stable public library APIs; therefore this is not an integration blocker for the current candidate. Before publishing/stabilizing the crate, document the compatibility policy or make the enum forward-compatible at an appropriate version boundary.

## Documentation and ledger

`STATUS.md` distinguishes the previously integrated parser from this pending isolated association candidate and keeps Lab UI/IPC, standards help, remaining Phase 2 work, `INS-005`, and Phase 2 exit open. The new validation note limits its claims to framing and says the independent review is pending. `INS-005`, the Phase 2 group, and the Phase 2 parser/explorer deliverable remain `IN_PROGRESS`; generated traceability links the new validation artifact at the relevant INS-005 and parser/explorer rows. No historical `WIFI-001` fuzz or review claim was promoted by this increment.

## Reviewer validation

- `cargo test -p kyberia-ieee80211 --locked --offline` — PASS: 33 unit tests, 3 fixture differential tests, and 3 compile-fail doctests. The external tcpdump differential remains intentionally ignored by default.
- `cargo clippy -p kyberia-ieee80211 --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/ledger.py check` — PASS (5,396 source blocks; 438 explicit ID occurrences; 447 headings), including recorded hashes.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS (522 locked external packages).
- `git diff --check` — PASS.

## Ten-field handoff

1. **Verdict:** Approve with a minor pre-release API compatibility note; no parser blocker.
2. **Candidate/base:** `50c483ecf63ca372558160ab4528e519bd01d2a5` / `eef8d897050835146621749c14609c870009bbd6`.
3. **Fixed-body framing:** Correct 4/6/10/6-byte skips before IE preflight/traversal.
4. **Raw/API semantics:** Raw MPDU retained; association fixed bodies are not miscast as `ResponseFixedFields`; existing Beacon/Probe semantics are preserved.
5. **Offsets/order/duplicates:** Tests verify offsets, raw TLV slices, ordering, and repeated vendor elements.
6. **Malformed/truncated/bounds:** Every short fixed-body prefix rejects; shared header/FCS/resource/IE guards remain in place; unsupported subtypes remain closed.
7. **Canonical/address/sequence:** New subtypes round-trip canonical bytes and retain expected address and sequence roles.
8. **Validation:** 33 unit + 3 fixture differential + 3 compile-fail doctests pass; strict Clippy, fmt, ledger, architecture, inventory, and diff checks pass.
9. **Limitations:** No association tcpdump differential packet, hardware capture, fixed-body field decoding, UI/IPC, standards clause/help catalog, or Phase 2 exit claim. Public enum extension can break exhaustive external matches before a stable release.
10. **Review tree:** Detached worktree `/private/tmp/kyberia-phase2-association-review-20260923`; this report is the only intended change. Report commit and final cleanliness are provided in the handoff.
