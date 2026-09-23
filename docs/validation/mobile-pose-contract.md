# Mobile pose fusion contract (candidate)

This is a platform-neutral Phase 6 foundation candidate on
`feat/phase6-mobile-pose-current`. It covers a bounded numerical contract, not
a mobile companion. The current phase remains Phase 0, and Phase 6 remains
open pending platform, route-comparison, pairing, and independent-review gates.

## Plan traceability

This increment partially advances **SUR-003** in §6.3 (`plan.md` lines
908–916), especially recording pose/covariance/tracking evidence and applying
drift corrections only through typed anchors. The matching Phase 6 deliverable
is §17 (`plan.md` lines 4138–4152), specifically AR path anchors and drift
correction. Supporting contracts are §7.2 coordinate frames, §7.3 monotonic
and UTC time, §7.20 spatial samples, §7.25 uncertainty decomposition, and
§7.28 the pose-quality dimension of the quality vector.

The implementation consumes already acquired `PoseReference` values; it does
not estimate camera/IMU motion. It does not satisfy SUR-003's continuous
camera/IMU companion requirement, relocalization, remote sensor correlation,
or platform capture. It implements neither iOS RoomPlan/ARKit nor Android
ARCore/RTT from §10.10 or the encrypted mobile/desktop coordination in §10.11.

## Contract

`PoseTimeline` is a borrowed view. Construction checks the sample and work
limits before scanning, then requires one session, producer, source frame,
clock epoch, strictly increasing monotonic time, and unique pose IDs. It uses
a temporary ordered identity set for at most the hard cap of 100,000 samples
and discards that set after validation; the sample slice remains borrowed. A
known clock model must name the same epoch. `ValidatedAnchors` is an opaque
borrowed view with private fields: callers cannot hand fusion arbitrary
correction segments. Each control point must have a unique ID, one target
frame, a time and pose ID that match an exact sample in that timeline, a usable
source yaw, and a tracking state other than `NotTracking`.

`fuse_at` interpolates only within both the pose and anchor supports. It rejects
oversized pose or anchor gaps and implausible source speed, returns explicit
unknown evidence outside support, and never extrapolates. The correction is a
deterministic yaw-plus-translation transform; pitch and roll are retained from
the source pose. Angles follow the shortest arc, with the exact antipodal case
choosing +pi. Results retain source session/producer/frame, exact supporting
pose and anchor IDs, interpolation fractions, tracking state, and algorithm
version.

The 2.5D correction assumes the caller has already put source and target
coordinates in compatible right-handed, +z-up metric frames. Frame IDs do not
prove that precondition, and this crate does not resolve frame graphs, tilt,
scale, or frame-calibration uncertainty. Exact-sample queries also check the
adjacent source segment(s), when present, against the speed limit; a zero-width
query bracket does not bypass that gate.

`DriftAnchor.alignment_covariance` describes only the target control-point
position covariance in the target frame, conditional on the supplied target
yaw; it does not include uncertainty in the source pose used to derive the
correction. For each anchor, the implementation rotates that exact source
pose covariance by the correction yaw and forms a correction-translation bound
`C_correction = 2 * (R C_source R^T + C_target)`. If either required covariance
is unknown, the correction covariance is unknown. This factor-two sum bounds
arbitrary cross-correlation without claiming independence.

For the query, the implementation interpolates the two anchor-correction
bounds, rotates/interpolates the query pose covariance, and forms
`C_fused = 2 * (R C_query R^T + C_correction_interpolated) + Q`, where `Q` is
the configured process-variance contribution. The outer factor two is another
conservative bound for unknown correlation between query pose and correction.
Unknown covariance remains unknown while supported position may remain known;
known values must not be interpreted as total uncertainty. The typed scope is
`ConditionalOnInputOrientation`: orientation covariance and frame-calibration
covariance are not represented by current input types. There is no independence
attestation or squared-weight reduction. Anchor target coordinates and
modality are caller-supplied evidence; this crate does not authenticate a
surveyor, validate a marker image, or resolve a floor-plan frame graph.

The hard limits cap algorithmic work at 100,000 samples and 512 anchors. They
do not bound memory already owned by caller input. The crate has no archive,
serialization, replay, or owned publication API.

## Review history and boundary

The prior historical Phase 6 candidate was rejected with four MAJOR findings;
its report is identified by revision
`3ac33f5d2ae8f909f82a0e461652271d67fa95e5` on branch
`review/phase-six-mobile-pose-final` at
`docs/reviews/phase-six-mobile-pose-final-review.md` (that path is not present
in this worktree). This candidate addresses the bounded findings as follows:

- **MPR-001, public correction bypass:** fusion now accepts only
  `ValidatedAnchors`, which can be created only after exact source timeline,
  session/producer/frame, pose/time, target-frame consistency, and anchor-ID
  checks.
  Caller-supplied control-point coordinates remain an explicit trust boundary;
  cryptographic anchor authority is not implemented.
- **MPR-002, replayable independence attestations:** this candidate has no
  independence attestation or independent-weight mode; it uses the conservative
  covariance bound above.
- **MPR-003, archive constructor under-budgeting:** there is no archive or
  returned owned-data constructor in this candidate. Borrowed inputs and work
  limits do not claim to enforce a memory budget on caller-owned allocations.
- **MPR-004, missing architecture registration:**
  `kyberia-mobile-pose` is registered as a numerical crate with no external
  dependencies; the architecture check passes.
- **MPR-MIN-001, duplicate pose IDs:** timeline validation rejects repeated
  pose identities before the timeline can be used for evidence links.
- **MPR-MIN-002, planar frame precondition:** the API and contract note now
  state the compatible right-handed, +z-up metric-frame caller precondition.
- **MPR-MIN-003, missing historical report path:** this note cites the
  historical branch/revision and identifies its absent path without a broken
  relative link.

The first independent review of candidate `ea7a4a4c445bf6f311db4a270846b9e042f305e4`
rejected integration pending fixes to exact-sample speed gating (MPR-005) and
anchor-source covariance propagation (MPR-006). Those author-side corrections
and regression tests are present here; a fresh independent rereview is pending.

These statements describe this API and focused tests only; they do not assert
that a new independent review has accepted the implementation.

## Focused validation

Executed in the isolated worktree with its locked dependencies and offline:

```text
cargo test -p kyberia-mobile-pose --locked --offline
  PASS: 21 integration tests, 0 failures
cargo clippy -p kyberia-mobile-pose --all-targets --locked --offline -- -D warnings
  PASS
cargo fmt --all -- --check
  PASS
python3 tools/architecture.py check
  PASS
python3 tools/source_inventory.py check
  PASS: 522 locked external packages
python3 tools/ledger.py check
  PASS after evidence hashes and generated traceability were refreshed
```

The tests cover frame/time/source binding, exact anchor binding, no
extrapolation, pose/anchor gap and speed gates, shortest-arc yaw, limited
tracking and unknown quality, unknown orientation/covariance retention,
correction transforms, evidence links, and conservative covariance/process
variance behavior. Regressions specifically cover exact first/interior/last
sample speed checks, duplicate pose IDs, and known/unknown covariance at
separated anchor-source timestamps. They are deterministic software contract
tests, not device or route validation.

No ARKit, RoomPlan, ARCore, RTT, camera/IMU acquisition, mobile build,
remote-sensor time fusion, hardware, field-route comparison, relocalization,
or centimeter-accuracy validation was exercised. Per §16.9 and §16.14, no
accuracy target or superiority claim is established.
