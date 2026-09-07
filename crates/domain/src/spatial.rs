//! Right-handed Cartesian positions: x/y in the named frame, +z up.
//! Pixel coordinates are a separate type; transforms live in the spatial crate.
use crate::{
    ValidationError,
    evidence::Evidence,
    identity::{FrameId, PoseId, Text},
    units::*,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point3 {
    pub x: CoordinateMeters,
    pub y: CoordinateMeters,
    pub z: CoordinateMeters,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PixelPoint {
    pub x: Pixels,
    pub y: Pixels,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrameKind {
    ImagePixels,
    FloorLocalMeters,
    BuildingLocalMeters,
    ProjectedCrs { definition: Text },
    ArSessionMeters,
    SensorLocalMeters,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinateFrame {
    pub id: FrameId,
    pub name: Text,
    pub kind: FrameKind,
}

/// Symmetric 3x3 position covariance in square meters, packed xx,xy,xz,yy,yz,zz.
/// Validated by diagonal-pivoted LDL elimination, with a residual-entry
/// tolerance of 64 epsilon after scaling by the largest entry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64;6]", into = "[f64;6]")]
pub struct PositionCovariance([f64; 6]);
impl PositionCovariance {
    pub fn new(values: [f64; 6]) -> Result<Self, ValidationError> {
        if !values.iter().all(|x| x.is_finite()) {
            return Err(ValidationError::InvalidCovariance);
        }
        let scale = values.iter().fold(0.0_f64, |a, b| a.max(b.abs()));
        if scale == 0.0 {
            return Ok(Self([0.0; 6]));
        }
        let [a, b, c, d, e, f] = values.map(|v| v / scale);
        if values[0] < 0.0
            || values[3] < 0.0
            || values[5] < 0.0
            || (values[0] == 0.0 && (values[1] != 0.0 || values[2] != 0.0))
            || (values[3] == 0.0 && (values[1] != 0.0 || values[4] != 0.0))
            || (values[5] == 0.0 && (values[2] != 0.0 || values[4] != 0.0))
        {
            return Err(ValidationError::InvalidCovariance);
        }
        let mut matrix = [[a, b, c], [b, d, e], [c, e, f]];
        let tolerance = 64.0 * f64::EPSILON;
        // Pivoting on the greatest remaining diagonal bounds the elimination
        // multiplier for PSD input and handles valid singular matrices.
        for k in 0..3 {
            let mut pivot = k;
            for i in k..3 {
                if matrix[i][i] < -tolerance {
                    return Err(ValidationError::InvalidCovariance);
                }
                if matrix[i][i] > matrix[pivot][pivot] {
                    pivot = i;
                }
            }
            matrix.swap(k, pivot);
            for row in &mut matrix {
                row.swap(k, pivot);
            }
            let diagonal = matrix[k][k];
            if diagonal <= tolerance {
                // A PSD matrix whose remaining variances are zero cannot
                // have nonzero cross-covariance. Bound residual entries,
                // rather than their squared products or determinant.
                if matrix[k..]
                    .iter()
                    .any(|row| row[k..].iter().any(|v| v.abs() > tolerance))
                {
                    return Err(ValidationError::InvalidCovariance);
                }
                break;
            }
            for i in k + 1..3 {
                for j in i..3 {
                    let residual = matrix[i][j] - (matrix[i][k] / diagonal) * matrix[k][j];
                    if !residual.is_finite() {
                        return Err(ValidationError::InvalidCovariance);
                    }
                    matrix[i][j] = residual;
                    matrix[j][i] = residual;
                }
            }
        }
        Ok(Self(values.map(|v| if v == 0.0 { 0.0 } else { v })))
    }
    pub const fn packed(self) -> [f64; 6] {
        self.0
    }
}
impl TryFrom<[f64; 6]> for PositionCovariance {
    type Error = ValidationError;
    fn try_from(v: [f64; 6]) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<PositionCovariance> for [f64; 6] {
    fn from(c: PositionCovariance) -> Self {
        c.0
    }
}

/// Intrinsic Z-Y-X rotation: yaw about +z, pitch about rotated +y,
/// roll about rotated +x. All angles use the right-hand rule.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Orientation {
    pub yaw: Radians,
    pub pitch: Radians,
    pub roll: Radians,
}

/// An immutable pose artifact reference plus its current association revision.
/// Repositioning observations creates another association, not altered raw bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoseReference {
    pub pose_id: PoseId,
    pub frame_id: FrameId,
    pub assignment_version: Text,
    pub position: Point3,
    pub covariance: Evidence<PositionCovariance>,
    pub orientation: Evidence<Orientation>,
    pub method_version: Text,
}
