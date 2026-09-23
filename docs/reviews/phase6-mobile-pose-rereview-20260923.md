# Independent Phase 6 mobile-pose follow-up review

Date: 2026-09-23
Reviewer: `/root/phase6_mobile_pose_independent_review`
Candidate: `96e66e1948f773f45fef6cbe5608600cbd4883fa`
Base: `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`

## 1. Decision

**APPROVE for integration as a bounded, platform-neutral pose/anchor contract.**
This approval does not complete Phase 6 or establish mobile, route, hardware,
field-accuracy, or centimeter-accuracy acceptance. No BLOCKER, MAJOR, or MINOR
findings remain in this bounded candidate.

## 2. Scope and plan requirements

The complete authoritative `plan.md` and assigned `AGENTS.md` were read for the
prior independent review. In this fresh worktree both files are unchanged from
the same base (`plan.md` SHA-256
`1e308d236d63520b6569243f09b1e1f3daf62e1e724f3993b4744af8c27dc6a6`; the
candidate/base diff for `plan.md` and `AGENTS.md` is empty). I reread the
task-relevant sections: SUR-003 and Phase 6 (§§6.3 and 17), coordinate/time,
sample, uncertainty and quality contracts (§§7.2, 7.3, 7.20, 7.25, 7.28),
mobile platform/coordinator boundaries (§§10.10 and 10.11), validation
requirements (§§16.9 and 16.14), architecture (§14.2), and definition of done
(§23).

The candidate borrows provider-supplied pose evidence and applies a bounded
yaw/translation correction. It is not a camera/IMU estimator, a mobile app, a
route evaluator, or a field-validation result.

## 3. Follow-up findings

**MPR-005 — closed.** At an exact pose timestamp, `fuse_at` now checks each
available immediately adjacent source segment against the speed limit; at
non-exact timestamps it checks the selected segment. Regressions cover valid
exact evidence and an interior outlier queried at the first, interior, and last
sample timestamps. Thus the former zero-width-bracket bypass is closed.

**MPR-006 — closed for the documented conditional covariance contract.** Each
anchor correction now requires known covariance for both its exact source pose
and target control point. It rotates source covariance by correction yaw and
uses the documented conservative factor-two sum. Unknown required covariance
keeps fused covariance unknown while supported position can remain known.
Separate-anchor tests verify both a dominating known anchor-source covariance
and unknown anchor-source covariance. The typed result scope and validation
note explicitly condition the bound on supplied orientations and exclude
orientation and frame-calibration uncertainty.

The covariance rotations and two factor-two bounds were also checked against
the implementation algebra: the first bounds arbitrary correlation between
source-pose and target-control-point error; the second bounds arbitrary
correlation between query-position and correction error. Endpoint covariance
interpolation is convex, and configured process variance is added separately.
No independence attestation or squared-weight uncertainty reduction is
present.

The three prior minor findings are also closed: timelines reject duplicate
PoseIds; the public type docs and validation note require compatible
right-handed, +z-up metric frames; and the historical review is cited by its
branch and immutable revision without a dangling Markdown link. The referenced
historical artifact is retrievable from revision
`3ac33f5d2ae8f909f82a0e461652271d67fa95e5`.

## 4. Historical MAJOR findings

The four prior MAJOR concerns were checked against their original review at
`3ac33f5d2ae8f909f82a0e461652271d67fa95e5` and are not reintroduced or
overstated:

| Historical concern | Current disposition |
|---|---|
| Public fusion bypassed validated correction/anchor authority | `fuse_at` accepts only `ValidatedAnchors`, whose private fields bind it to a validated borrowed timeline and exact anchor pose/time identities. Caller-supplied target coordinates remain explicitly unauthenticated. |
| Replayable independence evidence reduced covariance | There is no independence-attestation or squared-weight API; conservative bounds are used. |
| Public archive construction escaped live-owned-memory budgets | The archive, serialization, replay, and owned publication APIs are absent. The bounded input contract explicitly does not constrain caller-owned allocations. |
| Workspace architecture gate omitted the crate | `kyberia-mobile-pose` is registered in the numerical layer with no direct external dependencies; the independent architecture check passes. |

The earlier non-MAJOR shortest-arc concern remains closed by normalized
shortest-arc interpolation and the tested +π antipodal convention. This
candidate makes no route-role or archive-validation claim.

## 5. Other contract audit

- **Identity and time:** samples share session, producer, source frame and
  clock epoch; monotonic nanoseconds are strictly ordered; any known clock model
  must match the sample epoch. Anchors are ordered in that epoch and each binds
  to an exact timeline timestamp and PoseId. Queries reject a different epoch.
