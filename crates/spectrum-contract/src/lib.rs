//! Versioned contracts for hardware-independent spectrum evidence.
//!
//! The module records source, sweep, calibration, position, and time evidence.
//! It does not acquire or calibrate radio data, and its pattern rules never
//! identify an emitter or protocol.

mod model;
mod signature;

pub use model::*;
pub use signature::*;

use sha2::{Digest, Sha256};

/// Hard limits are explicit inputs so imported evidence cannot choose its own
/// memory or compute budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessingLimits {
    pub max_bins_per_sweep: usize,
    pub max_sweeps_per_event: usize,
    pub max_work_units: usize,
    pub max_canonical_bytes: usize,
}

impl ProcessingLimits {
    pub const fn conservative() -> Self {
        Self {
            max_bins_per_sweep: 16_384,
            max_sweeps_per_event: 512,
            max_work_units: 2_000_000,
            max_canonical_bytes: 16 * 1024 * 1024,
        }
    }

    pub(crate) fn validate(self) -> Result<(), SpectrumError> {
        if self.max_bins_per_sweep == 0
            || self.max_sweeps_per_event == 0
            || self.max_work_units == 0
            || self.max_canonical_bytes == 0
        {
            return Err(SpectrumError::Invalid("processing limits"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpectrumError {
    Invalid(&'static str),
    UnsupportedSchema,
    NonCanonical,
    EvidenceMismatch,
    ResourceLimit(&'static str),
    Serialization,
}

impl std::fmt::Display for SpectrumError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SpectrumError {}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
