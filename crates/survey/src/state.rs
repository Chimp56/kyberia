use crate::*;
mod wire;
pub use wire::{DecodedPointSurvey, PointSnapshotDecodeReceipt, PointSnapshotInputVersion};

/// Receipt schema evolves independently of the V1 capture configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointSnapshotSchemaVersion {
    #[serde(rename = "2")]
    V2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointPhase {
    Capturing,
    Paused,
    Completed,
    Cancelled,
    Failed { reason: Text },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveWindow {
    start: u64,
    end: Option<u64>,
}

/// Compact admitted evidence, linked to immutable canonical observations.
/// Deserialization verifies consistency, not cryptographic source authenticity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedEvidence<V = Evidence<Text>> {
    observation_id: ObservationId,
    captured: u64,
    result_age: Option<Seconds>,
    admitted: u64,
    bssid: MacAddress,
    rssi: Evidence<Dbm>,
    noise: Evidence<Dbm>,
    pose: Evidence<PoseReference>,
    calibration: Evidence<CalibrationState>,
    raw_source: Evidence<ArtifactReference>,
    source_version: V,
    parser_version: Text,
    quality: Vec<QualityFlag>,
    dwell: Option<MonotonicWindow>,
    tuned_frequency: Evidence<Hertz>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: PointSnapshotSchemaVersion,
    config: PointConfig,
    phase: PointPhase,
    started: u64,
    last: u64,
    windows: Vec<ActiveWindow>,
    records: Vec<AcceptedEvidence>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PointSurvey(Snapshot);
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PointProgress {
    pub metrics: BTreeMap<PointMetric, u32>,
    pub dwell_seconds: Vec<Seconds>,
    pub active_seconds: Seconds,
    pub active_windows: Vec<MonotonicWindow>,
    pub ready: bool,
    pub observation_ids: Vec<ObservationId>,
    pub manual_position_assumption: bool,
}

impl PointSurvey {
    pub fn start(config: PointConfig, at: MonotonicTimestamp) -> Result<Self, SurveyError> {
        config.preflight()?;
        if at.epoch != config.data().epoch {
            return Err(SurveyError::WrongClock);
        }
        Ok(Self(Snapshot {
            schema_version: PointSnapshotSchemaVersion::V2,
            config,
            phase: PointPhase::Capturing,
            started: at.nanoseconds,
            last: at.nanoseconds,
            windows: vec![ActiveWindow {
                start: at.nanoseconds,
                end: None,
            }],
            records: vec![],
        }))
    }
    pub const fn phase(&self) -> &PointPhase {
        &self.0.phase
    }
    pub const fn config(&self) -> &PointConfig {
        &self.0.config
    }
    fn check_time(&self, at: MonotonicTimestamp) -> Result<(), SurveyError> {
        if at.epoch != self.0.config.data().epoch {
            return Err(SurveyError::WrongClock);
        }
        if at.nanoseconds < self.0.last {
            return Err(SurveyError::ReversedTime);
        }
        Ok(())
    }
    pub fn advance(&self, at: MonotonicTimestamp) -> Result<Self, SurveyError> {
        if !matches!(self.0.phase, PointPhase::Capturing | PointPhase::Paused) {
            return Err(SurveyError::InvalidTransition);
        }
        self.check_time(at)?;
        let mut next = self.clone();
        next.0.last = at.nanoseconds;
        Ok(next)
    }
    pub fn pause(&self, at: MonotonicTimestamp) -> Result<Self, SurveyError> {
        if self.0.phase != PointPhase::Capturing {
            return Err(SurveyError::InvalidTransition);
        }
        let mut next = self.advance(at)?;
        next.close_window(at.nanoseconds);
        next.0.phase = PointPhase::Paused;
        Ok(next)
    }
    pub fn resume(&self, at: MonotonicTimestamp) -> Result<Self, SurveyError> {
        if self.0.phase != PointPhase::Paused {
            return Err(SurveyError::InvalidTransition);
        }
        if self.0.windows.len() >= MAX_WINDOWS {
            return Err(SurveyError::Limit);
        }
        let mut next = self.advance(at)?;
        next.0.windows.push(ActiveWindow {
            start: at.nanoseconds,
            end: None,
        });
        next.0.phase = PointPhase::Capturing;
        Ok(next)
    }
    fn close_window(&mut self, at: u64) {
        if let Some(window) = self.0.windows.last_mut()
            && window.end.is_none()
        {
            window.end = Some(at);
        }
    }
    pub fn finish(&self, at: MonotonicTimestamp) -> Result<Self, SurveyError> {
        if self.0.phase != PointPhase::Capturing {
            return Err(SurveyError::InvalidTransition);
        }
        let mut next = self.advance(at)?;
        if !next.progress().ready {
            return Err(SurveyError::NotReady);
        }
        next.close_window(at.nanoseconds);
        next.0.phase = PointPhase::Completed;
        Ok(next)
    }
    pub fn cancel(&self, at: MonotonicTimestamp) -> Result<Self, SurveyError> {
        let mut next = self.advance(at)?;
        next.close_window(at.nanoseconds);
        next.0.phase = PointPhase::Cancelled;
        Ok(next)
    }
    pub fn fail(&self, at: MonotonicTimestamp, reason: Text) -> Result<Self, SurveyError> {
        let mut next = self.advance(at)?;
        next.close_window(at.nanoseconds);
        next.0.phase = PointPhase::Failed { reason };
        Ok(next)
    }

    pub fn admit(
        &self,
        observation: &ObservationEnvelope,
        received: MonotonicTimestamp,
    ) -> Result<Self, SurveyError> {
        if self.0.phase != PointPhase::Capturing {
            return Err(SurveyError::InvalidTransition);
        }
        self.check_time(received)?;
        if self.0.records.len() >= MAX_RECORDS {
            return Err(SurveyError::Limit);
        }
        let cfg = self.0.config.data();
        let data = observation.data();
        if data.session_id != cfg.session_id {
            return Err(SurveyError::WrongSession);
        }
        if data.source.source_id != cfg.source_id
            || data.source.collector_id != cfg.collector_id
            || data.source.adapter_version != cfg.adapter_version
        {
            return Err(SurveyError::WrongSource);
        }
        if !cfg.allow_synthetic
            && (data.source.kind == SourceKind::SyntheticFixture
                || data.quality.contains(&QualityFlag::SyntheticFixture))
        {
            return Err(SurveyError::UnusableQuality);
        }
        if bad_quality(&data.quality) {
            return Err(SurveyError::UnusableQuality);
        }
        let captured = data
            .time
            .monotonic
            .as_known()
            .ok_or(SurveyError::TimestampUnavailable)?;
        if captured.epoch != cfg.epoch {
            return Err(SurveyError::WrongClock);
        }
        if captured.nanoseconds > received.nanoseconds {
            return Err(SurveyError::FutureCapture);
        }
        self.0.config.check_pose(&data.pose)?;
        let (signal, identity, age) = match (&data.payload, cfg.mode) {
            (ObservationPayload::Scan(scan), CaptureMode::Scan) => {
                let age = *scan.result_age.as_known().ok_or(SurveyError::StaleScan)?;
                if age > cfg.maximum_scan_age {
                    return Err(SurveyError::StaleScan);
                }
                (&scan.signal, &scan.identity, Some(age))
            }
            (ObservationPayload::Frame(frame), CaptureMode::Frame) => {
                (&frame.signal, &frame.identity, None)
            }
            _ => return Err(SurveyError::UnsupportedPayload),
        };
        let bssid = *identity
            .bssid
            .as_known()
            .ok_or(SurveyError::UnsupportedPayload)?;
        if let Some(age) = age {
            let actual_age = received.nanoseconds - captured.nanoseconds;
            if actual_age > nanos(cfg.maximum_scan_age)? {
                return Err(SurveyError::StaleScan);
            }
            if nanos(age)? > actual_age {
                return Err(SurveyError::InconsistentScanAge);
            }
        }
        let start = self
            .0
            .windows
            .last()
            .ok_or(SurveyError::InvalidSnapshot)?
            .start;
        if captured.nanoseconds < start {
            return Err(SurveyError::CaptureOutsidePoint);
        }
        if self.0.records.iter().any(|r| r.observation_id == data.id) {
            return Err(SurveyError::DuplicateObservation);
        }
        if self
            .0
            .records
            .iter()
            .any(|r| r.captured == captured.nanoseconds && r.bssid == bssid)
        {
            return Err(SurveyError::DuplicateSourceSample);
        }
        let mut dwell = None;
        let mut tuned_frequency = Evidence::Unknown(UnknownReason::NotObservable);
        if !incomplete_dwell(&data.quality)
            && let Evidence::Known(context) = &data.dwell
            && let Evidence::Known(window) = context.window
        {
            if window.start().epoch != cfg.epoch {
                return Err(SurveyError::WrongClock);
            }
            if window.end().nanoseconds > received.nanoseconds {
                return Err(SurveyError::FutureDwell);
            }
            dwell = Some(window);
            tuned_frequency = context.tuned_channel.primary_frequency.clone();
        }
        let record = AcceptedEvidence {
            observation_id: data.id,
            captured: captured.nanoseconds,
            result_age: age,
            admitted: received.nanoseconds,
            bssid,
            rssi: signal.rssi_dbm.clone(),
            noise: signal.noise_dbm.clone(),
            pose: data.pose.clone(),
            calibration: signal.calibration.clone(),
            raw_source: data.raw_source.clone(),
            source_version: data.source.source_version.clone(),
            parser_version: data.source.parser_version.clone(),
            quality: data.quality.clone(),
            dwell,
            tuned_frequency,
        };
        let mut next = self.clone();
        next.0.last = received.nanoseconds;
        next.0.records.push(record);
        Ok(next)
    }

    pub fn progress(&self) -> PointProgress {
        let cfg = self.0.config.data();
        let mut metrics: BTreeMap<_, _> =
            cfg.metrics.keys().map(|metric| (*metric, 0_u32)).collect();
        for record in &self.0.records {
            let target = matches!(cfg.target, Target::AnyBssid)
                || matches!(cfg.target,Target::Bssid(bssid) if record.bssid==bssid);
            if target {
                for (metric, count) in &mut metrics {
                    let present = match metric {
                        PointMetric::Rssi => record.rssi.as_known().is_some(),
                        PointMetric::Noise => record.noise.as_known().is_some(),
                        PointMetric::Snr => record
                            .rssi
                            .as_known()
                            .zip(record.noise.as_known())
                            .is_some_and(|(rssi, noise)| rssi.difference(*noise).is_ok()),
                    };
                    *count += u32::from(present);
                }
            }
        }
        let active_nanos = self
            .0
            .windows
            .iter()
            .map(|w| w.end.unwrap_or(self.0.last) - w.start)
            .sum::<u64>();
        let mut dwell_totals = Vec::new();
        for required in &cfg.channels {
            let mut intervals = Vec::new();
            for record in &self.0.records {
                if record.tuned_frequency.as_known() != Some(&required.frequency) {
                    continue;
                }
                if let Some(dwell) = record.dwell {
                    intervals.push((dwell.start().nanoseconds, dwell.end().nanoseconds));
                }
            }
            intervals.sort_unstable();
            let mut union = Vec::new();
            let mut merged: Option<(u64, u64)> = None;
            for (start, end) in intervals {
                match merged {
                    Some((a, b)) if start <= b => merged = Some((a, b.max(end))),
                    Some((a, b)) => {
                        union.push((a, b));
                        merged = Some((start, end));
                    }
                    None => merged = Some((start, end)),
                }
            }
            if let Some((a, b)) = merged {
                union.push((a, b));
            }
            // Both interval sets are disjoint and ordered. A two-pointer sweep
            // avoids materializing every dwell × pause-window intersection.
            let mut total = 0;
            let mut active_index = 0;
            for (start, end) in union {
                while let Some(active) = self.0.windows.get(active_index) {
                    let active_end = active.end.unwrap_or(self.0.last);
                    if active_end <= start {
                        active_index += 1;
                        continue;
                    }
                    if active.start >= end {
                        break;
                    }
                    total += end.min(active_end) - start.max(active.start);
                    if active_end >= end {
                        break;
                    }
                    active_index += 1;
                }
            }
            dwell_totals.push(total);
        }
        let ready = metrics
            .iter()
            .all(|(metric, count)| *count >= cfg.metrics[metric].get())
            && active_nanos >= nanos(cfg.minimum_active_time).expect("validated duration")
            && dwell_totals
                .iter()
                .zip(&cfg.channels)
                .all(|(total, channel)| {
                    *total >= nanos(channel.minimum_dwell).expect("validated duration")
                });
        PointProgress {
            metrics,
            dwell_seconds: dwell_totals.into_iter().map(seconds).collect(),
            active_seconds: seconds(active_nanos),
            active_windows: self
                .0
                .windows
                .iter()
                .map(|window| {
                    MonotonicWindow::new(
                        MonotonicTimestamp {
                            epoch: cfg.epoch,
                            nanoseconds: window.start,
                        },
                        MonotonicTimestamp {
                            epoch: cfg.epoch,
                            nanoseconds: window.end.unwrap_or(self.0.last),
                        },
                    )
                    .expect("validated active window")
                })
                .collect(),
            ready,
            observation_ids: self.0.records.iter().map(|r| r.observation_id).collect(),
            manual_position_assumption: matches!(cfg.pose_policy, PosePolicy::ManualAnchor { .. }),
        }
    }

    fn validate(&self) -> Result<(), SurveyError> {
        self.0.config.preflight()?;
        if self.0.records.len() > MAX_RECORDS
            || self.0.windows.is_empty()
            || self.0.windows.len() > MAX_WINDOWS
            || self.0.started > self.0.last
            || self.0.windows[0].start != self.0.started
        {
            return Err(SurveyError::InvalidSnapshot);
        }
        let mut previous = self.0.started;
        for (i, window) in self.0.windows.iter().enumerate() {
            let end = window.end.unwrap_or(self.0.last);
            if window.start < previous
                || end < window.start
                || end > self.0.last
                || (window.end.is_none()
                    && (i + 1 != self.0.windows.len() || self.0.phase != PointPhase::Capturing))
            {
                return Err(SurveyError::InvalidSnapshot);
            }
            previous = end;
        }
        if self.0.phase == PointPhase::Capturing
            && self.0.windows.last().is_none_or(|w| w.end.is_some())
        {
            return Err(SurveyError::InvalidSnapshot);
        }
        let mut ids = BTreeSet::new();
        let mut samples = BTreeSet::new();
        let mut previous_admitted = self.0.started;
        for record in &self.0.records {
            if !ids.insert(record.observation_id)
                || !samples.insert((record.captured, record.bssid.0))
                || record.captured > record.admitted
                || record.admitted > self.0.last
                || record.admitted < previous_admitted
                || bad_quality(&record.quality)
                || record.quality.len() > 32
            {
                return Err(SurveyError::InvalidSnapshot);
            }
            previous_admitted = record.admitted;
            if !self.0.windows.iter().any(|w| {
                record.captured >= w.start && record.admitted <= w.end.unwrap_or(self.0.last)
            }) {
                return Err(SurveyError::InvalidSnapshot);
            }
            if !self.0.config.data().allow_synthetic
                && record.quality.contains(&QualityFlag::SyntheticFixture)
            {
                return Err(SurveyError::InvalidSnapshot);
            }
            self.0.config.check_pose(&record.pose)?;
            match (self.0.config.data().mode, record.result_age) {
                (CaptureMode::Scan, Some(age)) => {
                    let actual_age = record.admitted - record.captured;
                    if actual_age > nanos(self.0.config.data().maximum_scan_age)?
                        || nanos(age)? > actual_age
                    {
                        return Err(SurveyError::InvalidSnapshot);
                    }
                }
                (CaptureMode::Frame, None) => {}
                _ => return Err(SurveyError::InvalidSnapshot),
            }
            if let Some(window) = record.dwell
                && (incomplete_dwell(&record.quality)
                    || window.start().epoch != self.0.config.data().epoch
                    || window.end().nanoseconds > record.admitted
                    || !window.contains(MonotonicTimestamp {
                        epoch: self.0.config.data().epoch,
                        nanoseconds: record.captured,
                    }))
            {
                return Err(SurveyError::InvalidSnapshot);
            }
        }
        if self.0.phase == PointPhase::Completed && !self.progress().ready {
            return Err(SurveyError::InvalidSnapshot);
        }
        Ok(())
    }
}
fn incomplete_dwell(flags: &[QualityFlag]) -> bool {
    flags.iter().any(|q| {
        matches!(
            q,
            QualityFlag::DroppedEvents | QualityFlag::PartialCapture | QualityFlag::Disconnected
        )
    })
}
fn bad_quality(flags: &[QualityFlag]) -> bool {
    flags.iter().any(|q| {
        matches!(
            q,
            QualityFlag::Stale
                | QualityFlag::ContradictorySourceFields
                | QualityFlag::Malformed
                | QualityFlag::Saturated
                | QualityFlag::InferredTimestamp
                | QualityFlag::ClockUncertain
        )
    })
}
impl TryFrom<Snapshot> for PointSurvey {
    type Error = SurveyError;
    fn try_from(snapshot: Snapshot) -> Result<Self, Self::Error> {
        let state = Self(snapshot);
        state.validate()?;
        Ok(state)
    }
}
impl From<PointSurvey> for Snapshot {
    fn from(state: PointSurvey) -> Self {
        state.0
    }
}
