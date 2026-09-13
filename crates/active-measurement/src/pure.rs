//! Side-effect-free active scheduling and result helpers.

use kyberia_domain::{
    active::{
        ActiveAuthorization, ActiveEndpoint, ActiveInterval, ActiveMeasurementProvenance,
        ActiveTestRun, ActiveValidationError,
    },
    identity::{ActiveEndpointId, ActiveIntervalId, ActiveSampleId, ActiveTestRunId, Text},
    time::MonotonicTimestamp,
    units::Seconds,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const SCHEDULE_VERSION: &str = "rf-atlas-active-schedule/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleOrder {
    EndpointIdThenOrdinal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Randomization {
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledSample {
    id: ActiveSampleId,
    endpoint: ActiveEndpoint,
    ordinal: u32,
    sequence: u32,
    planned_offset: Seconds,
    planned_at: MonotonicTimestamp,
}

impl ScheduledSample {
    pub const fn id(&self) -> ActiveSampleId {
        self.id
    }

    pub fn endpoint(&self) -> &ActiveEndpoint {
        &self.endpoint
    }

    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub const fn sequence(&self) -> u32 {
        self.sequence
    }

    pub const fn planned_offset(&self) -> Seconds {
        self.planned_offset
    }

    pub const fn planned_at(&self) -> MonotonicTimestamp {
        self.planned_at
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveSchedule {
    schedule_version: Text,
    run_id: ActiveTestRunId,
    interval_id: ActiveIntervalId,
    run_started: MonotonicTimestamp,
    run_deadline: MonotonicTimestamp,
    start: MonotonicTimestamp,
    duration: Seconds,
    limits: kyberia_domain::active::ActiveTestLimits,
    order: ScheduleOrder,
    randomization: Randomization,
    authorization: ActiveAuthorization,
    run_provenance: ActiveMeasurementProvenance,
    interval_provenance: ActiveMeasurementProvenance,
    samples: Vec<ScheduledSample>,
}

impl ActiveSchedule {
    pub fn schedule_version(&self) -> &Text {
        &self.schedule_version
    }

    pub const fn run_id(&self) -> ActiveTestRunId {
        self.run_id
    }

    pub const fn interval_id(&self) -> ActiveIntervalId {
        self.interval_id
    }

    pub const fn start(&self) -> MonotonicTimestamp {
        self.start
    }

    pub const fn run_started(&self) -> MonotonicTimestamp {
        self.run_started
    }

    pub const fn run_deadline(&self) -> MonotonicTimestamp {
        self.run_deadline
    }

    pub const fn duration(&self) -> Seconds {
        self.duration
    }

    pub const fn per_attempt_timeout(&self) -> Seconds {
        self.limits.per_attempt_timeout()
    }

    pub const fn minimum_spacing(&self) -> Seconds {
        self.limits.minimum_spacing()
    }

    pub const fn max_concurrency(&self) -> u16 {
        self.limits.max_concurrency()
    }

    pub const fn order(&self) -> ScheduleOrder {
        self.order
    }

    pub const fn randomization(&self) -> Randomization {
        self.randomization
    }

    pub fn authorization(&self) -> &ActiveAuthorization {
        &self.authorization
    }

    pub const fn max_samples_total(&self) -> u32 {
        self.limits.max_samples_total()
    }

    pub const fn limits(&self) -> kyberia_domain::active::ActiveTestLimits {
        self.limits
    }

    pub const fn run_provenance(&self) -> &ActiveMeasurementProvenance {
        &self.run_provenance
    }

    pub const fn interval_provenance(&self) -> &ActiveMeasurementProvenance {
        &self.interval_provenance
    }

    pub fn samples(&self) -> &[ScheduledSample] {
        &self.samples
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    Invalid(ActiveValidationError),
    RunIntervalMismatch,
    IntervalOutsideRun,
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) => write!(f, "invalid active schedule: {error}"),
            Self::RunIntervalMismatch => f.write_str("active run and interval identity mismatch"),
            Self::IntervalOutsideRun => f.write_str("active interval is outside run deadline"),
        }
    }
}

impl std::error::Error for ScheduleError {}

fn duration_nanos(seconds: Seconds) -> u64 {
    (seconds.get() * 1e9).round() as u64
}

fn sample_id(
    run_id: ActiveTestRunId,
    interval_id: ActiveIntervalId,
    endpoint_id: ActiveEndpointId,
    ordinal: u32,
) -> ActiveSampleId {
    let mut hasher = Sha256::new();
    hasher.update(run_id.bytes());
    hasher.update(interval_id.bytes());
    hasher.update(endpoint_id.bytes());
    hasher.update(ordinal.to_be_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    if bytes == [0; 16] {
        bytes[15] = 1;
    }
    ActiveSampleId::from_bytes(bytes).expect("hash-derived active sample identity is nonzero")
}

pub(crate) fn result_id(
    run_id: ActiveTestRunId,
    interval_id: ActiveIntervalId,
    endpoint_id: kyberia_domain::identity::ActiveEndpointId,
) -> kyberia_domain::identity::ActiveResultId {
    let mut hasher = Sha256::new();
    hasher.update(run_id.bytes());
    hasher.update(interval_id.bytes());
    hasher.update(endpoint_id.bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    if bytes == [0; 16] {
        bytes[15] = 1;
    }
    kyberia_domain::identity::ActiveResultId::from_bytes(bytes)
        .expect("hash-derived active result identity is nonzero")
}

/// Build a deterministic endpoint-id/ordinal schedule.  The executor is
/// intentionally serial in this first production slice; the declared
/// concurrency bound remains part of the contract and is never exceeded.
pub fn build_schedule(
    run: &ActiveTestRun,
    interval: &ActiveInterval,
) -> Result<ActiveSchedule, ScheduleError> {
    if run.id() != interval.run_id() {
        return Err(ScheduleError::RunIntervalMismatch);
    }
    if interval.window().start().epoch != run.started().epoch
        || interval.window().start().nanoseconds < run.started().nanoseconds
        || interval.window().end().nanoseconds > run.deadline().nanoseconds
    {
        return Err(ScheduleError::IntervalOutsideRun);
    }
    if interval.provenance() != run.provenance() {
        return Err(ScheduleError::Invalid(ActiveValidationError::Domain(
            kyberia_domain::ValidationError::Inconsistent(
                "active interval provenance differs from run",
            ),
        )));
    }
    let total_samples = run
        .endpoints()
        .len()
        .checked_mul(interval.samples_per_endpoint() as usize)
        .ok_or(ScheduleError::Invalid(ActiveValidationError::Domain(
            kyberia_domain::ValidationError::ResourceLimit("active schedule sample count"),
        )))?;
    if total_samples > run.limits().max_samples_total() as usize
        || total_samples > kyberia_domain::active::MAX_ACTIVE_SAMPLES as usize
    {
        return Err(ScheduleError::Invalid(ActiveValidationError::Domain(
            kyberia_domain::ValidationError::ResourceLimit("active schedule sample count"),
        )));
    }
    let interval_duration_nanos = interval
        .window()
        .end()
        .nanoseconds
        .checked_sub(interval.window().start().nanoseconds)
        .ok_or(ScheduleError::Invalid(ActiveValidationError::Domain(
            kyberia_domain::ValidationError::ReversedTime,
        )))?;
    let required_nanos = (total_samples.saturating_sub(1) as u64)
        .checked_mul(duration_nanos(run.limits().minimum_spacing()))
        .and_then(|offset| offset.checked_add(duration_nanos(run.limits().per_attempt_timeout())))
        .ok_or(ScheduleError::Invalid(ActiveValidationError::Domain(
            kyberia_domain::ValidationError::ResourceLimit("active schedule rate"),
        )))?;
    if required_nanos > interval_duration_nanos {
        return Err(ScheduleError::Invalid(ActiveValidationError::Domain(
            kyberia_domain::ValidationError::OutOfRange("active interval rate"),
        )));
    }
    let mut endpoints = run.endpoints().to_vec();
    endpoints.sort_by_key(ActiveEndpoint::id);
    let spacing = run.limits().minimum_spacing();
    let mut samples = Vec::with_capacity(
        endpoints
            .len()
            .saturating_mul(interval.samples_per_endpoint() as usize),
    );
    let mut sample_ids = BTreeSet::new();
    let mut sequence = 0u32;
    for endpoint in endpoints {
        for ordinal in 0..interval.samples_per_endpoint() {
            let offset_nanos = duration_nanos(spacing).saturating_mul(u64::from(sequence));
            let planned_nanos = interval
                .window()
                .start()
                .nanoseconds
                .checked_add(offset_nanos)
                .ok_or(ScheduleError::Invalid(ActiveValidationError::Domain(
                    kyberia_domain::ValidationError::OutOfRange("active schedule timestamp"),
                )))?;
            let planned_at = MonotonicTimestamp {
                epoch: interval.window().start().epoch,
                nanoseconds: planned_nanos,
            };
            let id = sample_id(run.id(), interval.id(), endpoint.id(), ordinal);
            if !sample_ids.insert(id) {
                return Err(ScheduleError::Invalid(ActiveValidationError::Domain(
                    kyberia_domain::ValidationError::Inconsistent(
                        "duplicate active sample identity",
                    ),
                )));
            }
            samples.push(ScheduledSample {
                id,
                endpoint: endpoint.clone(),
                ordinal,
                sequence,
                planned_offset: Seconds::new(offset_nanos as f64 / 1e9)
                    .map_err(ActiveValidationError::Domain)
                    .map_err(ScheduleError::Invalid)?,
                planned_at,
            });
            sequence = sequence.saturating_add(1);
        }
    }
    let schedule_version = Text::new(SCHEDULE_VERSION)
        .map_err(ActiveValidationError::Domain)
        .map_err(ScheduleError::Invalid)?;
    Ok(ActiveSchedule {
        schedule_version,
        run_id: run.id(),
        interval_id: interval.id(),
        run_started: run.started(),
        run_deadline: run.deadline(),
        start: interval.window().start(),
        duration: interval
            .window()
            .end()
            .elapsed_since(interval.window().start())
            .map_err(ActiveValidationError::Domain)
            .map_err(ScheduleError::Invalid)?,
        limits: run.limits(),
        order: ScheduleOrder::EndpointIdThenOrdinal,
        randomization: Randomization::None,
        authorization: run.authorization().clone(),
        run_provenance: run.provenance().clone(),
        interval_provenance: interval.provenance().clone(),
        samples,
    })
}
