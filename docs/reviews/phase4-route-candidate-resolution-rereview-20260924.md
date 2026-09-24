# Independent rereview: route candidate coordinate resolution

## 1. Verdict

**APPROVE WITH FOLLOW-UP.** The prior coordinate-collapse finding is resolved
for the documented same-segment adjacent-station case, and the new retraced
route regression preserves separate route occurrences as intended. One new
MINOR work-accounting gap remains: the added resolution preflight is not
included in the aggregate work estimate. Its iteration is bounded, but it can
push admitted work above the stated cap.

## 2. Reviewed range and plan

- Parent reviewed candidate: `3ed543ff77fc219f3c646f06d058b3123d573af8`
- Exact reviewed head: `310893d970379aa84d2559a9ecf0ebb5febee3d5`
- Plan SHA-256: `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`
- Scope: §6.8 OPT-001; §§9.1, 9.3, 16.3, 16.11; Phase 4; §18.8 OPTB-001.

The range adds the coordinate-resolution check, two candidate tests, and
STATUS/ledger/validation refreshes. No unrelated product behavior changed in
the reviewed diff.

## 3. Coordinate-resolution correction verified

In `crates/planner-candidate-adapter/src/lib.rs`,
`validate_station_coordinate_resolution` replays the station interpolation
sequence after the station/work bounds are checked and before
`let mut candidates = Vec::new()` (lines 221-228, 294-331). It compares
adjacent points only when they belong to the same route segment, returning the
typed `UnsupportedCoordinateResolution` error (lines 318-325). This rejects
without exposing a partial result.

The regression uses the previously reported allowed rectangle spanning
x=`999999998.0..1000000000.0`, route
`[999999999.0, 0.0] -> [999999999.007, 0.0]`, spacing `1.15e-7` m, and
maximum cable length `0.01` m. It first proves the production interpolation
expression collapses station indices 14 and 15, then asserts generation
returns `UnsupportedCoordinateResolution` (tests/candidates.rs:294-332).
The adjacent-collapse defect from the prior review is therefore resolved for
this case.

The retraced-route regression `[0,0] -> [2,0] -> [0,0]` with 2 m spacing
returns IDs 0, 1, 2; the first and last positions match while their route
distances are 0 and 4 m (tests/candidates.rs:334-366). This confirms there is
no global coordinate deduplication. The segment-index check deliberately
allows repeated coordinates at distinct route occurrences.

## 4. Finding — MINOR: resolution preflight omitted from work accounting

The aggregate estimate at `src/lib.rs:194-218` still charges two units per
station and three passes over route points. Those counts covered station-count
preflight/emission and route validation/segment lengths/emission before this
change. The new `validate_station_coordinate_resolution` loop at
`src/lib.rs:294-331` adds a third station traversal and another monotone
segment traversal, but the estimate is unchanged. The call is after the
work-cap check and before candidate-vector allocation at lines 217-228.

I reproduced an admitted near-cap request through the public Rust API: one
valid 17-coordinate polygon (one polygon, no exclusions); a two-point route
inside it of length `0.58252000000000004` m; spacing `0.00001` m; maximum
cable length `1.0` m. It has 58,253 stations. The current formula admits it
with `58,253 * (17*4 + 1 + 1 + 2) + 17 + 2*3 = 4,194,239` units, below the
`4,194,304` cap. The new resolution pass then processes all 58,253 stations
(and advances the segment cursor), work not included in that value. Counting
one additional per-station unit alone gives 4,252,492; the extra route-point
pass adds at most two further units in this fixture.

This remains bounded by existing station/route limits, so it is MINOR rather
than an unbounded-work issue. Update the admission estimate/comment and
validation description to account for the new pass (and route traversal), or
otherwise demonstrate that those units are already conservatively charged.

## 5. API, error, and preflight review

`UnsupportedCoordinateResolution` is a public typed error variant; its
`Display` remains deterministic through the existing debug-based formatter.
The error type and station records do not add serialization behavior. No
exhaustive error match or downstream production caller was found in the
reviewed repository source. Preflight uses the same station-distance and
segment-boundary advancement logic as emission. Exact vertices continue to be
assigned to the following segment, while the new retrace case retains stable
route-ordinal IDs.

## 6. Tracking and claims

The ledger passes with 5,396 source blocks, 438 explicit IDs, and 447 headings;
the OPTB-001 row remains `IN_PROGRESS`. STATUS and the validation record keep
the slice unintegrated and synthetic and leave OPTB-001 / Phase 4 open. The
candidate's 65,536 pre-pruning station cap still exceeds the evaluator's
128-candidate cap; any planner connection must define selection/remapping.
Route-local ordinal IDs also need a namespace/remap if multiple route outputs
are combined.

## 7. Checks and limitations

In the assigned detached worktree, with output under its ignored `target/`:

- `cargo test -p kyberia-geometry-adapter -p kyberia-planner-candidate-adapter --locked --offline` — PASS, 57 tests (41 geometry, 16 candidate).
- `cargo clippy -p kyberia-geometry-adapter -p kyberia-planner-candidate-adapter --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 packages.
- `python3 tools/ledger.py check` — PASS.
- `git diff --check 3ed543ff77fc219f3c646f06d058b3123d573af8..310893d970379aa84d2559a9ecf0ebb5febee3d5` — PASS.
- One-off Rust public-API work-accounting reproduction described above — PASS (generation accepts the input at the current estimate).

This is exact-source review; graph MCP is unavailable, so no graph-coverage
claim is made. No full-workspace suite or current-main integration validation
was run. Real-building, optimizer, RF, and Phase 4 acceptance remain out of
scope.

## 8. Report artifact

This report is the only intended change on the review worktree; candidate source
and generated outputs were not edited.
