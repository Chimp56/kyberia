# Observed RSSI selection boundary

`kyberia-observation-analysis` is the application-layer bridge between
canonical `ObservationEnvelope` values, persisted point-survey assignments, and
the pure `kyberia-spatial-analysis` model. It owns selection provenance and
does not open a project, decode Parquet, call a platform collector, or access
UI state.

## Input contract

The outer application supplies a bounded set of observation IDs, a committed
project revision, an exact target BSSID, explicit floor/frame scope, and a
`SelectionSourceBinding` copied from the verified indexed-query receipt. It
also supplies immutable observation envelopes and point surveys loaded from a
single store snapshot. `SurveyInput` pairs each survey with its floor because
the current point-survey configuration owns a coordinate frame but does not
own floor identity.

The application store adapter must query by observation ID using the indexed
observation-member table. It must reject or report missing IDs, enforce its
query bounds, and provide a revision-consistent result before calling
`ValidatedObservedRssiSet::build`. The adapter maps the store receipt's
committed revision and selected chunk descriptors to the storage-independent
`SelectionSourceBinding::from_verified_query` constructor only after the
store has verified the descriptors and bytes. The constructor enforces the
canonical sorted hash list and resource bounds, but does not claim to verify
arbitrary hashes or open artifacts. This crate deliberately has no dependency
on `kyberia-project-store`; the concrete adapter can be added without making
storage a domain or numerical dependency.

## Selection semantics

The envelope is authoritative for BSSID, RSSI, source/session identity,
payload kind, capture time, calibration, channel, dwell, quality, and raw
source reference. A survey contributes only the admitted observation IDs and
its spatial/time assignment. Association records intentionally do not carry a
second BSSID or RSSI value, so the bridge always rechecks those fields in the
envelope.

Only scan observations from scan surveys or frame observations from frame
surveys can produce this RSSI metric. A known `Evidence<Dbm>` RSSI and a known
exact `MacAddress` equal to the requested BSSID are required. Unknown values,
SSID-only identities, identity-graph BSS nodes, active measurements, spectrum
energy, health payloads, and Kismet raw PHY integers cannot become measured
RSSI samples. Each rejected requested ID remains in the manifest with its
reason.

Strict survey records use a reported capture pose when the envelope supplies
one. If the survey uses `ManualAnchor` and the envelope pose is unknown, the
configured anchor is used and recorded as `SelectedPointAnchor`; the capture
time remains the envelope's original evidence. Receipt associations always use
the association's selected anchor and retain API-window or receipt timing in a
separate field. Receipt timing never overwrites or estimates RF capture time,
and receipt associations never advance strict survey completion gates.

Position covariance is copied into both the provenance record and the spatial
sample. An unknown covariance remains unknown. The current numerical engine
does not propagate covariance into a calibrated dB confidence interval, so its
`uncertainty_db` output remains an explicit unknown value.

Uncalibrated or unknown calibration is accepted only when the request opts in;
no correction is applied. Calibration outside its valid range is always
rejected. Strict captures reject unusable capture-quality flags. Receipt
associations permit clock/drop/partial-capture flags that the survey
association contract permits, while still rejecting malformed or
contradictory source data; the flags remain in the manifest for downstream
quality policy.

## Selection manifest

The V1 manifest is canonical JSON with schema
`kyberia.observed-rssi-selection/1` and media type
`application/kyberia-observed-rssi-selection+json`. It contains:

* project revision, exact target BSSID, floor/frame, source/session/adapter
  scope, requested IDs, unknown/calibration policy, and the exact sorted
  selected chunk hashes plus the committed query revision;
* the metric artifact, metric-definition hash, signal aggregation, spatial
  method, and spatial configuration;
* a sorted selected record for each accepted RSSI observation, including
  numeric RSSI, source and radio identity evidence, pose and covariance,
  capture and association timing, calibration, channel/dwell, quality,
  measurement method, and raw-source reference;
* a sorted rejection record for every requested ID that was missing or did not
  satisfy the evidence contract.

Every selected record includes an immutable assignment pose and the original
observation pose separately. Receipt records include a hash of the complete
canonical point association plus its point and observation IDs. Strict manual
anchor records carry the survey point, anchor pose, and explicit manual-anchor
pose policy. Validation rejects reordered IDs, duplicate or incomplete
results, changed assignment poses, inconsistent capture/receipt provenance,
calibration-policy changes, unusable quality, stale scans, and unknown fields.

The manifest's canonical bytes are content-addressed as an
`ArtifactReference`. Those bytes are used as `SpatialInputs.source_artifact`,
so a tile's numeric `Sample` IDs resolve back through the retained manifest to
the exact BSSID and evidence record used for computation. `tile` composes the
validated samples with the existing spatial model and returns the model's
explicit Observed, Interpolated, Extrapolated, or Unknown classes.

## Bounds and failure behavior

The bridge accepts at most 4,096 requested IDs, 4,096 supplied envelopes, and
1,024 surveys. It rejects invalid spatial configurations and incompatible
metric definitions before producing a manifest. Canonical manifest decoding
is limited to 1 MiB and nesting depth 32. There is no fallback to zero RSSI,
fabricated pose/time, guessed BSSID, or a different analysis engine.

## Storage composition

`Bundle::read_observation_selection_by_id` returns the bounded canonical
envelopes and an `ObservationQueryReceipt` containing the committed project
revision and exact verified selected chunk descriptors. The outer application
maps those descriptor hashes into `SelectionSourceBinding`; it must not accept
caller-provided hashes as a substitute for the verified receipt. The bridge
records the binding in the V1 manifest without importing project-store types or
changing `spatial_analysis::Sample`. Broad BSSID scans remain outside this
path until a separately verified identity index is implemented.

## Plan alignment

This boundary implements the application-side portion of PAS-001, ANA-005,
§§7.1–7.5, 7.9–7.10, 7.20–7.25, 10.1/10.5/10.6/10.8, 11.5–11.7, 11.8,
12.2/12.6/12.7, Phase 0/Phase 1, and the provenance, determinism, unknown,
and evidence requirements in the definition of done. It does not claim
Kismet live runtime validation, calibrated uncertainty, or wall-aware
interpolation. Live Kismet runtime remains a separate integration gate.
