# Map asset admission current-candidate review

Date: 2026-09-23

## 1. Verdict and scope

**REQUEST CHANGES — one MAJOR finding.** This is an independent review of the
exact current-main candidate below, not approval of historical implementations
or a claim of product acceptance. The parser accepts malformed PNG chunk
ordering despite the candidate's strict-ordering contract. No candidate source
was edited in this review worktree.

## 2. Plan anchors

The full authoritative `plan.md` was read before architectural judgment. The
relevant anchors are §5.3, “Floor-plan ingestion and coordinate calibration”
(L337–348); MAP-002 import (L803–820); MAP-003 calibration and georeferencing
(L822–831); MAPB-001/MAPB-002 (L4268–4269); and Phase 0 deliverables/exit
criteria (§17, L3987–4016). These establish a broader import catalog,
calibration scope, and Phase 0 exit than this candidate implements. This is a
bounded Phase 0 increment only; it does not close Phase 0.

## 3. Exact revisions

- Candidate branch: `feat/map-asset-admission-current-main`
- Candidate: `ca99d5055712861271a638b6cac684d9b3a9faab`
- Base (`main`): `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`
- Review branch: `review/map-asset-admission-current-main`

## 4. Reviewed paths

Reviewed the complete candidate diff and scoped source paths:

- `STATUS.md`; `crates/application/Cargo.toml`, `src/error.rs`, `src/lib.rs`,
  `src/map_asset.rs`, `src/map_mutation.rs`, `src/port.rs`, `src/session.rs`,
  `tests/project_session.rs`.
- `crates/causal-materializer/src/lib.rs`; `crates/domain/src/project.rs`,
  `src/project/commands.rs`, `src/project/entities.rs`,
  `src/spatial/calibration.rs`; `crates/operation-log/src/lib.rs`.
- `crates/project-store/src/bundle.rs`, `src/manifest.rs`,
  `src/materialized_project.rs`, `src/operation_log.rs`,
  `tests/bundle.rs`, `tests/materialized_publication.rs`,
  `tests/operation_store.rs`.
- `docs/architecture/ADR/0034-map-asset-admission.md`,
  `docs/architecture/ADR/README.md`, `docs/implementation/TRACEABILITY.md`,
  `docs/implementation/ledger.json`, this review, and
  `docs/validation/map-asset-admission.md`.

## 5. Invariants and findings

**MAJOR — malformed PNG chunk order is admitted.** In
[`map_asset.rs`](../../crates/application/src/map_asset.rs), the `sRGB` and
`gAMA` guards at L230–247 require only a pre-IDAT position; they do not reject
these chunks after `PLTE` (which the parser accepts at L168–187). The truecolor
`tRNS` branch at L193–204 checks its length and pre-IDAT position but does not
require an already-seen optional `PLTE`; a later truecolor `PLTE` is therefore
accepted. PNG-3 Table 7 requires `gAMA` and `sRGB` before `PLTE` and `IDAT`,
and `tRNS` after `PLTE` (if present) and before `IDAT` ([W3C PNG Specification,
§5.6](https://www.w3.org/TR/png-3/#chunk-ordering)). These are otherwise
well-formed chunk/CRC streams, so this is a parser admission gap rather than a
CRC or length failure. Track palette ordering in those cases and add
valid-CRC regression fixtures for each invalid sequence before approval.
`pHYs` correctly rejects chunks after IDAT; this review found no pHYs-after-IDAT
gap.

Other reviewed invariants:

- Admission derives dimensions, SHA-256, byte length, and `image/png` from
  content; extension/MIME hints are ignored and paths are not returned. The
  parser bounds source size, dimensions, pixel declarations, chunk count and
  metadata, uses checked chunk offsets, validates CRCs and rejects unsupported
  chunks/trailing bytes. **It does not validate IDAT zlib/DEFLATE or pixels and
  does not establish that the asset is decodable or displayable.** The ADR and
  validation packet state this limit accurately.
- Source references are checked against registered `MapSource` kind, media
  type, byte length and streamed SHA-256 at append/replay, baseline, and
  publication boundaries. Hash syntax precedes construction of the artifact
  path, symlinks/reparse points are rejected, and provenance is constrained to
  an opaque identifier. Exact same-content/provenance registration is
  idempotent; conflicting registration does not overwrite the prior artifact.
  Artifact registration and operation append are separate durable steps, so a
  later failure can leave the documented immutable orphan, but cannot publish
  it as current project state by itself.
- V3 uses typed map/calibration mutations and priors; the calibration prior
  carries the exact prior active value, including `Unknown(NotMeasured)`.
  Existing V1/V2 validation paths remain version-restricted and the focused
  suite retains compatibility/golden tests. Caller logical time, causal depth,
  parents and operation ID are explicit; wall-clock commit time is not used to
  infer causal ordering. Exact operation retry and reopen are exercised.
- The materializer compares both ancestry directions for floor evidence and
  floor mutations, so concurrent conflict detection does not depend on
  operation-ID order. Tests cover both ID orders, a map imported earlier in
  the same set, an independent other-floor calibration, and calibration undo.
- Application flow preflights the candidate set against the baseline before
  append, then replays and checks cancellation/budget before atomic publication
  and readback. Read-only, cancelled and stale cases are exercised, with stale
  revisions mapped to `Conflict`. Architecture and locked dependency gates
  pass. No evidence here establishes full UI behavior or promotion beyond the
  bounded increment.

## 6. Independently rerun validation

All commands ran from the exact review worktree with local/offline Cargo
resolution; no online retry was needed.

- `cargo test --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application -- --test-threads=1` — PASS, **337 passed, 2 ignored**.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application --all-targets -- -D warnings` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 locked packages.
- `python3 tools/ledger.py check` — PASS, 5,396 source blocks, 438 explicit ID occurrences, 447 headings.

The full workspace suite was not run. Passing tests and static gates do not
resolve the ordering finding.

## 7. Ledger/status/evidence consistency

The regenerated traceability matrix records MAP-002, MAP-003, MAPB-001 and
MAPB-002 as `IN_PROGRESS`, leaving the broader obligations `NOT_STARTED`; the
ledger and source inventory checks pass. The validation packet and proposed
ADR correctly call this a bounded increment, explicitly disclaim pixel-stream
decodability/displayability, and do not claim Phase 0 exit. `STATUS.md`, the
ADR index, and the candidate packet still carry their pre-review “independent
review pending” labels; reconcile those labels and attach this finding outcome
when the candidate is revised/integrated. The source-qualified ledger has no
review evidence for this report yet, as expected before integration.

## 8. Report commit and worktree

Only this report was changed and committed on the isolated review branch. The
exact report commit ID and post-commit clean status are supplied in the
reviewer handoff, since a commit cannot embed its own hash.

## 9. Unresolved risks and limitations

- The MAJOR ordering gap must be fixed and covered by regression tests, then
  the focused suite and affected gates rerun.
- Admission does not inflate IDAT or validate zlib/DEFLATE or pixel contents;
  valid displayability remains unproven by design.
- JPEG/TIFF/WebP, PDF, vector/CAD/geospatial imports, preview/UI workflow,
  multi-point/residual/CRS calibration, evidence migration, the full workspace
  suite, and Phase 0 exit remain out of scope/open.
- Codebase-memory graph tools/resources were unavailable in this session. This
  review makes no graph or index-completeness claims; evidence is from the
  exact candidate source, plan, tests, and scoped documentation.

## 10. Blockers

Candidate approval is blocked by the MAJOR PNG ordering finding above. No
additional environment blocker occurred; the focused offline Cargo run and
all listed gates completed successfully.
