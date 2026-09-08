# Observed RSSI selection review

Candidate: `5ac17faf92639b9f7338a06ccc41a0f1990c9b37`.
Author: `/root/operation_log_review_luna`. Independent reviewer: `/root`.
Disposition: APPROVED after correction `3fc36d3bd4042babd1834ae20653726e9ef8b0da`; integrated as `f12fcb7` and `3d9d059`. No unresolved BLOCKER or MAJOR findings. Findings below retain the original review history.

MAJOR: strict selection uses membership in `PointSurvey::progress().observation_ids` without comparing the supplied canonical envelope to the snapshot's private `AcceptedEvidence` record. The stored record contains captured time, BSSID, RSSI/noise, pose, calibration, raw reference, source/parser version, quality, dwell and tuned frequency. An opaque observation ID does not establish equality of those fields. A same-ID substituted envelope may therefore become a measured spatial sample with evidence different from that admitted by the survey. Require a narrow survey validation method and bridge admission check, with matching and substituted RSSI/time/pose regressions. The author is implementing the correction in the isolated selection worktree.

MINOR: an envelope with no survey assignment is currently rejected as `MissingObservation`. Preserve the distinction between absent source evidence and absent spatial association.

Review is ongoing. Author-reported passing tests do not close these findings or imply product workflow validation. Receipt association closure is separately being centralized in the native-pipeline correction; the two survey helpers must retain strict-versus-receipt timing semantics.

MAJOR: `validate_metric` admits any definition with unit `Dbm` and matching spatial method. `MetricDefinition::from_spec` supports explicit alternative semantics; the unit alone cannot establish observed Wi-Fi RSSI. Require compatible observed-RSSI evidence/compute/selection semantics and a same-unit incompatible-definition regression. The author has been notified; this finding is resolved by admission of only the canonical observed-RSSI definition.

Independent correction review: retained strict admission evidence is checked through a narrow survey method; missing association is distinct from missing observation. Historical snapshots do not retain every original envelope field, and the documented comparison scope does not claim otherwise. Root reran `cargo test -p kyberia-observation-analysis -p kyberia-survey --locked --offline` (10 analysis and 34 survey tests passed, one survey benchmark ignored), focused Clippy with `-D warnings`, and formatting. Source-binding hashes remain an outer-adapter trust boundary; concrete store wiring and user-facing heatmaps are open.