- **Transforms and orientation:** the implementation applies yaw plus
  translation, retains pitch and roll, never resolves frame graphs or scale,
  and explicitly requires compatible +z-up metric frames. Missing orientation
  remains unknown rather than being synthesized.
- **Support and rejection:** no pose/anchor extrapolation; oversize interpolated
  gaps, implausible source speed, and `NotTracking` query evidence produce
  explicit unknown results. A `NotTracking` anchor is rejected.
- **Bounds and trust:** sample work is capped at 100,000; anchor work at 512.
  Timeline duplicate checking uses a bounded temporary ordered set; anchor-ID
  checking is bounded by the 512-anchor cap. No async cancellation hook is
  exposed, but these synchronous operations have hard finite input bounds.
  Caller-owned input memory and anchor authenticity are explicitly outside the
  contract.
- **Serialization/canonical behavior:** none is implemented or claimed.

## 6. Independent checks

All commands ran in this fresh review worktree at the exact candidate. No broad
workspace build or device/field validation was run.

| Check | Result |
|---|---|
| `cargo test -p kyberia-mobile-pose --locked --offline` | PASS — 21 integration tests (15 fusion, 6 validation); doc tests pass |
| `cargo clippy -p kyberia-mobile-pose --all-targets --locked --offline -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `python3 tools/architecture.py check` | PASS |
| `python3 tools/source_inventory.py check` | PASS — 522 locked external packages |
| `python3 tools/ledger.py check` | PASS — 5,396 source blocks, 438 explicit ID occurrences, 447 headings |
| `git diff --check 05953134d24666e8483cbfdb7d9aacd0ce4e6e48..96e66e1948f773f45fef6cbe5608600cbd4883fa` | PASS |

Ledger/source hashes for `fusion.rs`, `model.rs`, the validation note, and
`Cargo.lock` match their recorded SHA-256 values. The generated traceability
matrix carries the current `plan.md` source hash. The candidate records itself
as pending independent rereview in `STATUS.md`; this report supplies that
rereview without claiming any platform or field gate passed. Codebase-memory
MCP was unavailable in this reviewer context; no graph/index claim is made.

## 7. Traceability and remaining gates

The traceability matrix correctly marks only the bounded pose/covariance and
anchor-correction pieces of SUR-003 and Phase 6 as `IN_PROGRESS`. Camera/IMU
acquisition, iOS RoomPlan/ARKit, Android ARCore/RTT, relocalization,
degraded-tracking accuracy acceptance, phone profiles, route comparison,
remote-sensor pairing, guided-route features and synchronized surveys remain
open. The Phase 6 exit criteria—including route improvement versus
uniform-speed interpolation—remain `NOT_STARTED`; STATUS.md continues to say
the current phase is Phase 0 and Phase 6 is open.

## 8. Ten-field handoff

1. **Verdict:** APPROVE integration of this bounded software contract only.
2. **Plan anchors:** §§6.3/SUR-003, 7.2, 7.3, 7.20, 7.25, 7.28, 10.10, 10.11,
   14.2, 16.9, 16.14, 17/Phase 6, and 23.
3. **Base/head:** `05953134d24666e8483cbfdb7d9aacd0ce4e6e48` →
   `96e66e1948f773f45fef6cbe5608600cbd4883fa`.
4. **Reviewed paths:** `crates/mobile-pose/` source/tests/manifest; root Cargo
   lock/workspace configuration; validation note; `STATUS.md`; implementation
   ledger/traceability; architecture manifest and its check.
5. **Invariants:** borrowed immutable timeline; unique PoseIds; one source,
   session, frame and clock epoch; exact anchor pose/time binding; compatible
   +z-up metric-frame precondition; no extrapolation; conservative/unknown
   covariance semantics; bounded caller-input work; no authenticity,
   caller-memory, serialization, mobile-platform, or field-accuracy claim.
6. **Checks:** 21 tests, strict Clippy, fmt, architecture, inventory, ledger,
   and candidate diff check all pass; no broad build or hardware/route checks.
7. **Traceability:** the bounded contract advances only the cited foundation
   pieces; platform, route, pairing, relocalization and Phase 6 exit gates stay
   open. No graph claim.
8. **Reviewer artifact:** only this report is changed/committed by the reviewer;
   final report commit and worktree cleanliness are returned in the handoff.
9. **Findings/risks:** no remaining findings; scope limitations and lack of
   async cancellation are described above, not presented as product evidence.
10. **Integration gate:** no technical blocker to integrating the bounded
    candidate; retain all platform/field gates as open and do not mark Phase 6
    complete.
