//! Canonical evidence contracts. No clocks, persistence, OS APIs or foreign schemas.
//!
//! Physical quantities and identities cannot be interchanged accidentally:
//! ```compile_fail
//! use kyberia_domain::units::{Db, Dbm};
//! let power: Dbm = Db::new(3.0).unwrap();
//! ```
//! ```compile_fail
//! use kyberia_domain::units::{Dbm, Milliwatts};
//! let linear_power: Milliwatts = Dbm::new(-60.0).unwrap();
//! ```
//! ```compile_fail
//! use kyberia_domain::units::{Meters, Pixels};
//! let distance: Meters = Pixels::new(10.0).unwrap();
//! ```
//! ```compile_fail
//! use kyberia_domain::identity::{SessionId, SourceId};
//! fn store_session(_: SessionId) {}
//! store_session(SourceId::from_bytes([1;16]).unwrap());
//! ```
//! ```compile_fail
//! use kyberia_domain::time::{UtcTimestamp, MonotonicTimestamp};
//! fn correlate(_: UtcTimestamp) {}
//! fn invalid(t: MonotonicTimestamp) { correlate(t); }
//! ```
//! ```compile_fail
//! use kyberia_domain::units::{Percentage, Probability};
//! let probability: Probability = Percentage::new(50.0).unwrap();
//! ```
//! ```compile_fail
//! use kyberia_domain::units::{Hertz, Megahertz};
//! let frequency: Hertz = Megahertz::new(2412.0).unwrap();
//! ```
//! ```compile_fail
//! use kyberia_domain::units::{Eirp, ConductedPower, Dbm};
//! let power: Eirp = ConductedPower(Dbm::new(20.0).unwrap());
//! ```
pub mod analysis;
pub mod capability;
pub mod capture;
pub mod evidence;
pub mod identity;
pub mod observation;
pub mod project;
pub mod spatial;
pub mod time;
pub mod units;

use std::fmt;

/// A stable validation failure; no foreign parser or OS error leaks inward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    NonFinite(&'static str),
    OutOfRange(&'static str),
    InvalidIdentity,
    InvalidText,
    InvalidCovariance,
    ClockEpochMismatch,
    ReversedTime,
    UnsupportedSchema,
    ResourceLimit(&'static str),
    Inconsistent(&'static str),
}
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ValidationError {}
