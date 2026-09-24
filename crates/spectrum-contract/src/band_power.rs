//! Bounded integration of exact, caller-selected frequency bands.
//!
//! This derived display calculation sums power in the linear domain. It is
//! intentionally separate from the event-signature path: PSD event thresholds
//! still require equivalent-noise-bandwidth normalization, which is not
//! inferred from grid spacing here.

use crate::model::SpectrumBin;
use crate::{ProcessingLimits, SpectrumError, SpectrumSweep, model::PowerUnit};
use kyberia_domain::evidence::UnknownReason;

/// The rounding applied to the derived dBm display value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandPowerRounding {
    /// Round to the nearest 0.001 dBm; exact half-way cases round away from 0.
    NearestMillidBmTiesAwayFromZero,
}

/// Coverage of the caller-selected, whole-bin band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandPowerCoverage {
    /// Number of whole bins included in the selected half-open band.
    pub selected_bin_count: u32,
    /// Bins with an `Observed` state, including clipped observations.
    pub observed_bin_count: u32,
    /// Bins that are observed and unclipped, and therefore usable in an exact sum.
    pub exact_bin_count: u32,
    /// `observed_bin_count / selected_bin_count`, floored to parts per million.
    pub observed_coverage_parts_per_million: u32,
    /// `exact_bin_count / selected_bin_count`, floored to parts per million.
    pub exact_coverage_parts_per_million: u32,
}

/// A successfully integrated display value, derived from a validated sweep.
///
/// This is not canonical evidence and does not alter the source sweep or its
/// hash. `total_power_milli_dbm` is rounded only after summing selected-bin
/// powers in mW. For dBm/Hz input, each PSD density is multiplied by the
/// explicit frequency-grid bin width before summation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegratedBandPower {
    pub start_hz: u64,
    pub stop_hz_exclusive: u64,
    pub source_power_unit: PowerUnit,
    pub bin_width_hz: u64,
    pub coverage: BandPowerCoverage,
    pub total_power_milli_dbm: i32,
    pub rounding: BandPowerRounding,
}

/// Why a selected bin prevents an exact integrated total.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BandPowerUnknownReason {
    BelowDetectionThreshold,
    Clipped,
    NotObserved { reason: UnknownReason },
}

/// One blocking bin, indexed in the complete source sweep grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BandPowerUnknownBin {
    pub bin_index: u32,
    pub reason: BandPowerUnknownReason,
}

/// Fail-closed integration result with deterministic per-bin explanations.
///
/// `blocking_bins` is ordered by increasing source-grid index. It carries the
/// original reason for each not-observed bin without treating any unknown or
/// clipped value as zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownBandPower {
    pub start_hz: u64,
    pub stop_hz_exclusive: u64,
    pub source_power_unit: PowerUnit,
    pub bin_width_hz: u64,
    pub coverage: BandPowerCoverage,
    pub blocking_bins: Vec<BandPowerUnknownBin>,
}

/// Result of a pure band integration over one validated sweep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BandPowerOutcome {
    Exact(IntegratedBandPower),
    Unknown(UnknownBandPower),
}

impl SpectrumSweep {
    /// Integrate an exact whole-bin, half-open frequency range `[start, stop)`.
    ///
    /// Range endpoints must lie on the sweep grid and remain within it. Every
    /// selected bin must be `Observed { clipped: false, .. }` for an exact
    /// result. The caller-supplied limits bound both selected bins and work;
    /// they are rechecked even if the sweep was constructed under looser
    /// limits. No calibration terms, noise floor, or fractional-bin estimates
    /// are applied.
    pub fn integrate_band(
        &self,
        start_hz: u64,
        stop_hz_exclusive: u64,
        limits: ProcessingLimits,
    ) -> Result<BandPowerOutcome, SpectrumError> {
        integrate_band(self, start_hz, stop_hz_exclusive, limits)
    }
}

