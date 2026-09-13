//! Generic active execution orchestration.
//!
//! This module contains no socket implementation.  Clocks, cancellation and
//! the connector are inward ports, which makes deadline and cancellation
//! behavior testable without contacting an external address.

use crate::pure::{ActiveSchedule, ScheduledSample, result_id};
use kyberia_domain::{
    active::{
        ActiveEndpoint, ActiveInterval, ActiveResult, ActiveSample, ActiveSampleOutcome,
        ActiveTestRun, ActiveValidationError,
    },
    evidence::{Evidence, UnknownReason},
    time::MonotonicTimestamp,
    units::Milliseconds,
};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectResult {
    Connected,
    ConnectionRefused,
    Timeout,
    Unreachable,
    PermissionDenied,
    Error,
    /// The connector observed cancellation before the bounded attempt ended.
    /// This is distinct from a network timeout and carries no RTT evidence.
    Cancelled,
}

pub trait TcpConnector {
    /// Attempt one TCP handshake to this literal address. `timeout` bounds
    /// this call, and implementations must poll `cancellation` while waiting
    /// so cancellation cannot be held behind a long kernel connect wait.
    /// Port semantics come from `ActiveSocketAddr`: this is a numeric TCP
    /// destination port and never a service-name or protocol lookup.
    fn connect(
        &mut self,
        target: kyberia_domain::active::ActiveSocketAddr,
        timeout: Duration,
        cancellation: &dyn Cancellation,
    ) -> ConnectResult;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepResult {
    Complete,
    Cancelled,
}

pub trait MonotonicClock {
    /// Nanoseconds from a monotonic origin. Implementations must never move
    /// backwards during one execution.
    fn now_nanos(&mut self) -> u64;

    /// Sleeping is a separate inward port so deterministic tests can advance
    /// a fake clock without wall-clock delays.
    /// Sleep for at most `duration`, polling cancellation. Implementations
    /// must return `Cancelled` promptly instead of blocking for the complete
    /// duration after cancellation is observed.
    fn sleep_for(&mut self, duration: Duration, cancellation: &dyn Cancellation) -> SleepResult;
}

pub trait Cancellation {
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancelled;

impl Cancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActiveMeasurementError {
    ScheduleMismatch,
    ClockRegression,
    InvalidSample(ActiveValidationError),
    InvalidResult(ActiveValidationError),
}

impl std::fmt::Display for ActiveMeasurementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScheduleMismatch => f.write_str("active execution inputs do not match schedule"),
            Self::ClockRegression => f.write_str("active monotonic clock moved backwards"),
            Self::InvalidSample(error) => write!(f, "active sample rejected: {error}"),
            Self::InvalidResult(error) => write!(f, "active result rejected: {error}"),
        }
    }
}

impl std::error::Error for ActiveMeasurementError {}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveExecutionReport {
    run_id: kyberia_domain::identity::ActiveTestRunId,
    interval_id: kyberia_domain::identity::ActiveIntervalId,
    started: MonotonicTimestamp,
    finished: MonotonicTimestamp,
    results: Vec<ActiveResult>,
}

impl ActiveExecutionReport {
    pub const fn run_id(&self) -> kyberia_domain::identity::ActiveTestRunId {
        self.run_id
    }

    pub const fn interval_id(&self) -> kyberia_domain::identity::ActiveIntervalId {
        self.interval_id
    }

    pub const fn started(&self) -> MonotonicTimestamp {
        self.started
    }

    pub const fn finished(&self) -> MonotonicTimestamp {
        self.finished
    }

    pub fn results(&self) -> &[ActiveResult] {
        &self.results
    }

    pub fn samples(&self) -> impl Iterator<Item = &ActiveSample> {
        self.results.iter().flat_map(ActiveResult::samples)
    }
}

fn duration_from_seconds(seconds: kyberia_domain::units::Seconds) -> Duration {
    Duration::from_secs_f64(seconds.get())
}

fn stamp(
    schedule_start: MonotonicTimestamp,
    execution_start: u64,
    now: u64,
) -> Result<MonotonicTimestamp, ActiveMeasurementError> {
    let elapsed = now
        .checked_sub(execution_start)
        .ok_or(ActiveMeasurementError::ClockRegression)?;
    let nanos = schedule_start
        .nanoseconds
        .checked_add(elapsed)
        .ok_or(ActiveMeasurementError::ClockRegression)?;
    Ok(MonotonicTimestamp {
        epoch: schedule_start.epoch,
        nanoseconds: nanos,
    })
}

