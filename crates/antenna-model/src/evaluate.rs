use crate::{AntennaError, FrequencyPattern, GainSample, QuaternionWxyz, ValidatedAntenna};

/// A direction vector in world coordinates. Its magnitude is ignored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Direction3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Selects the matched or orthogonal pattern plane in the declared basis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolarizationComponent {
    CoPolar,
    CrossPolar,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainEvaluation {
    pub gain_dbi: f64,
    pub local_azimuth_degrees: f64,
    pub local_elevation_degrees: f64,
    pub lower_frequency_hz: u64,
    pub upper_frequency_hz: u64,
}

impl ValidatedAntenna {
    /// Evaluate a tabulated gain after rotating the world direction into the
    /// antenna's local frame. Polarization mismatch loss is intentionally not
    /// inferred; callers can request either stored plane explicitly.
    pub fn evaluate_gain(
        &self,
        frequency_hz: u64,
        world_direction: Direction3,
        component: PolarizationComponent,
    ) -> Result<GainEvaluation, AntennaError> {
        let local = inverse_rotate(self.mount_orientation(), normalize(world_direction)?)?;
        let azimuth = local.y.atan2(local.x).to_degrees().rem_euclid(360.0);
        let elevation = local.z.clamp(-1.0, 1.0).asin().to_degrees();
        let frequencies = &self.wire.frequencies;
        let (lower_index, upper_index) = frequency_bracket(frequencies, frequency_hz)?;
        let lower_gain = sample_direction(
            &self.wire.azimuth_degrees,
            &self.wire.elevation_degrees,
            &frequencies[lower_index],
            azimuth,
            elevation,
            component,
        )?;
        let upper_gain = if lower_index == upper_index {
            lower_gain
        } else {
            sample_direction(
                &self.wire.azimuth_degrees,
                &self.wire.elevation_degrees,
                &frequencies[upper_index],
                azimuth,
                elevation,
                component,
            )?
        };
        let lower_frequency_hz = frequencies[lower_index].frequency_hz;
        let upper_frequency_hz = frequencies[upper_index].frequency_hz;
        let gain_dbi = if lower_frequency_hz == upper_frequency_hz {
            lower_gain
        } else {
            let fraction = (frequency_hz - lower_frequency_hz) as f64
                / (upper_frequency_hz - lower_frequency_hz) as f64;
            interpolate_power_db(lower_gain, upper_gain, fraction)?
        };
        Ok(GainEvaluation {
            gain_dbi,
            local_azimuth_degrees: azimuth,
            local_elevation_degrees: elevation,
            lower_frequency_hz,
            upper_frequency_hz,
        })
    }
}

fn normalize(direction: Direction3) -> Result<Direction3, AntennaError> {
    let components = [direction.x, direction.y, direction.z];
    if components.iter().any(|value| !value.is_finite()) {
        return Err(AntennaError::Invalid("non-finite direction"));
    }
    let scale = direction
        .x
        .abs()
        .max(direction.y.abs())
        .max(direction.z.abs());
    if scale == 0.0 {
        return Err(AntennaError::Invalid("zero direction"));
    }
    let x = direction.x / scale;
    let y = direction.y / scale;
    let z = direction.z / scale;
    let norm = x.hypot(y).hypot(z);
    Ok(Direction3 {
        x: x / norm,
        y: y / norm,
        z: z / norm,
    })
}

fn inverse_rotate(
    quaternion: QuaternionWxyz,
    vector: Direction3,
) -> Result<Direction3, AntennaError> {
    // A validated unit quaternion maps local coordinates to world; its
    // conjugate therefore maps a world direction back into antenna-local axes.
    let qx = -quaternion.x;
    let qy = -quaternion.y;
    let qz = -quaternion.z;
    let qw = quaternion.w;
    let tx = 2.0 * (qy * vector.z - qz * vector.y);
    let ty = 2.0 * (qz * vector.x - qx * vector.z);
    let tz = 2.0 * (qx * vector.y - qy * vector.x);
    normalize(Direction3 {
        x: vector.x + qw * tx + (qy * tz - qz * ty),
        y: vector.y + qw * ty + (qz * tx - qx * tz),
        z: vector.z + qw * tz + (qx * ty - qy * tx),
    })
}

