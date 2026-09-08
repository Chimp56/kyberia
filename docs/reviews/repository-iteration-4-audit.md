# Iteration 4 independent repository audit

Repository retention note: this file preserves the independent reviewer report verbatim below. The standalone probes remain in the review worktree named in the report; their counterexamples are being converted into maintained package regressions before each affected fix is integrated.

Review date: 2026-09-07. Reviewer: `/root/repo_audit_luna`.

This is a read-only audit. No production source, main worktree, or other
worktree was edited. Review outputs and probes are retained in this directory.

## Scope and provenance

The requested baseline was `7d9022c3edd221ab7d18de0c7ad0a6319e409c14`.
During review, the shared main worktree advanced through the reviewed
supply-chain commit and was observed at
`a9a28d6f8846ed84d2d46ed3f274d13854c3839e` (`test(release): validate
committed-source supply-chain evidence`). The final observed ordinary working
tree diff was empty apart from the pre-existing untracked `.pnpm-store/`; its
binary diff hash was
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

I read the complete `AGENTS.md`, the complete `plan.md`, `STATUS.md`, the
canonical domain contracts, survey state, native macOS normalizer, PCAPNG and
Kismet adapters, spatial analysis, project store, and the current supply-chain
tooling/evidence files.

## Findings

### MAJOR PCAP-ITER4-001 — finite PCAPNG sections use the wrong length origin

`crates/packet-import/src/pcapng.rs:341-348` computes a finite section end as
`bytes + size + s.section_len`. The PCAPNG Section Header Block's
`section_length` is the total section length, including that Section Header
Block. At the point this code runs, `bytes` is the offset before the SHB and
`size` is the SHB block length, so the implementation adds the 28-byte SHB a
second time.

Counterexample: the retained independent probe
`pcap-section-len-probe/src/main.rs` constructs a valid little-endian SHB + IDB
+ EPB stream, sets `section_length` to the complete 96-byte section, and calls
the public `read` function. The command:

```text
CARGO_TARGET_DIR=.worktrees/manifest-luna-review/.review/repo-audit-iteration4/pcap-section-len-probe/target \
  cargo run --offline --manifest-path \
  .worktrees/manifest-luna-review/.review/repo-audit-iteration4/pcap-section-len-probe/Cargo.toml
```

prints `len=96 result=Err(Truncated)`. The checked-in test at
`crates/packet-import/tests/pcapng.rs:292-300` calls the value
`valid.len() - 28`, which makes the test conform to the implementation's
incorrect origin rather than the file format.

Impact: finite captures emitted by normal PCAPNG writers are rejected at EOF,
so packet evidence cannot be imported. An inflated/nonstandard length can also
move the expected section boundary beyond EOF and alter malformed/truncation
classification. Correct the end calculation to use the SHB start offset plus
the field value, and change the fixture to write the complete section length;
retain tests for multiple sections and trailing bytes.

### MAJOR STORE-ITER4-001 — SQLite sidecars bypass the project database read budget

`crates/project-store/src/bundle.rs:88-96` checks only
`project.sqlite`. `Bundle::open` then opens the database at lines 170-179
without checking `project.sqlite-wal` or `project.sqlite-journal`. SQLite can
consume a valid WAL when opening the main file, so the advertised 64 MiB
metadata read budget does not cover all bytes used to establish the bundle
state. Sidecars are also not rejected as Kismet's adapter does.

The retained `bundle-sidecar-probe/src/main.rs` creates a valid bundle, keeps a
real SQLite WAL connection open, commits 1,000 padded/restore cycles, and
leaves a valid `project.sqlite-wal` of 86,520,032 bytes beside an 8,192-byte
main database. `Bundle::open` still opens the bundle read-only and the probe
prints:

```text
main_size=8192 wal_size=86520032 open_ok=true
```

This is an adversarial resource-boundary bypass: the valid WAL is available for
SQLite to read and replay even though only the 8,192-byte main file was checked.
Reject any `-wal`/`-journal` sidecar for this closed-bundle format, or account
for each regular sidecar within a bounded total budget and verify the resulting
state. Add a regression that places both a sparse oversized sidecar and a
valid oversized WAL beside an otherwise valid bundle.

### MAJOR CAPTURE-SURVEY-ITER4-001 — native scan observations have no admitted timing path

The macOS normalizer deliberately emits unknown capture timing, pose, dwell,
and scan age at `crates/capture-adapter/src/macos/normalize.rs:329-357` and
stores the API receipt/window only in `ReceivedObservation` at lines 360-378.
`crates/survey/src/state.rs:173-216` accepts only an `ObservationEnvelope`,
requires `data.time.monotonic` to be known, and rejects `ClockUncertain` in
`bad_quality` at lines 509-520. The normalizer marks every observation
`ClockUncertain` at line 285. Therefore a valid native CoreWLAN result cannot
enter `PointSurvey::admit`; passing its `SourceResponseTiming.returned_at` as
the `received` argument cannot fix the missing source capture time, and the
current API has no projection/assignment operation from `ReceivedObservation`.