/// Integrate an exact whole-bin, half-open frequency band from one sweep.
pub fn integrate_band(
    sweep: &SpectrumSweep,
    start_hz: u64,
    stop_hz_exclusive: u64,
    limits: ProcessingLimits,
) -> Result<BandPowerOutcome, SpectrumError> {
    limits.validate()?;
    let document = sweep.document();
    let grid = document.grid;
    grid.validate(limits)?;

    if start_hz >= stop_hz_exclusive {
        return Err(SpectrumError::Invalid("empty spectrum band"));
    }
    if start_hz < grid.start_hz || stop_hz_exclusive > grid.stop_hz_exclusive {
        return Err(SpectrumError::Invalid("spectrum band outside sweep grid"));
    }

    let start_offset = start_hz
        .checked_sub(grid.start_hz)
        .ok_or(SpectrumError::Invalid("spectrum band start"))?;
    let stop_offset = stop_hz_exclusive
        .checked_sub(grid.start_hz)
        .ok_or(SpectrumError::Invalid("spectrum band stop"))?;
    if !start_offset.is_multiple_of(grid.bin_width_hz)
        || !stop_offset.is_multiple_of(grid.bin_width_hz)
    {
        return Err(SpectrumError::Invalid("spectrum band bin alignment"));
    }

    let start_index = start_offset
        .checked_div(grid.bin_width_hz)
        .ok_or(SpectrumError::Invalid("spectrum band start index"))?;
    let stop_index = stop_offset
        .checked_div(grid.bin_width_hz)
        .ok_or(SpectrumError::Invalid("spectrum band stop index"))?;
    let selected_bins_u64 = stop_index
        .checked_sub(start_index)
        .ok_or(SpectrumError::Invalid("spectrum band bin count"))?;
    let selected_bin_count = u32::try_from(selected_bins_u64)
        .map_err(|_| SpectrumError::ResourceLimit("spectrum band bin count"))?;
    let work_units = usize::try_from(selected_bins_u64)
        .map_err(|_| SpectrumError::ResourceLimit("spectrum band work"))?;
    if work_units > limits.max_bins_per_sweep || work_units > limits.max_work_units {
        return Err(SpectrumError::ResourceLimit("spectrum band work"));
    }

    let start_index = usize::try_from(start_index)
        .map_err(|_| SpectrumError::ResourceLimit("spectrum band start index"))?;
    let stop_index = usize::try_from(stop_index)
        .map_err(|_| SpectrumError::ResourceLimit("spectrum band stop index"))?;
    if stop_index > document.bins.len() || start_index >= stop_index {
        return Err(SpectrumError::Invalid("spectrum band bin indices"));
    }

    let mut observed_bin_count = 0_u32;
    let mut exact_bin_count = 0_u32;
    let mut total_milliwatts = 0.0_f64;
    let mut blocking_bins = Vec::new();

    for bin_index in start_index..stop_index {
        match &document.bins[bin_index] {
            SpectrumBin::Observed {
                power_milli_dbm,
                clipped: false,
            } => {
                observed_bin_count = observed_bin_count
                    .checked_add(1)
                    .ok_or(SpectrumError::ResourceLimit("spectrum band coverage"))?;
                exact_bin_count = exact_bin_count
                    .checked_add(1)
                    .ok_or(SpectrumError::ResourceLimit("spectrum band coverage"))?;
                let bin_milliwatts = dbm_to_milliwatts(*power_milli_dbm);
                let contribution = match document.power_unit {
                    PowerUnit::DbmPerBin => bin_milliwatts,
                    PowerUnit::DbmPerHertz => bin_milliwatts * grid.bin_width_hz as f64,
                };
                total_milliwatts += contribution;
            }
            SpectrumBin::Observed { clipped: true, .. } => {
                observed_bin_count = observed_bin_count
                    .checked_add(1)
                    .ok_or(SpectrumError::ResourceLimit("spectrum band coverage"))?;
                blocking_bins.push(BandPowerUnknownBin {
                    bin_index: u32::try_from(bin_index)
                        .map_err(|_| SpectrumError::ResourceLimit("spectrum band bin index"))?,
                    reason: BandPowerUnknownReason::Clipped,
                });
            }
            SpectrumBin::BelowDetectionThreshold { .. } => {
                blocking_bins.push(BandPowerUnknownBin {
                    bin_index: u32::try_from(bin_index)
                        .map_err(|_| SpectrumError::ResourceLimit("spectrum band bin index"))?,
                    reason: BandPowerUnknownReason::BelowDetectionThreshold,
                });
            }
            SpectrumBin::NotObserved { reason } => {
                blocking_bins.push(BandPowerUnknownBin {
                    bin_index: u32::try_from(bin_index)
                        .map_err(|_| SpectrumError::ResourceLimit("spectrum band bin index"))?,
                    reason: BandPowerUnknownReason::NotObserved {
                        reason: reason.clone(),
                    },
                });
            }
        }
    }

    let coverage = BandPowerCoverage {
        selected_bin_count,
        observed_bin_count,
        exact_bin_count,
        observed_coverage_parts_per_million: coverage_parts_per_million(
            observed_bin_count,
            selected_bin_count,
        ),
        exact_coverage_parts_per_million: coverage_parts_per_million(
            exact_bin_count,
            selected_bin_count,
        ),
    };
    if !blocking_bins.is_empty() {
        return Ok(BandPowerOutcome::Unknown(UnknownBandPower {
            start_hz,
            stop_hz_exclusive,
            source_power_unit: document.power_unit,
            bin_width_hz: grid.bin_width_hz,
            coverage,
            blocking_bins,
        }));
    }

    let total_power_milli_dbm = milliwatts_to_rounded_milli_dbm(total_milliwatts)?;
    Ok(BandPowerOutcome::Exact(IntegratedBandPower {
        start_hz,
        stop_hz_exclusive,
        source_power_unit: document.power_unit,
        bin_width_hz: grid.bin_width_hz,
        coverage,
        total_power_milli_dbm,
        rounding: BandPowerRounding::NearestMillidBmTiesAwayFromZero,
    }))
}

