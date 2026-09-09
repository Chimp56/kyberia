//! Rounded planar offsets through the adopted geometry kernel.
use crate::{BooleanError, ValidatedMultiPolygon};
use kyberia_domain::units::{Meters, Radians};

/// Conservative pairwise work admission over expanded rounded vertices.
pub const MAX_OFFSET_WORK: usize = 16_777_216;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffsetDirection {
    Outward(Meters),
    Inward(Meters),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetOptions {
    direction: OffsetDirection,
    max_arc_angle: Radians,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OffsetError {
    InvalidOptions,
    ResourceLimit,
    UnsupportedCoordinateResolution,
    Cancelled,
    KernelResult(BooleanError),
}
impl std::fmt::Display for OffsetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "planar offset rejected: {self:?}")
    }
}
impl std::error::Error for OffsetError {}

impl OffsetOptions {
    /// Angular tessellation is explicit: supported increments are 0.025 to
    /// pi/2 radians. This controls arc approximation, not a metric tolerance
    /// or a guarantee about the kernel's coordinate quantization.
    pub fn new(direction: OffsetDirection, max_arc_angle: Radians) -> Result<Self, OffsetError> {
        let distance = match direction {
            OffsetDirection::Outward(distance) | OffsetDirection::Inward(distance) => {
                distance.get()
            }
        };
        if distance > crate::MAX_ABSOLUTE_COORDINATE_METERS
            || !(0.025..=std::f64::consts::FRAC_PI_2).contains(&max_arc_angle.get())
        {
            return Err(OffsetError::InvalidOptions);
        }
        Ok(Self {
            direction,
            max_arc_angle,
        })
    }

    pub fn direction(self) -> OffsetDirection {
        self.direction
    }
    pub fn max_arc_angle(self) -> Radians {
        self.max_arc_angle
    }
    pub(crate) fn signed_distance(self) -> f64 {
        match self.direction {
            OffsetDirection::Outward(distance) => distance.get(),
            OffsetDirection::Inward(distance) => -distance.get(),
        }
    }
}

impl ValidatedMultiPolygon {
    /// Positive offsets grow exterior boundaries and close holes; negative
    /// offsets shrink interiors and may split or completely erase geometry.
    /// The synchronous third-party kernel cannot poll cancellation internally;
    /// admission bounds its input and polling occurs around the kernel and
    /// before the canonical result is returned.
    pub fn offset_with_cancellation(
        &self,
        options: OffsetOptions,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, OffsetError> {
        crate::polygon::offset(self, options, &mut cancelled)
    }

    pub fn offset(&self, options: OffsetOptions) -> Result<Self, OffsetError> {
        self.offset_with_cancellation(options, || false)
    }
}