fn outcome_and_rtt(
    result: ConnectResult,
    elapsed_nanos: u64,
    overall_deadline_reached: bool,
) -> (ActiveSampleOutcome, Evidence<Milliseconds>) {
    let outcome = if overall_deadline_reached {
        ActiveSampleOutcome::Timeout
    } else {
        match result {
            ConnectResult::Connected => ActiveSampleOutcome::Success,
            ConnectResult::ConnectionRefused => ActiveSampleOutcome::ConnectionRefused,
            ConnectResult::Timeout => ActiveSampleOutcome::Timeout,
            ConnectResult::Unreachable => ActiveSampleOutcome::Unreachable,
            ConnectResult::PermissionDenied => ActiveSampleOutcome::PermissionDenied,
            ConnectResult::Error => ActiveSampleOutcome::Error,
            ConnectResult::Cancelled => ActiveSampleOutcome::Cancelled,
        }
    };
    if outcome.is_success() {
        let millis = elapsed_nanos as f64 / 1e6;
        match Milliseconds::new(millis) {
            Ok(value) => (outcome, Evidence::Known(value)),
            Err(_) => (
                ActiveSampleOutcome::Error,
                Evidence::Unknown(UnknownReason::FailedTest),
            ),
        }
    } else {
        let reason = if outcome.is_cancelled() {
            UnknownReason::NotMeasured
        } else {
            UnknownReason::FailedTest
        };
        (outcome, Evidence::Unknown(reason))
    }
}

fn cancellation_sample(
    schedule_sample: &ScheduledSample,
    run: &ActiveTestRun,
    interval: &ActiveInterval,
    at: MonotonicTimestamp,
) -> Result<ActiveSample, ActiveMeasurementError> {
    ActiveSample::new(
        schedule_sample.id(),
        run.id(),
        interval.id(),
        schedule_sample.endpoint().id(),
        schedule_sample.endpoint().tier(),
        schedule_sample.endpoint().attribution().clone(),
        schedule_sample.ordinal(),
        at,
        at,
        ActiveSampleOutcome::Cancelled,
        Evidence::Unknown(UnknownReason::NotMeasured),
        run.provenance().clone(),
    )
    .map_err(ActiveMeasurementError::InvalidSample)
}

fn timeout_sample(
    schedule_sample: &ScheduledSample,
    run: &ActiveTestRun,
    interval: &ActiveInterval,
    at: MonotonicTimestamp,
) -> Result<ActiveSample, ActiveMeasurementError> {
    ActiveSample::new(
        schedule_sample.id(),
        run.id(),
        interval.id(),
        schedule_sample.endpoint().id(),
        schedule_sample.endpoint().tier(),
        schedule_sample.endpoint().attribution().clone(),
        schedule_sample.ordinal(),
        at,
        at,
        ActiveSampleOutcome::Timeout,
        Evidence::Unknown(UnknownReason::FailedTest),
        run.provenance().clone(),
    )
    .map_err(ActiveMeasurementError::InvalidSample)
}

#[allow(clippy::too_many_arguments)]
fn execute_one<C: MonotonicClock, T: TcpConnector, X: Cancellation>(
    scheduled: &ScheduledSample,
    run: &ActiveTestRun,
    interval: &ActiveInterval,
    schedule: &ActiveSchedule,
    clock: &mut C,
    connector: &mut T,
    cancellation: &X,
    deadline_nanos: u64,
    execution_start: u64,
    attempt_started: u64,
    last_now: &mut u64,
) -> Result<ActiveSample, ActiveMeasurementError> {
    if attempt_started >= deadline_nanos {
        return timeout_sample(
            scheduled,
            run,
            interval,
            stamp(schedule.start(), execution_start, attempt_started)?,
        );
    }
    if cancellation.is_cancelled() {
        return cancellation_sample(
            scheduled,
            run,
            interval,
            stamp(schedule.start(), execution_start, attempt_started)?,
        );
    }
    let remaining = Duration::from_nanos(deadline_nanos - attempt_started);
    let timeout = duration_from_seconds(schedule.per_attempt_timeout()).min(remaining);
    let result = connector.connect(
        scheduled.endpoint().target().address(),
        timeout,
        cancellation,
    );
    let after = clock.now_nanos();
    if after < attempt_started {
        return Err(ActiveMeasurementError::ClockRegression);
    }
    *last_now = after;
    let finished = stamp(schedule.start(), execution_start, after)?;
    let result = if cancellation.is_cancelled() {
        ConnectResult::Cancelled
    } else {
        result
    };
    let elapsed = after - attempt_started;
    let (outcome, rtt) = outcome_and_rtt(result, elapsed, after > deadline_nanos);
    ActiveSample::new(
        scheduled.id(),
        run.id(),
        interval.id(),
        scheduled.endpoint().id(),
        scheduled.endpoint().tier(),
        scheduled.endpoint().attribution().clone(),
        scheduled.ordinal(),
        stamp(schedule.start(), execution_start, attempt_started)?,
        finished,
        outcome,
        rtt,
        run.provenance().clone(),
    )
    .map_err(ActiveMeasurementError::InvalidSample)
}

