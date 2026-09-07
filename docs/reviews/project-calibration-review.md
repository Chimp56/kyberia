# Independent project and calibration contract review

Reviewer: `/root/qa_spec_audit`. Author: `/root/domain`.
Decision: **APPROVED for the selected initial domain contract increment**.
No unresolved BLOCKER or MAJOR finding. One MINOR numerical finding was corrected
and independently rechecked before approval.

## 1. Scope completed

Read-only review of hierarchy ownership, image calibration, floor transforms,
command admission, receipts/replay, uncertainty preservation and serialized
reference validation. Relevant plan sections: §6 MAP-001/MAP-003, §7.2,
§10.6–10.7, §11.3/11.6, §14.4–14.5, §16.3, §18.1 FND-009/FND-011,
§18.4 MAPB-002/MAPB-008 and §19 iteration 3.

## 2. Files reviewed and changed

Only this review report was authored by the reviewer. Implementation files were
read in the author's isolated worktree. HEAD there was
`e6c4a4b91badc50d5e77c3fd00b2bc3eb444965a`; the following working-tree hashes
identify actual reviewed bytes, rather than attributing uncommitted code to HEAD.

| File | SHA-256 |
|---|---|
| `crates/domain/src/project.rs` | `591d046d0aaa250c5510366271436f39e90de25a7f77a08f7b6fe875a0ff990c` |
| `crates/domain/src/project/entities.rs` | `ee4b90ebb82bf30884bce20c4f79c5c24ed2f50d51ce076ecb1cf4cd1f79305c` |
| `crates/domain/src/project/commands.rs` | `80f7799010fc4c389fb6c697c6cf1ac5739b32e6a23e97339580a3cdbce885c7` |
| `crates/domain/src/spatial/calibration.rs` | `26e06473766117ba24bfa3bf4913604d87cb2d01f1caf4a251d10068014f93a6` |
| `crates/domain/src/identity.rs` | `d3733dbfa573e1b70a634955788cadcbd0449d512f0b108d83ed6e3c36ae8b31` |
| `crates/domain/src/units.rs` | `ec884a619b3569d5254285c8a0d38a2a3615add8d26d4a9e76370cfb6dd1a19e` |
| `crates/domain/src/spatial.rs` | `22dec273cc5ad437d0b1b376d5279d135caec43a1aefe6f4cda42138520db310` |
| `crates/domain/src/lib.rs` | `88d0eb371e99492a8abcdbc7d7c3fdd1334e5832d2b5e5b423daa6d66ec1764e` |
| `crates/domain/tests/project_commands.rs` | `164158b3bc41bbcf45cce4981c616a2ade445ac58fb8f8ee21f86e2ba9fe1e8a` |
| `crates/domain/tests/contracts.rs` | `e52623f0ecdaf06858e8530d12e2a6d38472e634925092996dda48cd7ef6056b` |
| `docs/architecture/domain-contracts.md` | `f0bc1f0ae4dc979d4f93d633a9ee3b3d0b6103e99ba195ade0725b36edc26fb8` |

## 3. Architecture decisions assessed

The pure domain retains canonical identities, metric/pixel distinctions and
explicit coordinate frame ownership. No new production dependency or external
adapter object enters it. Immutable command admission checks full resulting
state, while replay compares the recomputed complete receipt. Actor identity is
provenance rather than authentication; outer application boundaries must still
authorize changes. Snapshots preserve referential integrity but do not purport
to prove historical authenticity. Unknown calibration uncertainty stays explicit.

## 4. Tests added during review

**MINOR PC-001 — Resolved.** Original calibration subtracted the image control
angle from an unrestricted finite target angle. At target direction `1e16`,
controls `(0,0)→(100,100)`, +y-up and known distance 10 m, floating-point
subtraction discarded the control angle and misplaced the second control by
approximately 7.65 m. The author now composes target sine/cosine with normalized
control direction using dot/cross products. A new constructor and serde
regression checks the endpoint independently against the requested direction.

The reviewer also compiled three separate temporary test probes, retaining them
outside source trees without recursive cleanup. These check seven malformed
snapshots, event forgery and stale logical time, and 56 oblique transform cases.
The transform cases combine four control quadrants, both image handedness values
and seven target angles including `1e16` and `±1e20`. Expected second-control
positions are independently calculated from the requested distance/direction;
inverse tolerance is `1e-11` pixels and endpoint tolerance `1e-12` meters for
these modest coordinate scales. Unknown uncertainty is checked unchanged.

## 5. Tests executed

Commands ran in the isolated domain workspace:

```text
cargo test --workspace --offline --locked
PASS: 14 project/calibration tests, 18 existing contracts, 7 compile-fail doctests

cargo clippy --workspace --all-targets --offline --locked -- -D warnings
PASS

cargo fmt --all -- --check
PASS

Independent rustc-compiled probe suite
PASS: 3 tests, including 7 malformed snapshots and 56 transform cases
```

Malformed snapshot cases cover parent-frame mismatch, duplicate map/building
frame identity, missing active calibration map, inconsistent revision index,
stale logical time, invalid unknown active-calibration reason and empty map
artifact. Existing tests also reject duplicate raw JSON keys, unknown parents,
cross-map calibration references and forged inverse/revision receipts.

## 6. Known limitations

Complete editor undo remains open. Import→calibrate→undo calibration→undo import
returns `HasDependents` because retained historical calibration still refers to
the map. This is documented and tested; it is not presented as complete undo.
Evidence-bound floor edits require a future explicit migration flow. Evidence
binding and first persistence must be one outer application transaction.

Per-command cloning and whole-state validation produce approximately quadratic
total history replay cost. The author's documented 10,000-operation timing of
2.417 seconds is a descriptive benchmark, not independently certified throughput
or a passing large-project interaction gate. Persistent data structures,
checkpointed replay or bounded background processing need their own design and
benchmarks before professional large-history use.

## 7. Requirements supported

The selected increment supports canonical project/site/building/floor/map
ownership, signed elevation and yaw transforms, two-point scale/origin/direction
calibration, explicit uncertainty, atomic pure commands, ordered deterministic
receipts, and validated serialized snapshots. Tests substantiate these contracts.

## 8. Requirements still open

Desktop calibration/import UX, operation persistence, complete editor undo,
history-preserving map archival, migration, offline branch merging, geospatial
frame graph, uncertainty propagation, geometry/material editor, radio identity
graph and large-project performance remain independent implementation work.
No complete feature-phase or GUI acceptance claim follows from this review.

## 9. Risks and follow-up

Outer import/IPC adapters must bound input bytes and nesting before serde can
allocate staging values, verify artifact bytes, authenticate actors, and retain
operation history independently of snapshots. Receipt consistency detects
internal forgery against prior state but is not a signature scheme. Numerical
results remain floating-point computations and exact two-point fit does not
establish surveyed physical accuracy. Preserve the documented open capabilities
in traceability when integrating these initial contracts.

## 10. Suggested commit message

`feat(domain): add project commands and frame-safe map calibration`

Review artifact commit: `docs(review): approve project and calibration contracts`.
