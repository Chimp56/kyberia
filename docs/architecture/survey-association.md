# Receipt-based point association

`kyberia-survey` has two separate ways to relate an observation to a point:

* `PointSurvey::admit` accepts strict capture evidence. It requires actual
  monotonic capture time, source cache age for scans, an acceptable pose and
  reported channel dwell. Its metric and dwell progress is unchanged by the
  receipt policy.
* `PointSurvey::associate_received` creates a versioned
  `PointObservationAssociation` normalized fact for a `ReceivedObservation`.
  This is the path for native managed-mode APIs such as CoreWLAN, where the
  source can report when a result was returned or which API request window
  produced it without exposing RF capture time or receiver pose.

The association copies the envelope's capture time, pose, dwell, scan result
age, channel, calibration, raw reference, source/parser versions and quality
flags. Nothing is rewritten. A CoreWLAN result therefore retains unknown
capture time, `ClockUncertain`, unknown channel dwell and unknown cache age.
The selected point anchor is recorded under the explicit
`selected_point_anchor` basis with method version
`point-receipt-anchor/v1`; this is an assignment to an operator-selected
location, not a fabricated capture pose.

When a source response has a known monotonic return time, it must be at or
after the current active point start and is recorded with the `receipt` basis.
When the return time is unknown, a known API request window can be used only
if its start is at or after the active point start. A window crossing the point
start is ambiguous and a window ending at or before the point start is
outside. The interval boundary is deterministic: a receipt at the point start
is accepted; an API window with zero positive duration at that boundary is not.
Source response and point timestamps must use the configured monotonic epoch.
Out-of-order responses are rejected. A paused, completed, cancelled or failed
point cannot receive an association; after resume, only the new active window
is eligible.

Associations and strict records share one observation-ID namespace within a
point. Either admission path rejects an ID already present in the other plane,
and snapshot validation enforces the same invariant during replay. They are
stored in the point receipt snapshot under their own V1 fact schema. The
association state advances a separate event-ordering watermark for
deterministic replay but never increments strict metric counts, active duration
or channel dwell. The public
`counts_toward_strict_point_gate` method is permanently false as a guard
against treating receipt timing as capture evidence. The canonical observation
referenced by ID remains the raw/normalized source of truth and must be stored
separately by the application layer.

This policy intentionally accepts `ClockUncertain` quality because the source
response can still provide a usable source-local ordering. That quality flag
and all unknown reasons remain visible in the fact. Receipt association is
therefore useful for a map point and later evidence fusion, while it cannot
silently satisfy freshness, pose, dwell or channel-coverage requirements.
