//! Explicit, invertible two-point image calibration. The complete controls,
//! handedness and uncertainty are retained; exact interpolation is not accuracy.
use super::PixelPoint;
use crate::{ValidationError, evidence::Evidence, identity::FrameId, units::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point2 {
    pub x: CoordinateMeters,
    pub y: CoordinateMeters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageYAxis {
    Down,
    Up,
}
impl ImageYAxis {
    fn sign(self) -> f64 {
        if self == Self::Down { -1.0 } else { 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationControls {
    pub source_frame: FrameId,
    pub target_frame: FrameId,
    pub image_first: PixelPoint,
    pub image_second: PixelPoint,
    pub target_origin: Point2,
    pub known_distance: Meters,
    /// Direction of the first→second control segment in the target frame.
    pub target_direction: Radians,
    pub image_y_axis: ImageYAxis,
    pub distance_uncertainty: Evidence<Meters>,
    /// Nonnegative image control-point uncertainty in pixels.
    pub control_point_uncertainty: Evidence<Pixels>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CalibrationControls", into = "CalibrationControls")]
pub struct TwoPointCalibration {
    controls: CalibrationControls,
    scale: MetersPerPixel,
    cosine: f64,
    sine: f64,
}
impl TwoPointCalibration {
    pub fn new(controls: CalibrationControls) -> Result<Self, ValidationError> {
        if controls.source_frame == controls.target_frame {
            return Err(ValidationError::Inconsistent("calibration frame loop"));
        }
        if controls.known_distance.get() == 0.0 {
            return Err(ValidationError::OutOfRange("known calibration distance"));
        }
        if matches!(controls.control_point_uncertainty,Evidence::Known(p) if p.get()<0.0) {
            return Err(ValidationError::OutOfRange("pixel uncertainty"));
        }
        let dx = controls.image_second.x.get() - controls.image_first.x.get();
        let dy = (controls.image_second.y.get() - controls.image_first.y.get())
            * controls.image_y_axis.sign();
        let length = dx.hypot(dy);
        if !length.is_finite() || length == 0.0 {
            return Err(ValidationError::OutOfRange(
                "degenerate calibration controls",
            ));
        }
        let scale = MetersPerPixel::new(controls.known_distance.get() / length)?;
        // Compose the target and inverse control rotations directly. Subtracting
        // atan2 from a very large finite angle can discard the control rotation.
        let (target_sine, target_cosine) = controls.target_direction.get().sin_cos();
        let control_cosine = dx / length;
        let control_sine = dy / length;
        let cosine = target_cosine * control_cosine + target_sine * control_sine;
        let sine = target_sine * control_cosine - target_cosine * control_sine;
        Ok(Self {
            controls,
            scale,
            cosine,
            sine,
        })
    }
    pub const fn controls(&self) -> &CalibrationControls {
        &self.controls
    }
    pub const fn scale(&self) -> MetersPerPixel {
        self.scale
    }
    pub fn to_floor(
        &self,
        source_frame: FrameId,
        pixel: PixelPoint,
    ) -> Result<Point2, ValidationError> {
        if source_frame != self.controls.source_frame {
            return Err(ValidationError::Inconsistent("image frame mismatch"));
        }
        let x = pixel.x.get() - self.controls.image_first.x.get();
        let y =
            (pixel.y.get() - self.controls.image_first.y.get()) * self.controls.image_y_axis.sign();
        Ok(Point2 {
            x: CoordinateMeters::new(
                self.controls.target_origin.x.get()
                    + (x * self.cosine - y * self.sine) * self.scale.get(),
            )?,
            y: CoordinateMeters::new(
                self.controls.target_origin.y.get()
                    + (x * self.sine + y * self.cosine) * self.scale.get(),
            )?,
        })
    }
    pub fn to_image(
        &self,
        target_frame: FrameId,
        point: Point2,
    ) -> Result<PixelPoint, ValidationError> {
        if target_frame != self.controls.target_frame {
            return Err(ValidationError::Inconsistent("floor frame mismatch"));
        }
        let x = (point.x.get() - self.controls.target_origin.x.get()) / self.scale.get();
        let y = (point.y.get() - self.controls.target_origin.y.get()) / self.scale.get();
        Ok(PixelPoint {
            x: Pixels::new(self.controls.image_first.x.get() + x * self.cosine + y * self.sine)?,
            y: Pixels::new(
                self.controls.image_first.y.get()
                    + (-x * self.sine + y * self.cosine) * self.controls.image_y_axis.sign(),
            )?,
        })
    }
}
impl TryFrom<CalibrationControls> for TwoPointCalibration {
    type Error = ValidationError;
    fn try_from(c: CalibrationControls) -> Result<Self, Self::Error> {
        Self::new(c)
    }
}
impl From<TwoPointCalibration> for CalibrationControls {
    fn from(c: TwoPointCalibration) -> Self {
        c.controls
    }
}
