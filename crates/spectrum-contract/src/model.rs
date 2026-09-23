use crate::{ProcessingLimits, SpectrumError, sha256};
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{CalibrationId, ContentHash, SensorId, SessionId, SourceId, Text},
    spatial::PoseReference,
    time::{CaptureTime, MonotonicWindow, UtcTimestamp},
};
use serde::{Deserialize, Serialize};

pub const SWEEP_SCHEMA_V1: &str = "kyberia.spectrum-sweep/1";
pub const CALIBRATION_SCHEMA_V1: &str = "kyberia.spectrum-calibration/1";
const MAX_FREQUENCY_HZ: u64 = 1_000_000_000_000;
const MIN_POWER_MILLI_DBM: i32 = -500_000;
const MAX_POWER_MILLI_DBM: i32 = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpectrumSourceKind {
    VendorAnalyzer,
    SoapySdr,
    ImportedTrace,
    RemoteSpectrumSensor,
}

/// Spectrum sources are kept separate from Wi-Fi NIC observations. In
/// particular, this type has no BSSID, channel-utilization, or noise fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpectrumSourceMetadata {
    pub kind: SpectrumSourceKind,
    pub source_id: SourceId,
    pub sensor_id: Evidence<SensorId>,
    pub adapter_name: Text,
    pub adapter_version: Text,
    pub device_model: Evidence<Text>,
    pub device_firmware: Evidence<Text>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrequencyGrid {
    pub start_hz: u64,
    /// Exclusive upper edge; bins cover `[start_hz, stop_hz_exclusive)`.
    pub stop_hz_exclusive: u64,
    pub bin_width_hz: u64,
    pub resolution_bandwidth_hz: u64,
    pub bin_count: u32,
}

