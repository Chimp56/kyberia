# Render scene resource review

Candidate: `1fdcc7f60431a91c299d787e0d0ac22251926980` in
`.worktrees/canonical-render-scene`. Reviewer: root; resource/cancellation author:
Rawls. Status: review in progress; integration not approved.

## Verified evidence

Root independently executed `cargo test -p kyberia-rendering-scene --locked
--offline`: eight unit and nineteen integration tests pass. Direct mutable-input
ordering now projects canonical replay output. Import replays preserved samples;
prior independent review accepted its numerical rejection tests.

Cancellation checks now cover decoding, structural/grid loops, replay,
projection, encoding and hashing. The candidate adds pre-clone shape admission
for sample/group/cell/contribution counts and estimated work/memory.

## Accounting evidence required

The code claims its constants over-approximate vector storage at overlapping
Model/Tile/SceneWire peaks, but does not derive the overlap factors. A retained
host probe at `.worktrees/canonical-render-scene/.trash/render-size-probe-20260909`
reports `Sample=136`, `Cell=96`, `LocationGroup=144` bytes. The group accounting
constant is 128 bytes. This alone does not prove the total estimate is exceeded,
since the formula includes encoded-byte and fixed terms, but it disproves any
interpretation that the group term alone covers even one group value.

Provide an explicit derivation for live copy counts, nested observation ID
vectors (both group IDs and aggregate observation order), decoding capacity,
canonical output and temporary spatial-analysis arrays. Validate representative
large tiles and cancellation latency. Keep accounting proxies separate from RSS;
no exact memory ceiling is implied by a passing shape test.

## Corrected allocation model

Root inspected follow-up `0376336fa15e42c31ae95e4dc47a7d92ba8c42d9` and
independently ran the locked/offline rendering-scene suite: ten unit and twenty
integration tests pass. The replacement uses measured type sizes and observed
vector capacities, four sample/group collections, three cell collections,
nested observation-ID vectors, aggregation scratch and explicit allocation
padding. The padding is an accounting policy, not an allocator RSS guarantee.
The previous 128-byte group constant is removed.

The representative 512-sample/1,024-cell test establishes successful admission
and cooperative cancellation after 32 polls. It does not measure process memory
or wall-clock cancellation latency. Those measurements remain requested.

Serialized import checks the 64 MiB encoded input limit before serde decoding,
but checks decoded vector capacities afterward. Review of pre-decode allocation
bounds therefore remains open; a post-decode rejection does not prove that a
256 MiB working-set policy was enforced before allocation. The implementer is
evaluating decoded expansion and bounded admission, alongside the measurement
harness. Integration remains unapproved pending this evidence.

Full renderer selection, real map/browser integration and Gate B
performance/accessibility remain open.

## Current-host measurement at import-preflight candidate

Candidate `39f6555` adds lexical string admission, streaming shape admission,
duplicate-field rejection and a reproducible measurement test. Independent
follow-up review remains pending.

Root ran the compiled integration-test binary directly under macOS
`/usr/bin/time -l`, selecting
`resource_measurement_harness_covers_direct_import_and_cancelled_paths` with
`--exact --nocapture`. The 512-sample, 1,024-cell fixture passed and reported:

| Quantity | Observation |
|---|---:|
| Encoded scene | 618,346 bytes |
| Deterministic working estimate | 70,080,530 bytes |
| Direct projection | 93,839 microseconds |
| Canonical import | 122,898 microseconds |
| Cancellation | 7 microseconds after 8 polls |
| Maximum process resident set | 8,863,744 bytes |
| Peak process memory footprint | 5,554,536 bytes |

The measurement includes fixture construction and test-harness overhead. It is
one debug-build run on the current macOS host, not an allocation proof, an
upper bound, a cross-platform benchmark or a product SLO. The resource estimate
is deliberately distinct from resident memory. Log:
`.worktrees/canonical-render-scene/.trash/root-scene-resource-measurement-host.log`.
The earlier sandboxed run passed the test but could not collect macOS resource
statistics because `kern.clockrate` access was denied; its log is retained too.