The normalizer's own test `source_receipt_is_never_measurement_time_or_dwell`
passes and verifies the first half of this path. The docs acknowledge the
other half in `docs/adapters/capture-normalization.md:45` and
`docs/architecture/ADR/0005-source-response-timing.md:23-25`, but this remains
an integrated contract blocker: the primary native managed-mode adapter cannot
complete a strict point survey. Add an explicit policy and API that either
assigns a separately labeled receipt-time sample with appropriate quality and
age semantics, or requires a capture-capable source; persist the raw pair and
test the adapter-to-survey rejection/assignment path. Never overwrite capture
time or dwell with response timing.

### MINOR SPATIAL-ITER4-001 — metric-definition provenance does not constrain dBm aggregation

`crates/spatial-analysis/src/model.rs:67-76` documents
`Inputs.metric_definition` as pinning aggregation, but `Model::new` at
`90-145` always computes coincident samples with `convex_mean` over dBm. The
tile reports the fixed string `arithmetic-mean-dbm/1` at
`crates/spatial-analysis/src/tile.rs:108-113`; no enum or validation links the
caller-supplied metric definition to that aggregation.

Counterexample: an input with `metric_definition = "linear-power-mean-dbm/1"`
and co-located values -40 dBm and -80 dBm is accepted, reports an arithmetic
mean of -60 dBm, and serializes `coincident_aggregation` as
`arithmetic-mean-dbm/1`. A linear-power mean would be approximately -42.97
dBm. The output is self-describing, so this is not a hidden average, but the
input provenance can claim a different aggregation and the API cannot prevent
that unit-semantic mismatch. Replace the free-form aggregation claim with a
validated aggregation enum/registry or reject definitions that do not match
the implementation; add a two-value power-domain regression.

### MINOR SUPPLY-FREEZE-ITER4-001 — retained supply-chain freeze hashes are stale

The current `docs/reviews/supply-final-freeze-hashes.json` does not hash the
current versions of two files. The independent comparison produced:

```text
docs/licenses/SOURCE_LEDGER.md
  actual   0136df08424b43c3804535576cfe964a4017913fdf976ab3c5e70cd40c224664
  frozen   bc5cffec1e2b1ad8a6f729e43afd8d515c33aa9df536e83d62063006315a0281
docs/validation/supply-chain-runtime.json
  actual   3722d0411d27101269711b71dde6f6e46ff60fca069d55cf2f05d65105446698
  frozen   793b2dea4fc1f9cab55625f287faeb2585fea814d0fd33eed81b89e9bf5b49ff
```

The source-ledger correction and committed runtime-evidence addition are
intentional current changes, but `docs/reviews/supply-chain-final-review.md`
still says the 15-file freeze was verified. Refresh the freeze hash manifest
alongside those accepted files, or retain the original files under a distinct
freeze directory. Until then, the retained review cannot reproduce its stated
freeze and should not be used as current supply-chain evidence.

## Checks and retained probes

- `python3 tools/architecture.py`: PASS, reviewed dependency directions and
  external package boundaries.
- `cargo test -p kyberia-capture-adapter --test normalization
  source_receipt_is_never_measurement_time_or_dwell --offline`: PASS (1 test).
- `cargo test -p kyberia-project-store --test schema_guard
  oversized_sparse_database_is_rejected_without_reading_its_contents
  --offline`: PASS (1 test); this covers only the main file and is why the
  sidecar probe is separate.
- The PCAPNG and sidecar probes compile and run under local `target/` paths in
  this owned review directory. No source edits were made.

## Ten-field handoff

1. **Scope completed:** architecture/dependency direction, canonical units and
   provenance, capture-to-survey timing composition, PCAPNG/Kismet parser
   boundaries, project SQLite storage, spatial aggregation semantics, and the
   current supply-chain freeze/evidence diff.
2. **Files changed:** only this report and retained standalone probes beneath
   `.worktrees/manifest-luna-review/.review/repo-audit-iteration4`; no main or
   production source files.
3. **Architecture decisions:** no new ADR; preserve response timing separately
   from capture timing, keep foreign schemas at adapter boundaries, and make
   storage sidecar policy explicit.
4. **Tests/probes added:** finite total-length PCAPNG probe, oversized SQLite
   sidecar probe, and hash-manifest comparison; source tests were not changed.
5. **Tests executed/results:** both standalone probes pass their adversarial
   assertions; native normalization, project-store main-file budget, and
   architecture checks pass.
6. **Known limitations:** no hardware capture, no live Kismet producer, and no
   end-to-end survey assignment policy exists to validate on this revision.
7. **Requirements advanced:** parser framing and untrusted bundle-boundary
   evidence; explicit review of the response-timing contract and supply-chain
   provenance.
8. **Requirements still open:** finite PCAPNG interoperability, sidecar
   resource policy, native receipt-to-survey assignment, metric aggregation
   registry, and freeze-manifest refresh.
9. **Risks/follow-up:** resolve both MAJOR findings before promoting packet
   import or untrusted project opening; resolve the capture policy before
   calling native managed-mode scans survey-ready. Keep current docs explicit
   that no usable survey app is promoted.
10. **Suggested disposition:** reject promotion of the affected integrated
    paths pending independent fixes and rerun this review's probes; source
    edits belong to the implementation owner.