impl FrequencyGrid {
    pub(crate) fn validate(self, limits: ProcessingLimits) -> Result<(), SpectrumError> {
        if self.start_hz == 0
            || self.stop_hz_exclusive > MAX_FREQUENCY_HZ
            || self.stop_hz_exclusive <= self.start_hz
            || self.bin_width_hz == 0
            || self.resolution_bandwidth_hz == 0
        {
            return Err(SpectrumError::Invalid("frequency grid"));
        }
        let span = self.stop_hz_exclusive - self.start_hz;
        if !span.is_multiple_of(self.bin_width_hz)
            || span / self.bin_width_hz != u64::from(self.bin_count)
            || self.bin_count == 0
            || usize::try_from(self.bin_count).unwrap_or(usize::MAX) > limits.max_bins_per_sweep
        {
            return Err(SpectrumError::Invalid("frequency bin geometry"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorKind {
    Sample,
    Average,
    Peak,
    Rms,
    DeviceDefined,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowFunction {
    Rectangular,
    Hann,
    Hamming,
    Blackman,
    DeviceDefined,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GainSetting {
    Automatic,
    Manual { gain_milli_db: i32 },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerUnit {
    /// Logarithmic power integrated across one bin.
    DbmPerBin,
    /// Power spectral density. This must not be confused with dBm per bin.
    DbmPerHertz,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionSettings {
    pub detector: DetectorKind,
    pub window: WindowFunction,
    pub gain: GainSetting,
    pub dwell_nanoseconds: u64,
    pub sweep_nanoseconds: u64,
}

impl AcquisitionSettings {
    fn validate(self) -> Result<(), SpectrumError> {
        if self.dwell_nanoseconds == 0
            || self.sweep_nanoseconds == 0
            || self.dwell_nanoseconds > self.sweep_nanoseconds
        {
            return Err(SpectrumError::Invalid("acquisition timing"));
        }
        Ok(())
    }
}

/// Calibration profile evidence carried by a sweep. Bin readings remain the
/// values reported by the adapter; this crate never silently reapplies terms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpectrumCalibrationProfile {
    pub schema: String,
    pub calibration_id: CalibrationId,
    pub version: Text,
    pub profile_sha256: ContentHash,
    pub valid_start_hz: u64,
    pub valid_stop_hz_exclusive: u64,
    pub valid_from_utc: UtcTimestamp,
    pub valid_until_utc: UtcTimestamp,
    /// Net correction terms and uncertainty are recorded in milli-dB. Their
    /// sign convention belongs to the named adapter/profile version.
    pub reference_correction_milli_db: i32,
    pub antenna_factor_milli_db: i32,
    pub cable_loss_milli_db: i32,
    pub frequency_offset_correction_hz: i64,
    pub uncertainty_milli_db: u32,
    pub clipping_checked: Evidence<bool>,
    pub dynamic_range_checked: Evidence<bool>,
}

impl SpectrumCalibrationProfile {
    fn validate_for(&self, grid: FrequencyGrid, time: &CaptureTime) -> Result<(), SpectrumError> {
        if self.schema != CALIBRATION_SCHEMA_V1 {
            return Err(SpectrumError::UnsupportedSchema);
        }
        if self.valid_start_hz == 0
            || self.valid_stop_hz_exclusive <= self.valid_start_hz
            || self.valid_from_utc.0 >= self.valid_until_utc.0
            || grid.start_hz < self.valid_start_hz
            || grid.stop_hz_exclusive > self.valid_stop_hz_exclusive
            || self.reference_correction_milli_db.unsigned_abs() > 200_000
            || self.antenna_factor_milli_db.unsigned_abs() > 200_000
            || self.cable_loss_milli_db.unsigned_abs() > 200_000
            || self.frequency_offset_correction_hz.unsigned_abs() > 1_000_000_000
            || self.uncertainty_milli_db > 200_000
        {
            return Err(SpectrumError::Invalid("calibration profile"));
        }
        let Evidence::Known(reading) = &time.wall else {
            return Err(SpectrumError::Invalid(
                "calibration requires wall-clock evidence",
            ));
        };
        if reading.time.0 < self.valid_from_utc.0 || reading.time.0 >= self.valid_until_utc.0 {
            return Err(SpectrumError::Invalid("calibration validity interval"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpectrumBin {
    Observed { power_milli_dbm: i32, clipped: bool },
    BelowDetectionThreshold { threshold_milli_dbm: i32 },
    NotObserved { reason: UnknownReason },
}

impl SpectrumBin {
    fn validate(&self) -> Result<(), SpectrumError> {
        let power = match self {
            Self::Observed {
                power_milli_dbm, ..
            } => *power_milli_dbm,
            Self::BelowDetectionThreshold {
                threshold_milli_dbm,
            } => *threshold_milli_dbm,
            Self::NotObserved { .. } => return Ok(()),
        };
        if !(MIN_POWER_MILLI_DBM..=MAX_POWER_MILLI_DBM).contains(&power) {
            return Err(SpectrumError::Invalid("spectrum power range"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpectrumSweepDocument {
    pub schema: String,
    pub session_id: SessionId,
    pub sequence: u64,
    pub source: SpectrumSourceMetadata,
    pub grid: FrequencyGrid,
    pub acquisition: AcquisitionSettings,
    pub power_unit: PowerUnit,
    pub time: CaptureTime,
    pub capture_window: Evidence<MonotonicWindow>,
    pub pose: Evidence<PoseReference>,
    pub calibration: Evidence<SpectrumCalibrationProfile>,
    pub bins: Vec<SpectrumBin>,
}

impl SpectrumSweepDocument {
    fn validate(&self, limits: ProcessingLimits) -> Result<(), SpectrumError> {
        limits.validate()?;
        if self.schema != SWEEP_SCHEMA_V1 {
            return Err(SpectrumError::UnsupportedSchema);
        }
        self.grid.validate(limits)?;
        self.acquisition.validate()?;
        if self.bins.len() != self.grid.bin_count as usize {
            return Err(SpectrumError::Invalid("sweep bin count"));
        }
        if let Evidence::Known(window) = &self.capture_window {
            let start = window.start();
            let end = window.end();
            if start.epoch != end.epoch || end.nanoseconds <= start.nanoseconds {
                return Err(SpectrumError::Invalid("monotonic capture window"));
            }
            if end.nanoseconds - start.nanoseconds != self.acquisition.sweep_nanoseconds {
                return Err(SpectrumError::Invalid("capture duration mismatch"));
            }
            if let Some(captured) = self.time.monotonic.as_known()
                && (captured.epoch != start.epoch || !window.contains(*captured))
            {
                return Err(SpectrumError::Invalid("capture time outside sweep window"));
            }
        }
        if let Evidence::Known(profile) = &self.calibration {
            profile.validate_for(self.grid, &self.time)?;
        }
        for bin in &self.bins {
            bin.validate()?;
        }
        Ok(())
    }
}

/// Validated sweep. Construction and decoding preserve exact canonical bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct SpectrumSweep {
    document: SpectrumSweepDocument,
    canonical_bytes: Vec<u8>,
    sha256: ContentHash,
}

impl SpectrumSweep {
    pub fn new(
        document: SpectrumSweepDocument,
        limits: ProcessingLimits,
    ) -> Result<Self, SpectrumError> {
        document.validate(limits)?;
        let canonical_bytes =
            serde_json::to_vec(&document).map_err(|_| SpectrumError::Serialization)?;
        if canonical_bytes.len() > limits.max_canonical_bytes {
            return Err(SpectrumError::ResourceLimit("canonical sweep bytes"));
        }
        let sha256 = ContentHash::from_sha256(sha256(&canonical_bytes));
        Ok(Self {
            document,
            canonical_bytes,
            sha256,
        })
    }

    pub fn from_canonical_bytes(
        bytes: &[u8],
        limits: ProcessingLimits,
    ) -> Result<Self, SpectrumError> {
        limits.validate()?;
        if bytes.is_empty() || bytes.len() > limits.max_canonical_bytes {
            return Err(SpectrumError::ResourceLimit("canonical sweep bytes"));
        }
        let document: SpectrumSweepDocument =
            serde_json::from_slice(bytes).map_err(|_| SpectrumError::Serialization)?;
        let sweep = Self::new(document, limits)?;
        if sweep.canonical_bytes != bytes {
            return Err(SpectrumError::NonCanonical);
        }
        Ok(sweep)
    }

    pub const fn document(&self) -> &SpectrumSweepDocument {
        &self.document
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub const fn sha256(&self) -> ContentHash {
        self.sha256
    }
}