fn frequency_bracket(
    frequencies: &[FrequencyPattern],
    target: u64,
) -> Result<(usize, usize), AntennaError> {
    match frequencies.binary_search_by_key(&target, |pattern| pattern.frequency_hz) {
        Ok(index) => Ok((index, index)),
        Err(0) => Err(AntennaError::Unsupported("frequency below pattern range")),
        Err(index) if index == frequencies.len() => {
            Err(AntennaError::Unsupported("frequency above pattern range"))
        }
        Err(index) => Ok((index - 1, index)),
    }
}

fn sample_direction(
    azimuths: &[f64],
    elevations: &[f64],
    pattern: &FrequencyPattern,
    azimuth: f64,
    elevation: f64,
    component: PolarizationComponent,
) -> Result<f64, AntennaError> {
    if elevation <= -90.0 {
        return sample_component(pattern.samples[0], component);
    }
    if elevation >= 90.0 {
        return sample_component(
            pattern.samples[(elevations.len() - 1) * azimuths.len()],
            component,
        );
    }
    let (az0, az1, az_fraction) = periodic_bracket(azimuths, azimuth);
    let (el0, el1, el_fraction) = bounded_bracket(elevations, elevation);
    let azimuth_count = azimuths.len();
    let sample = |elevation_index: usize, azimuth_index: usize| {
        sample_component(
            pattern.samples[elevation_index * azimuth_count + azimuth_index],
            component,
        )
    };
    let lower = interpolate_power_db(sample(el0, az0)?, sample(el0, az1)?, az_fraction)?;
    let upper = interpolate_power_db(sample(el1, az0)?, sample(el1, az1)?, az_fraction)?;
    interpolate_power_db(lower, upper, el_fraction)
}

fn sample_component(
    sample: GainSample,
    component: PolarizationComponent,
) -> Result<f64, AntennaError> {
    match component {
        PolarizationComponent::CoPolar => Ok(sample.co_polar_gain_dbi),
        PolarizationComponent::CrossPolar => sample
            .cross_polar_gain_dbi
            .ok_or(AntennaError::Unsupported("cross-polar pattern unavailable")),
    }
}

fn bounded_bracket(axis: &[f64], target: f64) -> (usize, usize, f64) {
    match axis.binary_search_by(|value| value.total_cmp(&target)) {
        Ok(index) => (index, index, 0.0),
        Err(0) => (0, 0, 0.0),
        Err(index) if index == axis.len() => (axis.len() - 1, axis.len() - 1, 0.0),
        Err(index) => {
            let lower = index - 1;
            (
                lower,
                index,
                (target - axis[lower]) / (axis[index] - axis[lower]),
            )
        }
    }
}

fn periodic_bracket(axis: &[f64], target: f64) -> (usize, usize, f64) {
    match axis.binary_search_by(|value| value.total_cmp(&target)) {
        Ok(index) => (index, index, 0.0),
        Err(0) => {
            let lower = axis.len() - 1;
            let lower_angle = axis[lower] - 360.0;
            (lower, 0, (target - lower_angle) / (axis[0] - lower_angle))
        }
        Err(index) if index == axis.len() => {
            let lower = axis.len() - 1;
            (
                lower,
                0,
                (target - axis[lower]) / (axis[0] + 360.0 - axis[lower]),
            )
        }
        Err(index) => {
            let lower = index - 1;
            (
                lower,
                index,
                (target - axis[lower]) / (axis[index] - axis[lower]),
            )
        }
    }
}

fn interpolate_power_db(left_db: f64, right_db: f64, fraction: f64) -> Result<f64, AntennaError> {
    let left_power = 10.0_f64.powf(left_db / 10.0);
    let right_power = 10.0_f64.powf(right_db / 10.0);
    let power = left_power.mul_add(1.0 - fraction, right_power * fraction);
    let result = 10.0 * power.log10();
    if result.is_finite() {
        Ok(result)
    } else {
        Err(AntennaError::Invalid("unrepresentable interpolated gain"))
    }
}
