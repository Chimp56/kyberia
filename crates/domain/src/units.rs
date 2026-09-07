//! Units serialize as a number in the named field's documented unit.
//! Constructors and deserializers share validation; arithmetic is checked.
use crate::ValidationError;
use serde::{Deserialize, Serialize};

macro_rules! quantity {
    ($name:ident, $valid:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "f64", into = "f64")]
        pub struct $name(f64);
        impl $name {
            pub fn new(value: f64) -> Result<Self, ValidationError> {
                if !value.is_finite() {
                    return Err(ValidationError::NonFinite(stringify!($name)));
                }
                if !($valid)(value) {
                    return Err(ValidationError::OutOfRange(stringify!($name)));
                }
                Ok(Self(if value == 0.0 { 0.0 } else { value }))
            }
            pub const fn get(self) -> f64 {
                self.0
            }
        }
        impl TryFrom<f64> for $name {
            type Error = ValidationError;
            fn try_from(value: f64) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
        impl From<$name> for f64 {
            fn from(value: $name) -> f64 {
                value.0
            }
        }
    };
}
quantity!(
    Dbm,
    |_: f64| true,
    "Logarithmic power referenced to one milliwatt; finite, signed."
);
quantity!(
    Db,
    |_: f64| true,
    "Logarithmic ratio, gain or loss; finite, signed."
);
quantity!(
    Hertz,
    |v: f64| v > 0.0,
    "Strictly positive frequency in Hz."
);
quantity!(
    Megahertz,
    |v: f64| v > 0.0,
    "Strictly positive frequency/bandwidth in MHz."
);
quantity!(
    Gigahertz,
    |v: f64| v > 0.0,
    "Strictly positive frequency in GHz."
);
quantity!(
    Meters,
    |v: f64| v >= 0.0,
    "Nonnegative length in meters; coordinates use CoordinateMeters."
);
quantity!(
    CoordinateMeters,
    |_: f64| true,
    "Signed Cartesian component in meters."
);
quantity!(
    MetersPerPixel,
    |v: f64| v > 0.0,
    "Positive scale mapping image pixels to physical meters."
);
quantity!(
    Pixels,
    |_: f64| true,
    "Signed image pixel coordinate; not a physical length."
);
quantity!(
    Seconds,
    |v: f64| v >= 0.0,
    "Nonnegative duration in seconds."
);
quantity!(
    Milliseconds,
    |v: f64| v >= 0.0,
    "Nonnegative duration in milliseconds."
);
quantity!(
    SignedSeconds,
    |_: f64| true,
    "Signed clock offset in seconds; not a duration."
);
quantity!(
    Mbps,
    |v: f64| v >= 0.0,
    "Nonnegative megabits per second (decimal millions)."
);
quantity!(
    Probability,
    |v: f64| (0.0..=1.0).contains(&v),
    "Unit interval probability."
);
quantity!(
    Percentage,
    |v: f64| (0.0..=100.0).contains(&v),
    "Percentage in [0,100], distinct from probability."
);
quantity!(
    Radians,
    |_: f64| true,
    "Right-handed rotation in radians, finite, not implicitly normalized."
);
quantity!(
    Degrees,
    |_: f64| true,
    "Right-handed rotation in degrees, finite, not implicitly normalized."
);
quantity!(
    PartsPerMillion,
    |_: f64| true,
    "Signed clock drift in parts per million."
);

macro_rules! conversion {
    ($from:ident, $to:ident, $factor:expr) => {
        impl TryFrom<$from> for $to {
            type Error = ValidationError;
            fn try_from(value: $from) -> Result<Self, Self::Error> {
                Self::new(value.get() * $factor)
            }
        }
    };
}
conversion!(Hertz, Megahertz, 1e-6);
conversion!(Megahertz, Hertz, 1e6);
conversion!(Gigahertz, Hertz, 1e9);
conversion!(Hertz, Gigahertz, 1e-9);
conversion!(Seconds, Milliseconds, 1000.0);
conversion!(Milliseconds, Seconds, 0.001);
conversion!(Probability, Percentage, 100.0);
conversion!(Percentage, Probability, 0.01);
conversion!(Degrees, Radians, std::f64::consts::PI / 180.0);
conversion!(Radians, Degrees, 180.0 / std::f64::consts::PI);

impl Dbm {
    /// A power difference is a dB ratio, never another dBm power.
    pub fn difference(self, other: Self) -> Result<Db, ValidationError> {
        Db::new(self.0 - other.0)
    }
    pub fn apply_gain(self, gain: Db) -> Result<Self, ValidationError> {
        Self::new(self.0 + gain.get())
    }
}

/// These wrappers prevent EIRP, conducted power and PSD from sharing an API slot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Eirp(pub Dbm);
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConductedPower(pub Dbm);
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PowerSpectralDensityDbmPerMhz(pub Dbm);
