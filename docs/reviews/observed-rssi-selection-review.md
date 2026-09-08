# Observed RSSI selection review

Candidate: `5ac17faf92639b9f7338a06ccc41a0f1990c9b37`.
Author: `/root/operation_log_review_luna`. Independent reviewer: `/root`.
Disposition: REQUEST_CHANGES; candidate is not integrated.

MAJOR: strict selection uses membership in `PointSurvey::progress().observation_ids` without comparing the supplied canonical envelope to the snapshot's private `AcceptedEvidence` record. The stored record contains captured time, BSSID, RSSI/noise, pose, calibration, raw reference, source/parser version, quality, dwell and tuned frequency. An opaque observation ID does not establish equality of those fields. A same-ID substituted envelope may therefore become a measured spatial sample with evidence different from that admitted by the survey. Require a narrow survey validation method and bridge admission check, with matching and substituted RSSI/time/pose regressions. The author is implementing the correction in the isolated selection worktree.

MINOR: an envelope with no survey assignment is currently rejected as `MissingObservation`. Preserve the distinction between absent source evidence and absent spatial association.

Review is ongoing. Author-reported passing tests do not close these findings or imply product workflow validation. Receipt association closure is separately being centralized in the native-pipeline correction; the two survey helpers must retain strict-versus-receipt timing semantics.