fn dbm_to_milliwatts(power_milli_dbm: i32) -> f64 {
    10.0_f64.powf(f64::from(power_milli_dbm) / 10_000.0)
}

fn milliwatts_to_rounded_milli_dbm(milliwatts: f64) -> Result<i32, SpectrumError> {
    if !milliwatts.is_finite() || milliwatts <= 0.0 {
        return Err(SpectrumError::Invalid("integrated linear power"));
    }
    round_milli_dbm_ties_away_from_zero(10_000.0 * milliwatts.log10())
}

/// Quantize an already-converted milli-dBm value. Kept separate so exact
/// half-way cases can be tested without relying on `log10` to produce an exact
/// binary floating-point tie.
fn round_milli_dbm_ties_away_from_zero(milli_dbm: f64) -> Result<i32, SpectrumError> {
    let rounded = milli_dbm.round();
    if !rounded.is_finite() || rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        return Err(SpectrumError::ResourceLimit("integrated display power"));
    }
    Ok(rounded as i32)
}

fn coverage_parts_per_million(observed: u32, selected: u32) -> u32 {
    debug_assert!(selected > 0 && observed <= selected);
    ((u64::from(observed) * 1_000_000) / u64::from(selected)) as u32
}

#[cfg(test)]
mod tests {
    use super::round_milli_dbm_ties_away_from_zero;

    #[test]
    fn final_millidbm_quantization_rounds_half_ties_away_from_zero() {
        let half = 0.5_f64;
        let below_half = f64::from_bits(half.to_bits() - 1);
        let above_half = f64::from_bits(half.to_bits() + 1);

        assert_eq!(round_milli_dbm_ties_away_from_zero(below_half), Ok(0));
        assert_eq!(round_milli_dbm_ties_away_from_zero(half), Ok(1));
        assert_eq!(round_milli_dbm_ties_away_from_zero(above_half), Ok(1));

        assert_eq!(round_milli_dbm_ties_away_from_zero(-below_half), Ok(0));
        assert_eq!(round_milli_dbm_ties_away_from_zero(-half), Ok(-1));
        assert_eq!(round_milli_dbm_ties_away_from_zero(-above_half), Ok(-1));
    }
}
