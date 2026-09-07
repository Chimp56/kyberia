# Independent review: PCAPNG container adapter

Reviewer: `rf_numerical_review`, independent of root implementation author. Date: 2026-09-07. Decision: **APPROVED for this bounded backend increment**, after independent verification of PCAP-001. No unresolved BLOCKER, MAJOR, MINOR or NIT findings remain. Scope is the bounded PCAPNG container increment, not the full packet capture, canonical normalization, Kismet integration or release security gates.

## Specification and threat boundary

The complete authoritative plan was read during this review assignment, including §7 time/provenance methodology, §14 inward contracts, §15 untrusted capture/import and data minimization, §16 parser validation and Appendix I integration/runtime gates. The relevant threat is an untrusted finite byte stream producing packet evidence and a final receipt. The adapter must bound framing/allocation, preserve source units and unknown values, and never issue a successful import receipt after corruption, cancellation or timeout. Callers own staged transactional publication and process isolation for blocking I/O; no production canonical/store/UI consumer exists in this increment.

Reviewed every packet crate source/test file, its adapter documentation, workspace dependency policy, Cargo lock entries and source/license inventory additions. Followed parsing into the pinned dependency's generic framing, enhanced/simple packet, option, section/interface, decryption-secret and name-resolution implementations to inspect reachable bounds. This is an independent manual code/contract review, not a claim of a completed automated whole-repository security scan or advisory audit.