/// Execute every scheduled sample in deterministic order.  A cancellation
/// or overall deadline does not discard the remainder: each remaining sample
/// receives a typed `Cancelled` or `Timeout` outcome, preserving the declared
/// sample count and endpoint attribution. The connector is called at most
/// once at a time, so actual concurrency is always bounded by one even when
/// a larger configured bound is carried for future schedulers.
pub fn execute<C: MonotonicClock, T: TcpConnector, X: Cancellation>(
    run: &ActiveTestRun,
    interval: &ActiveInterval,
    schedule: &ActiveSchedule,
    clock: &mut C,
    connector: &mut T,
    cancellation: &X,
) -> Result<ActiveExecutionReport, ActiveMeasurementError> {
    if schedule.run_id() != run.id()
        || schedule.interval_id() != interval.id()
        || interval.run_id() != run.id()
        || schedule.samples().len()
            != run
                .endpoints()
                .len()
                .saturating_mul(interval.samples_per_endpoint() as usize)
    {
        return Err(ActiveMeasurementError::ScheduleMismatch);
    }
    let execution_start = clock.now_nanos();
    let deadline_nanos = execution_start
        .checked_add(duration_from_seconds(schedule.duration()).as_nanos() as u64)
        .ok_or(ActiveMeasurementError::ClockRegression)?;
    let mut last_now = execution_start;
    let mut samples_by_endpoint: BTreeMap<_, Vec<ActiveSample>> = BTreeMap::new();
    let mut cancelled = false;
    for scheduled in schedule.samples() {
        let now = clock.now_nanos();
        if now < last_now {
            return Err(ActiveMeasurementError::ClockRegression);
        }
        last_now = now;
        let at = stamp(schedule.start(), execution_start, now)?;
        let sample = if cancelled || cancellation.is_cancelled() {
            cancelled = true;
            cancellation_sample(scheduled, run, interval, at)?
        } else {
            let offset_nanos = duration_from_seconds(scheduled.planned_offset()).as_nanos() as u64;
            let target_start = execution_start.saturating_add(offset_nanos);
            if now < target_start {
                let sleep_result =
                    clock.sleep_for(Duration::from_nanos(target_start - now), cancellation);
                let after_sleep = clock.now_nanos();
                if after_sleep < last_now {
                    return Err(ActiveMeasurementError::ClockRegression);
                }
                last_now = after_sleep;
                if sleep_result == SleepResult::Cancelled || cancellation.is_cancelled() {
                    cancelled = true;
                    cancellation_sample(
                        scheduled,
                        run,
                        interval,
                        stamp(schedule.start(), execution_start, after_sleep)?,
                    )?
                } else {
                    let now = last_now;
                    execute_one(
                        scheduled,
                        run,
                        interval,
                        schedule,
                        clock,
                        connector,
                        cancellation,
                        deadline_nanos,
                        execution_start,
                        now,
                        &mut last_now,
                    )?
                }
            } else if now >= deadline_nanos {
                timeout_sample(scheduled, run, interval, at)?
            } else if cancellation.is_cancelled() {
                cancelled = true;
                cancellation_sample(scheduled, run, interval, at)?
            } else {
                execute_one(
                    scheduled,
                    run,
                    interval,
                    schedule,
                    clock,
                    connector,
                    cancellation,
                    deadline_nanos,
                    execution_start,
                    now,
                    &mut last_now,
                )?
            }
        };
        if sample.outcome().is_cancelled() {
            cancelled = true;
        }
        samples_by_endpoint
            .entry(scheduled.endpoint().id())
            .or_default()
            .push(sample);
    }
    let finished_nanos = last_now.max(execution_start);
    let finished = stamp(schedule.start(), execution_start, finished_nanos)?;
    let mut results = Vec::with_capacity(run.endpoints().len());
    let mut endpoints = run.endpoints().to_vec();
    endpoints.sort_by_key(ActiveEndpoint::id);
    for endpoint in endpoints {
        let samples = samples_by_endpoint
            .remove(&endpoint.id())
            .unwrap_or_default();
        let result = ActiveResult::from_samples(
            result_id(run.id(), interval.id(), endpoint.id()),
            run.id(),
            interval.id(),
            &endpoint,
            samples,
        )
        .map_err(ActiveMeasurementError::InvalidResult)?;
        results.push(result);
    }
    Ok(ActiveExecutionReport {
        run_id: run.id(),
        interval_id: interval.id(),
        started: schedule.start(),
        finished,
        results,
    })
}
