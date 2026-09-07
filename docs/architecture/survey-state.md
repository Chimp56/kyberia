# Point-survey admission contract

`kyberia-survey` is an inward, deterministic application core depending only on
`kyberia-domain` and Serde. It controls one point capture from one source and one
source-local monotonic epoch. It has no clock, capture, network, storage or UI
side effects. Each accepted transition returns a new immutable state; an error
leaves the original untouched. The caller stores canonical observation envelopes
and persists the returned point state together with its observation references.

## Requirements and current boundary

This increment implements the passive point-state portion of plan §6.3 SUR-001,
§7.1 evidence separation, §7.2 frame references, §7.3 source-local time,
§7.4 capability admission, §7.7 measured channel completeness and §7.20 capture
window representation. It advances the Phase 1 click-to-measure workflow but does
not complete that user feature or all SUR-001 quality-gate dimensions.

The following remain open: capture/UI/store wiring, annotations, repeated-point
orchestration, statistical variance gates, active-test synchronization and result
references, spectrum completion, multiple sensors and clock alignment, orientation
stability, calibrated aggregation and the complete quality scorecard. Continuous
paths (SUR-002), floor switching within a session and all other survey modes remain
separate work. Unsupported active/spectrum gates are not offered in this contract.

## Configuration and evidence

`PointConfig::new` validates a V1 configuration containing point/session IDs,
position anchor and covariance, frame ID, explicit known/unknown map calibration,
collector/source IDs and adapter version, epoch, probed capabilities, target BSSID,
requested RSSI/noise/SNR sample counts, selected channel dwell minima and minimum
active duration. RSSI count is mandatory and nonzero. Durations use seconds at the
public boundary and checked, conservatively rounded nanoseconds internally.

Starting requires the configured collector's `NearbyScan` or `MonitorFrames`
capability to be available. Noise and SNR requirements additionally require
`NoiseDbm`. Conditional, unavailable and absent capabilities fail preflight;
capability availability does not guarantee every sample contains that metric.
Optional additional capability requirements are checked the same way.

`RequireReported` demands known position/covariance in the anchor frame, bounds
Euclidean displacement and per-axis standard deviations. `ManualAnchor` permits
missing reported position with an explicit assumption flag; any reported position
must still match the frame and distance bound. Manual placement is not represented
as a new sensor pose measurement. The anchor and map-calibration reference are
immutable throughout the point. Starting a different floor/frame requires another
point configuration. Resolving the frame/calibration against a project is the
outer application's responsibility.

Each admission checks immutable envelope validity, matching session/source/
collector/adapter version, known monotonic timestamp and epoch, pose policy and
payload mode. Synthetic fixtures require explicit opt-in and remain flagged.
Stale, malformed, saturated, contradictory, uncertain-clock and inferred-time
evidence is rejected. The current source's monotonic clock is passed as the
`received` time; a host clock from another epoch cannot be substituted.

For scans, source cache age must be known. Canonical `CaptureTime` is always actual
capture time, never API retrieval time. It must fall in the current active
interval. Admission time minus actual capture time is the total age including
cache, queue and transport, and must stay within the configured maximum. Reported
cache age at retrieval must not exceed that total age. Cache age is neither
subtracted from actual capture nor added to total age again. For example, capture
at 1.1 s, retrieval/admission at 1.3 s and reported age 0.2 s is valid for a point
started at 1.0 s. Unknown actual capture remains unknown and cannot be admitted.
The pair `(actual capture, BSSID)` is deduplicated in addition to observation ID,
so rereading a cached scan does not advance counts. This relies on a truthful
adapter timestamp and age contract; the state machine cannot infer a hidden cache.
Frame timestamps
refer to actual capture and have no scan cache age. Buffered frames can be admitted
only within the currently active interval with an acceptable reported pose.

