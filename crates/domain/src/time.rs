//! Monotonic clocks compare only within one source boot/capture epoch.
//! UTC values are signed nanoseconds since Unix epoch (POSIX, no leap seconds).
use crate::{
    ValidationError,
    evidence::Evidence,
    identity::{ClockEpochId, Text},
    units::*,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UtcTimestamp(pub i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonotonicTimestamp {
    pub epoch: ClockEpochId,
    pub nanoseconds: u64,
}
impl MonotonicTimestamp {
    pub fn elapsed_since(self, earlier: Self) -> Result<Seconds, ValidationError> {
        if self.epoch != earlier.epoch {
            return Err(ValidationError::ClockEpochMismatch);
        }
        let nanos = self
            .nanoseconds
            .checked_sub(earlier.nanoseconds)
            .ok_or(ValidationError::ReversedTime)?;
        Seconds::new(nanos as f64 / 1e9)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WallClockReading {
    pub time: UtcTimestamp,
    pub source: Text,
    pub precision: Seconds,
    pub uncertainty: Evidence<Seconds>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClockModel {
    pub epoch: ClockEpochId,
    pub reference_monotonic_nanoseconds: u64,
    pub reference_utc: UtcTimestamp,
    /// Add this offset to the source UTC to estimate reference UTC.
    pub offset_to_reference: Evidence<SignedSeconds>,
    pub drift: Evidence<PartsPerMillion>,
    pub error: Evidence<Seconds>,
    pub method_version: Text,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureTime {
    pub wall: Evidence<WallClockReading>,
    pub monotonic: Evidence<MonotonicTimestamp>,
    pub synchronization: Evidence<ClockModel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "WindowWire", into = "WindowWire")]
pub struct MonotonicWindow {
    start: MonotonicTimestamp,
    end: MonotonicTimestamp,
}
impl MonotonicWindow {
    pub fn new(
        start: MonotonicTimestamp,
        end: MonotonicTimestamp,
    ) -> Result<Self, ValidationError> {
        end.elapsed_since(start)?;
        Ok(Self { start, end })
    }
    pub const fn start(self) -> MonotonicTimestamp {
        self.start
    }
    pub const fn end(self) -> MonotonicTimestamp {
        self.end
    }
    pub fn contains(self, time: MonotonicTimestamp) -> bool {
        time.epoch == self.start.epoch
            && time.nanoseconds >= self.start.nanoseconds
            && time.nanoseconds <= self.end.nanoseconds
    }
}
#[derive(Serialize, Deserialize)]
struct WindowWire {
    start: MonotonicTimestamp,
    end: MonotonicTimestamp,
}
impl TryFrom<WindowWire> for MonotonicWindow {
    type Error = ValidationError;
    fn try_from(w: WindowWire) -> Result<Self, Self::Error> {
        Self::new(w.start, w.end)
    }
}
impl From<MonotonicWindow> for WindowWire {
    fn from(w: MonotonicWindow) -> Self {
        Self {
            start: w.start,
            end: w.end,
        }
    }
}
