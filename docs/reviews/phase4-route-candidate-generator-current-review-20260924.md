# Independent review: Phase 4 route candidate generation

## 1. Disposition

**APPROVE WITH FOLLOW-UP.** No BLOCKER or MAJOR issue was found in the
reviewed slice. One MINOR numeric-resolution edge case is recorded below; it
does not invalidate the bounded foundation contract as currently documented,
but should be resolved or explicitly handled before a planner consumes these
stations as distinct physical placement options.

## 2. Reviewed range and plan scope

- Base: `664f885649c40cee02865bc721205570259f0688`
- Feature commit: `ba128152fd5290e74e407fb674bf3d6fc899794c`
- Test-only follow-up / reviewed head: `3ed543ff77fc219f3c646f06d058b3123d573af8`
- Plan SHA-256: `1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`
- Anchors: §§6.8, 9.1, 9.3, 16.3, 16.11, Phase 4, and 18.8 (OPTB-001).

The follow-up changes tests, the ledger, and validation wording only; production
code is unchanged from `ba128152`. The test additions cover an exact route
vertex and a route/frame mismatch.

## 3. Inspected implementation and evidence

Reviewed `crates/geometry-adapter/src/{lib.rs,polygon.rs}` and
`tests/polygons.rs`; `crates/planner-candidate-adapter/{Cargo.toml,src/lib.rs}`
and `tests/candidates.rs`; `crates/planner-evaluator/src/lib.rs`; `Cargo.lock`,
`tools/architecture.json`, `docs/licenses/cargo-sources.json`,
`docs/implementation/{TRACEABILITY.md,ledger.json}`, `STATUS.md`,
`docs/validation/phase4-route-candidate-generator.md`, and the scoped plan
anchors.

The point query enforces exact floor/frame identity and distinguishes inside,
boundary, and outside without implicit transformation. Candidate filtering
includes allowed boundaries (including hole boundaries), excludes exclusion
boundaries/interiors, and preserves original route-ordinal IDs after pruning.
The station loop advances to the next segment at an exact route vertex; the
inclusive cable cap compares the total length of the full polyline. Validation
and work admission precede emission, and errors return no partial vector.

Coverage includes boundary/hole/multipolygon policies, floor/frame mismatches,
route turns and exact vertices, the inclusive cable threshold, full-polyline
length, station/geometry/work limits, exclusion restoration with stable IDs,
and malformed/degenerate route cases. The 256-empty-exclusion test confirms
empty region-object visits are charged. The architecture declaration correctly
classifies the new crate as composition over domain, geometry, and evaluator
crates; dependency direction is inward.

## 4. Finding — MINOR numeric coordinate collapse

`checked_station_count` verifies that route-distance values progress, but
`interpolate` does not detect when adjacent distances round to the same
`Point2` at the route's coordinate magnitude (`crates/planner-candidate-adapter/src/lib.rs:255-306`).
This reproduces through the public Rust API with:

- an allowed rectangle spanning x=`999999998.0..1000000000.0`, y=`-1.0..1.0`;
- a route from (`999999999.0`, `0.0`) to (`999999999.007`, `0.0`);
- spacing `1.15e-7` m and maximum cable length `0.01` m.

The route length is `0.00699996948242188` m. Generation succeeds with 60,870
stations, but 2,149 neighboring pairs have equal `Point2` values because the
spacing is below the representable coordinate step near x=1e9. Station
distances and IDs remain distinct. This is within the declared coordinate,
station, and work bounds. The API does not promise unique coordinates, so this
is not a rejection of its present route-station semantics; however, a later
planner must not assume distinct IDs always mean distinct physical sites. Add a
resolution regression and either define/detect unsupported adjacent-coordinate
resolution or explicitly deduplicate/handle such sites at the composition
boundary. Self-intersecting routes can also revisit a coordinate legitimately,
so any policy should distinguish numerical collapse from route occurrence
identity.

## 5. Bounds and all-or-error review

The route-point, pre-pruning station, exclusion count, geometry coordinate, and
aggregate-work limits are explicit and checked without truncation. The work
estimate charges region-coordinate passes, polygon/object visits, station
preflight/emission, and route traversal before the station-emission loop.
Checked arithmetic and typed errors are used for route lengths, counts, and
interpolation outputs. Maximum cable length is inclusive; one excess input
rejects the whole request. I found no unbounded loop or partial-result escape
within this API's declared bounds.

## 6. Verification performed

All commands ran in the assigned detached review worktree; Cargo output stayed
under its ignored local `target/` directory.

- `CARGO_TARGET_DIR=/private/tmp/kyberia-phase4-candidate-generator-review-20260924/target cargo test -p kyberia-geometry-adapter -p kyberia-planner-candidate-adapter --locked --offline` — PASS, 55 tests (41 geometry, 14 candidate).
- `CARGO_TARGET_DIR=/private/tmp/kyberia-phase4-candidate-generator-review-20260924/target cargo clippy -p kyberia-geometry-adapter -p kyberia-planner-candidate-adapter --all-targets --locked --offline -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 packages.
- `python3 tools/ledger.py check` — PASS, 5,396 source blocks, 438 explicit IDs, 447 headings.
- `git diff --check 664f885649c40cee02865bc721205570259f0688..3ed543ff77fc219f3c646f06d058b3123d573af8` — PASS.
- A one-off Rust executable linked against the review build reproduced the coordinate-collapse case above; it was written only under ignored `target/`.

## 7. Tracking and integration cautions

`backlog:OPTB-001:1` remains `IN_PROGRESS`; `STATUS.md` accurately labels this
as an unintegrated candidate-generation slice with integration review pending.
The validation record limits evidence to synthetic route-backed geometry and
correctly leaves OPTB-001 and Phase 4 open. Nothing here establishes inferred
routes, whole-floor sampling, mounting/power/RF behavior, optimizer correctness,
or any Phase 4 exit criterion.

Before wiring into planning, reconcile the adapter's 65,536-station maximum
with the current evaluator's 128-candidate limit
(`crates/planner-evaluator/src/lib.rs:12,258-260`). Also namespace or remap
route-local ordinal IDs if multiple route results are combined. These are
integration obligations rather than defects in this one-route foundation.

## 8. Limitations

This was an exact-source bounded review; graph MCP was unavailable, so no graph
coverage or completeness claim is made. The full workspace/platform suites and
current-main integration were not run. Real-building route quality, field
behavior, and optimizer semantics remain outside scope.

## 9. Report artifact

This report is the only intended change on the review worktree. Candidate
source, tests, documentation, and generated outputs were not edited.
