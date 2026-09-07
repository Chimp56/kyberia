//! Pure point-survey admission. Raw observation storage and platform actuation
//! remain outside this crate; progress comes only from admitted evidence.
mod config;
mod state;
pub use config::*;
use kyberia_domain::{evidence::*, identity::*, observation::*, spatial::*, time::*, units::*};
use serde::{Deserialize, Serialize};
pub use state::*;
use std::collections::{BTreeMap, BTreeSet};
const MAX_RECORDS: usize = 4096;
const MAX_WINDOWS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurveyError {
    InvalidConfiguration,
    UnsupportedCapabilities(Vec<kyberia_domain::capability::Capability>),
    InvalidTransition,
    WrongClock,
    ReversedTime,
    WrongSession,
    WrongSource,
    WrongFrame,
    PoseUnavailable,
    PoseOutsidePoint,
    PoseUncertain,
    TimestampUnavailable,
    StaleScan,
    InconsistentScanAge,
    CaptureOutsidePoint,
    FutureCapture,
    FutureDwell,
    DuplicateObservation,
    DuplicateSourceSample,
    UnsupportedPayload,
    UnusableQuality,
    NotReady,
    InvalidSnapshot,
    Limit,
}
impl std::fmt::Display for SurveyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SurveyError {}
fn nanos(seconds: Seconds) -> Result<u64, SurveyError> {
    let n = (seconds.get() * 1e9).ceil();
    if !n.is_finite() || n >= u64::MAX as f64 {
        return Err(SurveyError::InvalidConfiguration);
    }
    Ok(n as u64)
}
fn seconds(nanos: u64) -> Seconds {
    Seconds::new(nanos as f64 / 1e9).expect("u64 nanoseconds always finite")
}