Primary format semantics were checked against the [PCAPNG format draft, version 05](https://www.ietf.org/archive/id/draft-ietf-opsawg-pcapng-05.html): timestamp resolution and signed offset, section-local interface references, original/captured lengths, packet padding, drop-count and packet-ID options. The implementation correctly keeps these facts separate from source clock accuracy and packet reception deduplication.

## Finding PCAP-001 — MAJOR, resolved

The original `read_part` checked cancellation/deadline before `reader.read`, but directly returned EOF success when that call returned zero bytes. `read` then finalized a receipt without rechecking. Cancellation or timeout occurring during the final EOF read therefore produced `Ok(Receipt)` and could permit publication of an import the caller had cancelled or that exceeded its budget.

Two independent concrete probes reproduced the defect against `crates/packet-import/src/pcapng.rs` SHA-256 `eebf8f0d4a8a1c2e8e7127d6c54444dcc6773059d41b56a75338271085b60e2e`:

- An `EofCancel` reader delegates to a finite slice, then sets the provided `AtomicBool` only when its underlying read returns zero. A three-block, 108-byte, one-packet stream incorrectly returned a successful receipt instead of `Cancelled`.
- An `EofDelay` reader sleeps 20 ms only after its underlying slice returns EOF, with a 10 ms parser timeout. The same stream incorrectly returned a successful receipt instead of `Deadline`.

Requested correction: recheck cancellation/deadline after every read returns, and before final success/publication. Keep the documented requirement for process supervision: a cooperative check cannot interrupt a currently blocked kernel read. The root implementation author accepted and corrected the finding. The corrected code captures each read result, rechecks cancellation/deadline before processing it, then also checks immediately before returning the constructed final receipt. Both independently authored reproducers now return their expected errors in debug and release. Two persisted author regression tests additionally assert that the EOF branch was reached, and all 17 packet behavior tests pass.

## Other review results

The minor documentation count/coverage drift introduced by the two new EOF regression tests was corrected by the author before the final snapshot. No additional confirmed correctness or security finding. Timestamp conversion uses a sufficiently wide integer intermediate and explicitly reports overflow; all 256 resolution bytes passed independently generated arbitrary-precision integer reference cases in both byte orders. Missing simple-packet time remains unknown. Enhanced-packet caplen is bounded against parsed data and snaplen; padding never becomes payload. Interface IDs reset per section, duplicate scalar timestamp/packet options reject ambiguity and exact stream bytes feed the final hash.

Bounds are layered: the one-MiB block limit bounds the input allocation before the adopted parser runs. The 256-option check occurs after the adopted parser has built its borrowed option vector, so the one-MiB limit is not a one-MiB total heap guarantee. Parser allocation overhead is still bounded by block length. This distinction and hard wall-clock/process limits must remain part of production importer design. A finite container prefix may reach a staging callback before a later failure, so receipt-gated transactional publication is essential.

Static review found no unchecked 32-bit-target arithmetic reachable from untrusted packet lengths under the outer one-MiB framing bound. This is not 32-bit execution evidence. Every platform beyond the tested macOS arm64 host remains unexecuted here.

## Executed validation

The first main-worktree `--locked` attempt failed while root was integrating the independently reviewed spatial package. No packet source changed; after the author updated workspace metadata, the exact locked commands passed. The reviewer did not alter main source or metadata.

```sh
cargo test -p kyberia-packet-import --locked --offline
cargo clippy -p kyberia-packet-import --all-targets --locked --offline -- -D warnings
cargo test -p kyberia-packet-import --release --locked --offline packet_stream_benchmark -- --ignored --nocapture
.tools/venv/bin/python tools/source_inventory.py check
python3 tools/architecture.py
```

Initial results: 15 behavioral tests passed, one explicit benchmark passed separately, Clippy passed, source inventory verified all 90 locked external packages and architecture policy passed. The system Python 3.9 lacks Tomli; source inventory was run successfully with the repository's bootstrapped virtual environment. Local release benchmarks including parsing and SHA-256: 10,000 packets / 2,880,072 bytes in 16.359 ms; 100,000 packets / 28,800,072 bytes in 93.780 ms. Each payload is 256 bytes; construction is excluded. These are single-host observations, not stable CI thresholds.

Independent standalone support is retained in ignored `.tools/packet-review-probe` in the review worktree, with path dependencies pointing to the frozen main crate. Four non-regression probes pass:

1. Arbitrary-precision Python oracle: seed 5342; for each resolution byte 0..255, use tick counts 0, u64::MAX and one seeded random u64, with seeded offsets from [-3, 0, 2, i64::MAX, i64::MIN]. Python computes exact quotient/remainder and checked i64 output. Both byte orders produce 1,536 checked packet cases, including expected overflow.
2. Enhanced-packet capture lengths 0, 1, 2, 3, 4, 1024, 1048576, u32::MAX-32, u32::MAX-5, u32::MAX-4 and u32::MAX, in both byte orders, never panic.
3. 6,336 structured block mutations: 12 block types, 33 body lengths 0..128 by four, eight LCG bodies each, both byte orders. Includes recognized and unknown types; no panic or unbounded work was observed.
4. Actual 257-option rejection and zero-snaplen unlimited packet acceptance in both byte orders.

The two EOF regressions failed before the correction, as described above, and now pass alongside the four other independent probes in both debug and release. No production parser or dependency source was edited by the reviewer. This bounded mutation exercise is not coverage-guided fuzzing or proof that no malformed input can fail.

## Source/dependency check

Adopted pcap-parser 0.17.0 archive SHA-256 is `1099a3a64b376c536394622c9e5ea8ecfac3e3e44f88857b1e7da114135d3df4`, declared `MIT/Apache-2.0`. The three new transitive packages are circular 0.3.0 (MIT), nom 8.0.0 (MIT) and rusticata-macros 5.0.0 (`MIT/Apache-2.0`); there are four new external packages total. Source inventory checks all downloaded archive hashes against the lockfile. No GPL source or native libpcap dependency is introduced. This verifies declared provenance and boundary, not legal clearance for a future distributable. Release notices, advisory checks and target SBOM remain open.

## Final verification and immutable review snapshot

After the correction, the reviewer reran all 17 authored behavior tests, all six independent tests in debug and release, Clippy with warnings denied, rustfmt checks, source inventory (90 packages), architecture checks and the explicit release benchmark. Every command passed. Final benchmark: 10,000 packets in **10.378 ms**, 100,000 packets in **103.334 ms**, with the same sizes and timing scope as above. The source is approved only at the exact hashes below. Main metadata was inspected at HEAD `fc76fd77ece78e380ffe1274dca74ea471394cc2` with the packet addition and reviewed spatial workspace entries uncommitted; later unrelated metadata changes do not imply changes to these packet source bytes.

| Artifact | SHA-256 |
|---|---|
| `crates/packet-import/Cargo.toml` | `3e7d3edd8e0d40fb0ab28d438d77a5f781796730761987c9ea9f9163b1d1355f` |
| `crates/packet-import/src/lib.rs` | `9b018ad351d794f1397880ba7a2ea6a379153280846d67600ffc306767ca5ae5` |
| `crates/packet-import/src/pcapng.rs` | `3fd9d3e4856fa8128c2b1a264c08ca1c7cf1bf0a560d29a56e8caa5f4efb485f` |
| `crates/packet-import/tests/pcapng.rs` | `679bb746350f8848439495e73a23f133295619bc1cd7f68a3cdb95c5816ec47f` |
| `docs/adapters/pcapng.md` | `1ca9e3a9f8bd7857a03c80a67ff8eea8e219051f653091f4359fa696b520b05f` |
| `Cargo.lock` | `0a4a9aee763a2c507996ab222bb21d72907e8afe1221871e8d297dc0f1cd5dd3` |
| `tools/architecture.json` | `89ca16e639a1e72907de0ce2ab06e2df53a1610e5705bb621394dfa412cdd7c1` |
| `docs/licenses/cargo-sources.json` | `3c657842734409567266cbccaef26a09ee2abea3e454ecbc7c0463bea47d493d` |
| `docs/licenses/SOURCE_LEDGER.md` | `95b92fecf221457f66095028bc564e8ce4b221771720077a41bba0ec999568fa` |

## Ten-field handoff

1. **Scope completed:** independent PCAPNG framing, source/timestamp semantics, malformed input, cancellation, resource, dependency-boundary and performance review.
2. **Files changed:** only `docs/reviews/pcapng-container-review.md`; ignored independent test harness retained locally. Root alone changed production code and regression tests.
3. **Architecture decisions:** no new ADR; confirmed adoption behind an outward adapter, borrowed packet evidence and receipt-gated staged publication. Process isolation remains necessary for hard I/O deadlines.
4. **Tests added:** six independent probes, including two reproductions that disproved the original EOF cancellation/deadline contract, 1,536 arbitrary-precision timestamp cases and 6,336 bounded structured mutations.
5. **Tests executed/results:** final 17 author tests, six independent debug tests, six independent release tests, explicit benchmark, Clippy, rustfmt, 90-package inventory and architecture checks all pass.
6. **Known limitations:** no real capture producer parity, 32-bit execution, coverage-guided fuzzing, process isolation or application transactional publication tested in this increment.
7. **Requirements advanced:** software container portions of backlog OSS-002/COL-003 and Appendix I CAP-006/TST-002/TST-006; complete Kismet runtime or product capability gates remain open.
8. **Requirements still open:** legacy PCAP and obsolete packet blocks, interface-statistics/custom source metadata, Radiotap/802.11 semantics, canonical normalization, real-producer parity, privacy-aware persistence, raw export, UI/import workflow and release notices/SBOM/advisory audits.
9. **Risks/follow-up:** preserve staging until successful receipt; supervisor must bound blocking readers and callbacks; one-MiB input blocks can allocate additional parser metadata; unknown/skipped source data must not silently become complete survey provenance.
10. **Suggested commit:** `docs(review): approve corrected PCAPNG container contracts`.

## Independent EOF reproducer

These tests use the original fixture constructors documented by the author (a complete SHB + IDB + EPB, 108 bytes total). The independently written reader wrappers isolate the asynchronous terminal boundary; they are not production implementations. The root has now added equivalent persistent regressions to the reviewed test file.

```rust
struct EofCancel<'a> {
    data: &'a [u8],
    cancel: &'a AtomicBool,
}
impl Read for EofCancel<'_> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let n = self.data.read(b)?;
        if n == 0 {
            self.cancel.store(true, Ordering::Relaxed);
        }
        Ok(n)
    }
}
#[test]
fn cancellation_during_final_eof_read_must_prevent_receipt() {
    let data = fixture(Order(false));
    let cancel = AtomicBool::new(false);
    let result = read(
        EofCancel {
            data: &data,
            cancel: &cancel,
        },
        Limits::default(),
        &cancel,
        |_| true,
    );
    assert_eq!(result.unwrap_err(), Error::Cancelled);
}
struct EofDelay<'a>(&'a [u8]);
impl Read for EofDelay<'_> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let n = self.0.read(b)?;
        if n == 0 {
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(n)
    }
}
#[test]
fn deadline_during_final_eof_read_must_prevent_receipt() {
    let data = fixture(Order(false));
    let cancel = AtomicBool::new(false);
    let limits = Limits {
        timeout: Duration::from_millis(10),
        ..Limits::default()
    };
    let result = read(EofDelay(&data), limits, &cancel, |_| true);
    assert_eq!(result.unwrap_err(), Error::Deadline);
}
```
