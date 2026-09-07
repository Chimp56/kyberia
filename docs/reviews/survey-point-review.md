# Independent passive point-survey review

Reviewer: `/root/qa_spec_audit`. Author: `/root/domain`.
Decision: **APPROVED for the selected pure point-state contract**.
One MAJOR timestamp finding was corrected and independently verified; no
unresolved BLOCKER or MAJOR finding remains.

## 1. Scope completed

Read-only review of source/capability admission, pose policy, freshness,
pause/resume windows, dwell accounting, metric counts, terminal states and
snapshot validation. Relevant plan: SUR-001, §§7.1–7.4, 7.7 and 7.20.
The existing canonical domain contracts were inspected as the inward boundary.

## 2. Files reviewed and changed

The author worktree HEAD was `e93927aa866c214835510bccc133dd7d1c32f217`.
The six working-tree hashes below identify actual reviewed source bytes;
uncommitted changes are not attributed to that baseline. Only this report was
authored by the reviewer. Independent test probes were retained outside source.

| File | SHA-256 |
|---|---|
| `crates/survey/Cargo.toml` | `2539c265adbab168fecdab361404a841ecae174fa5e3a3fb48b16c9be2036507` |
| `crates/survey/src/lib.rs` | `e7252b4227ec0fe96db84281e49dfb5ce5052a8f138cdcd1d27995e9739e256b` |
| `crates/survey/src/config.rs` | `c160ee8eb3998564fd9edd2351f0345268a5688fb365ed4128ccb5f48333dac8` |
| `crates/survey/src/state.rs` | `67fa221089e6018c80498d096984cc76ce14ec201f55cc2f14810741ced53437` |
| `crates/survey/tests/point.rs` | `5b339db70fa7b1da23c2813b5d643d2ac87e70d1af6dd8b6531729df50ad1ada` |
| `docs/architecture/survey-state.md` | `616d0cf9dd007125223aaf2be7427886e253a220fe50204bb96c1bc74976d760` |

## 3. Architecture decisions assessed

Survey depends only on the canonical domain and Serde in production, with no
clock, storage, capture or UI side effects. One point uses one explicit source
and monotonic epoch. Immutable transitions preserve original state on failure.
Compact admitted receipts refer to authoritative observations; they are not a
replacement raw evidence store. Completion derives from present metric values,
active intervals and actual reported channel dwell, not a fabricated timer-based
progress percentage. Manual position assignment remains an explicit assumption.

## 4. Finding and tests added

**MAJOR SP-001 — Resolved.** Original scan admission subtracted cache age from
canonical CaptureTime, implicitly reinterpreting capture as retrieval time.
An independent compiled test used point start 1.0 s, actual capture 1.1 s,
retrieval/admission 1.3 s and reported cache age 0.2 s. The code incorrectly
derived an origin of 0.9 s and returned `CaptureOutsidePoint`.

The correction keeps actual capture as the interval/deduplication origin.
Capture-to-admission elapsed time is total age; reported cache age remains
separate evidence and cannot exceed that elapsed time. Age is neither subtracted
from capture nor added again. The compact receipt retains it, and snapshot
validation applies the same temporal semantics. Unknown actual capture remains
unknown and cannot be replaced by API receipt time. The original independent
test now passes. Author regressions cover this exact case, cached rereads,
impossible ages, unknown capture and serialized round trips.

## 5. Tests executed

Commands ran in the isolated domain/survey workspace:

```text
cargo test --workspace --offline --locked
PASS: 17 survey tests, 32 domain tests, 7 domain compile-fail doctests
One explicit release benchmark is intentionally ignored by this command.

cargo clippy --workspace --all-targets --offline --locked -- -D warnings
PASS

cargo fmt --all -- --check
PASS

cargo test --manifest-path crates/survey/Cargo.toml --offline --locked --release -- --ignored --nocapture
PASS: bounded_point_benchmark
4096 records, 1024 active windows, one required frequency:
admission_ms=1713.928, progress_ms=0.115

Independent rustc-compiled probes
PASS: actual capture/cache-age regression;
      u64-max timeline without invented observations or nonfinite progress;
      paused-window rejection, unknown capture rejection and overlapping snapshot windows
```

Property tests compare dwell union/intersection against a discrete independent
oracle and establish that elapsed time alone cannot satisfy sample counts.
Adversarial tests cover stale/delayed scans, duplicate source samples, source and
clock mismatch, uncertain/wrong poses, unsupported or unknown noise, unavailable
capabilities, malformed snapshots, finite SNR and terminal transitions.

## 6. Known limitations

The release timing is a local descriptive baseline, not a hardware-independent
threshold or continuous frame-ingestion throughput claim. Repeated immutable
snapshot cloning has quadratic total admission cost; the point is explicitly
bounded to 4096 records and 1024 active windows. High-volume raw streams remain
outside this aggregate. The benchmark exercises one frequency, not all 256
configurable frequencies.

## 7. Requirements supported

This increment supplies the initial single-source passive point workflow:
explicit start/pause/resume/cancel/fail/finish, stable anchor and epoch, source
matching, real evidence counts, honest missing metrics, deduplicated samples,
bounded age/pose admission, actual dwell union excluding pause gaps, deterministic
transitions and validated snapshot reconstruction.

## 8. Requirements still open

Capture/UI/storage wiring, frame/calibration resolution against project state,
annotations, repeated-point orchestration, statistical stability, orientation
quality, active and spectrum integration, multiple sensors, persisted operation
log, calibrated aggregation, continuous paths and professional quality scorecards
remain independent work. Approval does not complete SUR-001, Phase 1, desktop
click-to-measure acceptance or any native capture runtime gate.

## 9. Risks and follow-up

Outer adapters must preserve actual timestamps and truthful cache age; the state
machine cannot infer hidden cache reuse. In particular, native API receipt time
must not be inserted into canonical CaptureTime to make an otherwise unsupported
measurement pass. Snapshot validation proves internal consistency, not source
authenticity; observation IDs must be reconciled with canonical envelopes during
persistence/import. Byte/depth limits must be applied before serde allocation.

Unknown dwell or incomplete capture contributes no completeness. A missing AP on
an unvisited channel is not evidence of absence. Dwell-only health events and
radio scheduling are not implemented here. Consumers must inspect terminal phase
as well as numeric readiness, because cancelled partial evidence can have met
quality gates without producing a completed point.

## 10. Suggested commit

`feat(survey): implement evidence-driven passive point state machine`

Review artifact: `docs(review): approve corrected point-survey timing contracts`.
