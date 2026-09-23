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

`PoseTimeline` is an allocation-free borrowed view. Construction checks the
sample and work limits before scanning, then requires one session, producer,
source frame, clock epoch, and strictly increasing monotonic time. A known
clock model must name the same epoch. `ValidatedAnchors` is an opaque borrowed
view with private fields: callers cannot hand fusion arbitrary correction
segments. Each control point must have a unique ID, one target frame, a time
and pose ID that match an exact sample in that timeline, a usable source yaw,
and a tracking state other than `NotTracking`.

`fuse_at` interpolates only within both the pose and anchor supports. It rejects
oversized pose or anchor gaps and implausible source speed, returns explicit
unknown evidence outside support, and never extrapolates. The correction is a
deterministic yaw-plus-translation transform; pitch and roll are retained from
the source pose. Angles follow the shortest arc, with the exact antipodal case
choosing +pi. Results retain source session/producer/frame, exact supporting
pose and anchor IDs, interpolation fractions, tracking state, and algorithm
version.

Position covariance is propagated from translational pose covariance, anchor
alignment covariance, and configured process variance. Since pose/anchor
correlation is unknown, the reported bound uses `2 * (C_pose + C_anchor)`;
there is no independence attestation or squared-weight reduction. Unknown
covariance remains unknown while supported position may remain known. The
typed scope is `ConditionalOnInputOrientation`: orientation covariance and
frame-calibration covariance are not represented by current input types, so
the result must not be described as complete total-position uncertainty.
Anchor target coordinates and modality are caller-supplied evidence; this
crate does not authenticate a surveyor, validate a marker image, or resolve a
floor-plan frame graph.

The hard limits cap algorithmic work at 100,000 samples and 512 anchors. They
do not bound memory already owned by caller input. The crate has no archive,
serialization, replay, or owned publication API.

## Review history and boundary

The prior historical Phase 6 candidate was rejected with four MAJOR findings
in `docs/reviews/phase-six-mobile-pose-final-review.md` on branch
`feat/phase-six-mobile-pose`. This candidate addresses the bounded findings as
follows:

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

These statements describe this API and focused tests only; they do not assert
that a new independent review has accepted the implementation.

## Focused validation

Executed in the isolated worktree with its locked dependencies and offline:

```text
cargo test -p kyberia-mobile-pose --locked --offline
  PASS: 17 integration tests, 0 failures
cargo clippy -p kyberia-mobile-pose --all-targets --locked --offline -- -D warnings
  PASS
cargo fmt --all -- --check
  PASS
python3 tools/architecture.py check
  PASS
python3 tools/source_inventory.py check
  PASS: 522 locked external packages
```

The tests cover frame/time/source binding, exact anchor binding, no
extrapolation, pose/anchor gap and speed gates, shortest-arc yaw, limited
tracking and unknown quality, unknown orientation/covariance retention,
correction transforms, evidence links, and conservative covariance/process
variance behavior. They are deterministic software contract tests, not device
or route validation.

No ARKit, RoomPlan, ARCore, RTT, camera/IMU acquisition, mobile build,
remote-sensor time fusion, hardware, field-route comparison, relocalization,
or centimeter-accuracy validation was exercised. Per §16.9 and §16.14, no
accuracy target or superiority claim is established.