Point state retains compact evidence and observation IDs, source/parser versions,
raw-source and radio-calibration references, quality flags and reported pose. The
canonical source envelope remains the authority for complete provenance. Counts
are derived from present values, never fabricated zero measurements. SNR requires
both powers from the same signal reading and a finite checked dB difference.
Calibration state is preserved; no calibration correction is applied here.

## State transitions and progress

`start → capturing`; capturing permits `admit`, `advance`, `pause`, `finish`,
`cancel` and `fail`. Paused permits `advance`, `resume`, `cancel` and `fail`.
Completed, cancelled and failed states are terminal. `finish` requires every
configured evidence/count/dwell/time gate. Time advancement alone cannot satisfy
sample counts. Cancellation/failure retain partial observations and expose their
terminal state. `progress.ready` describes achieved quality gates; callers must
also inspect `phase`, since partial evidence in a cancelled point may meet gates.

Progress exposes numeric metric counts, selected-channel dwell durations, active
duration/windows, observation IDs and the manual-position assumption. It is not an
RSSI aggregation or a percentage based on elapsed time. Applications inspect
canonical observations by ID to display actual values and unknown reasons.

Channel completeness is the union of reported actual dwell intervals for each
requested tuned frequency, intersected with the point's active windows. Overlapping
observations cannot double count dwell; pause gaps and time outside the point do
not count. Unknown windows/frequencies and incomplete capture marked dropped,
partial or disconnected contribute no dwell. Their otherwise admissible measured
signal values may still contribute metric counts. Dwell-only health events are
not yet ingested: a channel with no observation can therefore remain incomplete
even when a future collector could attest a full visit. Missing completeness is
never interpreted as AP absence. This is accounting, not a radio scheduler.

## Serialization, bounds and replay

The point snapshot contains its versioned configuration and is revalidated on
deserialization: interval order/phase, epoch, record uniqueness, sample origin and
admission times, reported cache age, pose bounds, scan freshness, dwell consistency and completed
gates. Unknown point fields and duplicate metric requirements are rejected.
Replaying the same caller-supplied transitions and immutable envelopes produces
the same state. This increment does not introduce a persisted transition log;
canonical command/log integration remains an application task. Deserialization
establishes consistency, not authenticity; imported receipts must be reconciled
with their canonical observation IDs by the storage/application boundary.

Limits are 4096 records, 1024 active windows and 256 selected frequencies per point.
Callers must enforce an outer serialized byte/depth limit before decoding because
Serde allocates vectors before semantic length validation. Each immutable
transition clones the bounded point snapshot; aggregate admission cost therefore
grows quadratically with record count. This is not a continuous frame-ingestion
buffer. Full raw streams stay in observation storage. Exceeding the limit returns
`Limit`; it never silently truncates. A future persistent receipt collection can
replace the cloning representation without changing point semantics.

## Validation and performance evidence

Acceptance tests precede implementation and cover actual evidence completion,
unknown/unsupported metrics, pause gaps, stale/cached/queued scans, source/frame/
clock mismatches, manual and uncertain pose, BSSID filtering, incomplete dwell,
frame capture and finite SNR, malformed/future snapshots, terminal transitions and
deterministic replay. Property tests prove elapsed time cannot replace evidence
and compare the dwell sweep against an independent discrete interval oracle.

Commands from the isolated workspace:

```sh
cargo fmt --all -- --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline
cargo test --manifest-path crates/survey/Cargo.toml --offline --release -- --ignored --nocapture
```

The explicit release benchmark uses 4096 observations, 1024 active windows, one
frequency and two metric samples required, on the available macOS ARM64 host,
Rust 1.98.1. Initial result: total admission 1679.025 ms and final progress
calculation 0.129 ms. This is a local baseline, not a cross-machine regression
threshold. Per-frequency dwell computation sorts at most the record count and
intersects disjoint unions with a linear sweep, avoiding a record-by-window
Cartesian product. Numerical durations are checked against exact integer
nanosecond reference counts before conversion to public seconds.
