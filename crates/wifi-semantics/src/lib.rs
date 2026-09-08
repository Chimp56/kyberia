//! Deterministic Wi-Fi-specific numerical semantics.
//!
//! All aggregation methods are explicit. Missing evidence remains unknown,
//! and arithmetic never inserts a noise floor or converts failure to zero.
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::ObservationId,
    time::MonotonicTimestamp,
    units::{Db, Dbm, Dimensionless, Milliwatts, Probability},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const ALGORITHM_VERSION: SignalAlgorithmVersion = SignalAlgorithmVersion::V1;
/// A single aggregate is bounded to a generous survey-window size. Larger
/// histories must be windowed explicitly so one request cannot exhaust memory.
pub const MAX_SAMPLES: usize = 100_000;

/// Closed semantic version for persisted signal results. A producer must add a
/// new variant when a numerical rule changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalAlgorithmVersion {
    #[serde(rename = "kyberia-wifi-signal/1")]
    V1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfiguration(&'static str),
    DuplicateObservation(ObservationId),
    ClockEpochMismatch,
    NonMonotonicSequence,
    ResourceLimit,
    NumericalFailure,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalSample {
    /// Resolves to the immutable canonical observation carrying sensor,
    /// adapter, measurement-method, channel, calibration, pose and raw-source
    /// provenance. This numerical crate never duplicates or rewrites it.
    pub observation_id: ObservationId,
    pub captured_at: MonotonicTimestamp,
    pub rssi: Dbm,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AggregateMethod {
    MedianDbm,
    TrimmedMeanDbm {
        trim_each_tail: Probability,
    },
    LinearPowerMean,
    PercentileRange {
        lower: Probability,
        upper: Probability,
    },
    EwmaDbm {
        alpha: Probability,
    },
    RobustStateSpaceDbm {
        process_stddev: Db,
        measurement_stddev: Db,
        huber_threshold_stddevs: Dimensionless,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PercentileInterval {
    pub lower: Dbm,
    pub upper: Dbm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalAggregate {
    pub algorithm_version: SignalAlgorithmVersion,
    pub method: AggregateMethod,
    pub estimate: Evidence<Dbm>,
    pub percentile_interval: Evidence<PercentileInterval>,
    pub sample_count: u32,
    /// Order-sensitive methods retain acquisition order. Other methods sort
    /// IDs so equivalent input sets have identical provenance ordering.
    pub observation_order: Vec<ObservationId>,
}

pub fn dbm_to_milliwatts(value: Dbm) -> Result<Milliwatts, Error> {
    let result = 10_f64.powf(value.get() / 10.0);
    Milliwatts::new(result).map_err(|_| Error::NumericalFailure)
}

pub fn milliwatts_to_dbm(value: Milliwatts) -> Result<Dbm, Error> {
    Dbm::new(10.0 * value.get().log10()).map_err(|_| Error::NumericalFailure)
}

/// SNR is defined only when noise was independently measured or explicitly
/// modeled by a provenance-bearing caller. Unknown noise remains unknown.
pub fn snr_db(signal: Dbm, noise: Evidence<Dbm>) -> Result<Evidence<Db>, Error> {
    ratio_db(signal, noise)
}

/// SIR consumes an already aggregated interference power. Use `sum_dbm` to
/// aggregate independently measured interferers in the linear domain.
pub fn sir_db(signal: Dbm, aggregate_interference: Evidence<Dbm>) -> Result<Evidence<Db>, Error> {
    ratio_db(signal, aggregate_interference)
}

/// SINR adds independently measured interference and noise as linear powers.
/// Either unknown input makes the result unknown with its original reason.
pub fn sinr_db(
    signal: Dbm,
    aggregate_interference: Evidence<Dbm>,
    noise: Evidence<Dbm>,
) -> Result<Evidence<Db>, Error> {
    let interference = match aggregate_interference {
        Evidence::Known(value) => value,
        Evidence::Unknown(reason) => return Ok(Evidence::Unknown(reason)),
    };
    let noise = match noise {
        Evidence::Known(value) => value,
        Evidence::Unknown(reason) => return Ok(Evidence::Unknown(reason)),
    };
    let denominator = sum_dbm(&[interference, noise])?;
    ratio_db(signal, denominator)
}

fn ratio_db(signal: Dbm, denominator: Evidence<Dbm>) -> Result<Evidence<Db>, Error> {
    match denominator {
        Evidence::Known(value) => Ok(Evidence::Known(
            Db::new(signal.get() - value.get()).map_err(|_| Error::NumericalFailure)?,
        )),
        Evidence::Unknown(reason) => Ok(Evidence::Unknown(reason)),
    }
}

/// Add powers in a scaled linear-milliwatt domain to avoid overflow.
pub fn sum_dbm(values: &[Dbm]) -> Result<Evidence<Dbm>, Error> {
    if values.is_empty() {
        return Ok(Evidence::Unknown(UnknownReason::NotMeasured));
    }
    if values.len() > MAX_SAMPLES {
        return Err(Error::ResourceLimit);
    }
    let mut sorted: Vec<_> = values.iter().map(|value| value.get()).collect();
    sorted.sort_by(f64::total_cmp);
    let max = *sorted.last().expect("nonempty values");
    let scaled_sum: f64 = sorted
        .iter()
        .map(|value| 10_f64.powf((value - max) / 10.0))
        .sum();
    let result = max + 10.0 * scaled_sum.log10();
    Ok(Evidence::Known(
        Dbm::new(result).map_err(|_| Error::NumericalFailure)?,
    ))
}

pub fn aggregate(
    samples: &[SignalSample],
    method: AggregateMethod,
) -> Result<SignalAggregate, Error> {
    validate_method(method)?;
    if samples.len() > MAX_SAMPLES {
        return Err(Error::ResourceLimit);
    }
    let mut ids = BTreeSet::new();
    for sample in samples {
        if !ids.insert(sample.observation_id) {
            return Err(Error::DuplicateObservation(sample.observation_id));
        }
    }
    let order_sensitive = matches!(
        method,
        AggregateMethod::EwmaDbm { .. } | AggregateMethod::RobustStateSpaceDbm { .. }
    );
    if order_sensitive {
        validate_sequence(samples)?;
    }
    let observation_order = if order_sensitive {
        samples.iter().map(|sample| sample.observation_id).collect()
    } else {
        ids.into_iter().collect()
    };
    if samples.is_empty() {
        let interval_reason = if matches!(method, AggregateMethod::PercentileRange { .. }) {
            UnknownReason::NotMeasured
        } else {
            UnknownReason::NotApplicable
        };
        return Ok(SignalAggregate {
            algorithm_version: ALGORITHM_VERSION,
            method,
            estimate: Evidence::Unknown(UnknownReason::NotMeasured),
            percentile_interval: Evidence::Unknown(interval_reason),
            sample_count: 0,
            observation_order,
        });
    }

    let values: Vec<f64> = samples.iter().map(|sample| sample.rssi.get()).collect();
    let (estimate, interval) = match method {
        AggregateMethod::MedianDbm => (median(&values), None),
        AggregateMethod::TrimmedMeanDbm { trim_each_tail } => {
            (trimmed_mean(&values, trim_each_tail.get())?, None)
        }
        AggregateMethod::LinearPowerMean => (linear_power_mean(&values)?, None),
        AggregateMethod::PercentileRange { lower, upper } => {
            let mut sorted = values.clone();
            sorted.sort_by(f64::total_cmp);
            let low = percentile(&sorted, lower.get());
            let high = percentile(&sorted, upper.get());
            (median_sorted(&sorted), Some((low, high)))
        }
        AggregateMethod::EwmaDbm { alpha } => {
            let mut state = values[0];
            for value in &values[1..] {
                state = convex_pair(state, *value, alpha.get());
            }
            (state, None)
        }
        AggregateMethod::RobustStateSpaceDbm {
            process_stddev,
            measurement_stddev,
            huber_threshold_stddevs,
        } => (
            robust_state_space(
                &values,
                process_stddev.get(),
                measurement_stddev.get(),
                huber_threshold_stddevs.get(),
            )?,
            None,
        ),
    };
    Ok(SignalAggregate {
        algorithm_version: ALGORITHM_VERSION,
        method,
        estimate: Evidence::Known(Dbm::new(estimate).map_err(|_| Error::NumericalFailure)?),
        percentile_interval: match interval {
            Some((lower, upper)) => Evidence::Known(PercentileInterval {
                lower: Dbm::new(lower).map_err(|_| Error::NumericalFailure)?,
                upper: Dbm::new(upper).map_err(|_| Error::NumericalFailure)?,
            }),
            None => Evidence::Unknown(UnknownReason::NotApplicable),
        },
        sample_count: samples.len() as u32,
        observation_order,
    })
}

fn validate_method(method: AggregateMethod) -> Result<(), Error> {
    match method {
        AggregateMethod::TrimmedMeanDbm { trim_each_tail } if trim_each_tail.get() >= 0.5 => {
            Err(Error::InvalidConfiguration("trim must be below 0.5"))
        }
        AggregateMethod::PercentileRange { lower, upper } if lower.get() >= upper.get() => {
            Err(Error::InvalidConfiguration("percentiles must be ordered"))
        }
        AggregateMethod::EwmaDbm { alpha } if alpha.get() == 0.0 => {
            Err(Error::InvalidConfiguration("EWMA alpha must be positive"))
        }
        AggregateMethod::RobustStateSpaceDbm {
            process_stddev,
            measurement_stddev,
            huber_threshold_stddevs,
        } if process_stddev.get() < 0.0
            || measurement_stddev.get() <= 0.0
            || huber_threshold_stddevs.get() <= 0.0 =>
        {
            Err(Error::InvalidConfiguration("invalid state-space scale"))
        }
        _ => Ok(()),
    }
}

fn validate_sequence(samples: &[SignalSample]) -> Result<(), Error> {
    for pair in samples.windows(2) {
        if pair[0].captured_at.epoch != pair[1].captured_at.epoch {
            return Err(Error::ClockEpochMismatch);
        }
        if pair[0].captured_at.nanoseconds >= pair[1].captured_at.nanoseconds {
            return Err(Error::NonMonotonicSequence);
        }
    }
    Ok(())
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    median_sorted(&sorted)
}

fn median_sorted(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        convex_pair(sorted[middle - 1], sorted[middle], 0.5)
    } else {
        sorted[middle]
    }
}

fn trimmed_mean(values: &[f64], fraction: f64) -> Result<f64, Error> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let trim = (sorted.len() as f64 * fraction).floor() as usize;
    let retained = sorted
        .get(trim..sorted.len().saturating_sub(trim))
        .ok_or(Error::InvalidConfiguration("trim removes all samples"))?;
    if retained.is_empty() {
        return Err(Error::InvalidConfiguration("trim removes all samples"));
    }
    let scale = retained
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return Ok(0.0);
    }
    let result =
        (retained.iter().map(|value| *value / scale).sum::<f64>() / retained.len() as f64) * scale;
    result
        .is_finite()
        .then_some(result)
        .ok_or(Error::NumericalFailure)
}

fn linear_power_mean(values: &[f64]) -> Result<f64, Error> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let max = *sorted.last().expect("nonempty values");
    let scaled_mean = sorted
        .iter()
        .map(|value| 10_f64.powf((value - max) / 10.0))
        .sum::<f64>()
        / values.len() as f64;
    let result = max + 10.0 * scaled_mean.log10();
    result
        .is_finite()
        .then_some(result)
        .ok_or(Error::NumericalFailure)
}

/// R-7 linear interpolation, evaluated in dBm space and explicitly labeled by
/// the aggregate method rather than reused as a linear-power statistic.
fn percentile(sorted: &[f64], probability: f64) -> f64 {
    let position = probability * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f64;
    convex_pair(sorted[lower], sorted[upper], fraction)
}

/// Scale before a convex combination so finite endpoints cannot overflow.
fn convex_pair(left: f64, right: f64, right_weight: f64) -> f64 {
    let scale = left.abs().max(right.abs());
    if scale == 0.0 {
        return 0.0;
    }
    let normalized =
        ((1.0 - right_weight) * (left / scale) + right_weight * (right / scale)).clamp(-1.0, 1.0);
    normalized * scale
}

fn robust_state_space(values: &[f64], q_std: f64, r_std: f64, huber: f64) -> Result<f64, Error> {
    let q = q_std * q_std;
    let r = r_std * r_std;
    if !q.is_finite() || !r.is_finite() || r == 0.0 {
        return Err(Error::NumericalFailure);
    }
    let mut estimate = values[0];
    let mut variance = r;
    for measurement in &values[1..] {
        let predicted_variance = variance + q;
        let innovation_variance = predicted_variance + r;
        let limit = huber * innovation_variance.sqrt();
        let innovation = (*measurement - estimate).clamp(-limit, limit);
        let gain = predicted_variance / innovation_variance;
        estimate += gain * innovation;
        variance = (1.0 - gain) * predicted_variance;
        if !estimate.is_finite() || !variance.is_finite() {
            return Err(Error::NumericalFailure);
        }
    }
    Ok(estimate)
}
