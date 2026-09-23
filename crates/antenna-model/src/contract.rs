use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

pub const SCHEMA_VERSION: &str = "kyberia.antenna-pattern/1";
pub const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_JSON_DEPTH: usize = 32;
pub const MAX_FREQUENCIES: usize = 256;
pub const MAX_AZIMUTHS: usize = 720;
pub const MAX_ELEVATIONS: usize = 361;
pub const MAX_TOTAL_SAMPLES: usize = 1_000_000;
pub const MAX_TEXT_BYTES: usize = 2_048;
pub const MAX_VALIDATION_WORK: u64 = 24_000_000;

const QUATERNION_NORM_TOLERANCE: f64 = 1.0e-9;
const POLE_GAIN_TOLERANCE_DBI: f64 = 1.0e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkLimits {
    pub max_steps: u64,
}

impl Default for WorkLimits {
    fn default() -> Self {
        Self {
            max_steps: MAX_VALIDATION_WORK,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkUsage {
    pub steps: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkBudget {
    limits: WorkLimits,
    usage: WorkUsage,
}

impl WorkBudget {
    pub const fn new(limits: WorkLimits) -> Self {
        Self {
            limits,
            usage: WorkUsage { steps: 0 },
        }
    }

    pub const fn usage(&self) -> WorkUsage {
        self.usage
    }

    fn charge(&mut self, steps: u64) -> Result<(), AntennaError> {
        let next = self
            .usage
            .steps
            .checked_add(steps)
            .ok_or(AntennaError::ResourceLimit("validation work"))?;
        if next > self.limits.max_steps {
            return Err(AntennaError::ResourceLimit("validation work"));
        }
        self.usage.steps = next;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AntennaError {
    InputTooLarge,
    JsonDepthExceeded,
    InvalidJson(String),
    Invalid(&'static str),
    Unsupported(&'static str),
    ResourceLimit(&'static str),
}

impl fmt::Display for AntennaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => write!(formatter, "antenna JSON exceeds byte limit"),
            Self::JsonDepthExceeded => write!(formatter, "antenna JSON exceeds depth limit"),
            Self::InvalidJson(message) => write!(formatter, "invalid antenna JSON: {message}"),
            Self::Invalid(message) => write!(formatter, "invalid antenna pattern: {message}"),
            Self::Unsupported(message) => {
                write!(formatter, "unsupported antenna pattern: {message}")
            }
            Self::ResourceLimit(message) => write!(formatter, "antenna resource limit: {message}"),
        }
    }
}

impl std::error::Error for AntennaError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Schema {
    #[serde(rename = "kyberia.antenna-pattern/1")]
    V1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolarizationBasis {
    LinearHorizontalVertical,
    CircularRightLeft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Representation {
    #[serde(rename = "full_sphere_sample_grid")]
    FullSphereSampleGrid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum SpatialInterpolation {
    #[serde(rename = "bilinear_linear_power")]
    BilinearLinearPower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum FrequencyInterpolation {
    #[serde(rename = "linear_power_reject_outside")]
    LinearPowerRejectOutside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum SampleOrder {
    #[serde(rename = "elevation_major_azimuth_minor")]
    ElevationMajorAzimuthMinor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum CoordinateFrame {
    #[serde(rename = "right_handed_x_forward_y_left_z_up")]
    RightHandedXForwardYLeftZUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum AngleUnit {
    #[serde(rename = "degrees")]
    Degrees,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum AzimuthZeroAxis {
    #[serde(rename = "positive_x")]
    PositiveX,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum PositiveAzimuth {
    #[serde(rename = "toward_positive_y")]
    TowardPositiveY,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum PositiveElevation {
    #[serde(rename = "toward_positive_z")]
    TowardPositiveZ,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoordinateConvention {
    frame: CoordinateFrame,
    angle_unit: AngleUnit,
    azimuth_zero_axis: AzimuthZeroAxis,
    positive_azimuth: PositiveAzimuth,
    positive_elevation: PositiveElevation,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuaternionWxyz {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl QuaternionWxyz {
    pub const IDENTITY: Self = Self {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceProvenance {
    source_uri: String,
    license_spdx: String,
    source_checksum_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuantityDbi {
    value_dbi: f64,
    uncertainty_db: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Efficiency {
    fraction: f64,
    uncertainty: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GainSample {
    pub(crate) co_polar_gain_dbi: f64,
    #[serde(default)]
    pub(crate) cross_polar_gain_dbi: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrequencyPattern {
    pub(crate) frequency_hz: u64,
    nominal_gain: QuantityDbi,
    efficiency: Efficiency,
    pub(crate) samples: Vec<GainSample>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AntennaWire {
    schema: Schema,
    model_id: String,
    representation: Representation,
    coordinate_convention: CoordinateConvention,
    mount_orientation_local_to_world: QuaternionWxyz,
    polarization_basis: PolarizationBasis,
    spatial_interpolation: SpatialInterpolation,
    frequency_interpolation: FrequencyInterpolation,
    sample_order: SampleOrder,
    normalization_tolerance_db: f64,
    source: SourceProvenance,
    pub(crate) azimuth_degrees: Vec<f64>,
    pub(crate) elevation_degrees: Vec<f64>,
    pub(crate) frequencies: Vec<FrequencyPattern>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AntennaIdentity([u8; 32]);

impl AntennaIdentity {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            write!(&mut output, "{byte:02x}").expect("String formatting cannot fail");
        }
        output
    }
}

#[derive(Clone, Debug)]
pub struct ValidatedAntenna {
    pub(crate) wire: AntennaWire,
    identity: AntennaIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrequencyMetadata {
    pub frequency_hz: u64,
    pub nominal_gain_dbi: f64,
    pub nominal_gain_uncertainty_db: f64,
    pub efficiency_fraction: f64,
    pub efficiency_uncertainty: f64,
}

impl ValidatedAntenna {
    /// Parse, validate and canonicalize one v1 JSON pattern.
    pub fn parse_json(input: &[u8]) -> Result<Self, AntennaError> {
        let mut budget = WorkBudget::new(WorkLimits::default());
        Self::parse_json_with_budget(input, &mut budget)
    }

    /// The caller may lower the default deterministic work allowance.
    pub fn parse_json_with_budget(
        input: &[u8],
        budget: &mut WorkBudget,
    ) -> Result<Self, AntennaError> {
        if input.len() > MAX_JSON_BYTES {
            return Err(AntennaError::InputTooLarge);
        }
        budget.charge(u64::try_from(input.len()).map_err(|_| AntennaError::InputTooLarge)?)?;
        validate_json_depth(input)?;
        let mut wire: AntennaWire = serde_json::from_slice(input)
            .map_err(|error| AntennaError::InvalidJson(error.to_string()))?;
        validate_and_canonicalize(&mut wire, budget)?;
        let canonical = serde_json::to_vec(&wire)
            .map_err(|error| AntennaError::InvalidJson(error.to_string()))?;
        budget.charge(
            u64::try_from(canonical.len())
                .map_err(|_| AntennaError::ResourceLimit("identity encoding"))?,
        )?;
        let mut digest = Sha256::new();
        digest.update(b"kyberia-antenna-pattern-canonical-v1\0");
        digest.update(&canonical);
        Ok(Self {
            wire,
            identity: AntennaIdentity(digest.finalize().into()),
        })
    }

    pub const fn identity(&self) -> AntennaIdentity {
        self.identity
    }

    pub fn model_id(&self) -> &str {
        &self.wire.model_id
    }

    pub const fn mount_orientation(&self) -> QuaternionWxyz {
        self.wire.mount_orientation_local_to_world
    }

    pub const fn polarization_basis(&self) -> PolarizationBasis {
        self.wire.polarization_basis
    }

    pub fn supported_frequency_hz(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
        self.wire
            .frequencies
            .iter()
            .map(|pattern| pattern.frequency_hz)
    }

    pub fn frequency_metadata(&self) -> impl ExactSizeIterator<Item = FrequencyMetadata> + '_ {
        self.wire
            .frequencies
            .iter()
            .map(|pattern| FrequencyMetadata {
                frequency_hz: pattern.frequency_hz,
                nominal_gain_dbi: pattern.nominal_gain.value_dbi,
                nominal_gain_uncertainty_db: pattern.nominal_gain.uncertainty_db,
                efficiency_fraction: pattern.efficiency.fraction,
                efficiency_uncertainty: pattern.efficiency.uncertainty,
            })
    }

    pub fn source_uri(&self) -> &str {
        &self.wire.source.source_uri
    }

    pub fn license_spdx(&self) -> &str {
        &self.wire.source.license_spdx
    }

    pub fn source_checksum_sha256(&self) -> &str {
        &self.wire.source.source_checksum_sha256
    }

    /// Return canonical v1 JSON. Missing and explicit-null cross-polar values
    /// have the same normalized representation.
    pub fn canonical_json(&self) -> Vec<u8> {
        serde_json::to_vec(&self.wire).expect("validated wire serialization is infallible")
    }
}

fn validate_json_depth(input: &[u8]) -> Result<(), AntennaError> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in input {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth
                    .checked_add(1)
                    .ok_or(AntennaError::JsonDepthExceeded)?;
                if depth > MAX_JSON_DEPTH {
                    return Err(AntennaError::JsonDepthExceeded);
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn validate_and_canonicalize(
    wire: &mut AntennaWire,
    budget: &mut WorkBudget,
) -> Result<(), AntennaError> {
    validate_identifier(&wire.model_id, "model id")?;
    validate_source_uri(&wire.source.source_uri)?;
    validate_text(&wire.source.license_spdx, "SPDX license expression")?;
    if !valid_spdx_expression(&wire.source.license_spdx) {
        return Err(AntennaError::Invalid("SPDX license expression"));
    }
    validate_sha256(&wire.source.source_checksum_sha256)?;
    canonicalize_quaternion(&mut wire.mount_orientation_local_to_world)?;
    canonicalize_zero(&mut wire.normalization_tolerance_db);
    if !wire.normalization_tolerance_db.is_finite()
        || !(0.0..=3.0).contains(&wire.normalization_tolerance_db)
    {
        return Err(AntennaError::Invalid("normalization tolerance"));
    }
    validate_axis(
        &mut wire.azimuth_degrees,
        MAX_AZIMUTHS,
        0.0,
        360.0,
        false,
        "azimuth axis",
        budget,
    )?;
    validate_axis(
        &mut wire.elevation_degrees,
        MAX_ELEVATIONS,
        -90.0,
        90.0,
        true,
        "elevation axis",
        budget,
    )?;
    if wire.elevation_degrees.first() != Some(&-90.0)
        || wire.elevation_degrees.last() != Some(&90.0)
    {
        return Err(AntennaError::Invalid(
            "elevation axis must include both poles",
        ));
    }
    if wire.azimuth_degrees.first() != Some(&0.0) {
        return Err(AntennaError::Invalid("azimuth axis must start at zero"));
    }
    if wire.frequencies.is_empty() || wire.frequencies.len() > MAX_FREQUENCIES {
        return Err(AntennaError::ResourceLimit("frequency count"));
    }
    let samples_per_frequency = wire
        .azimuth_degrees
        .len()
        .checked_mul(wire.elevation_degrees.len())
        .ok_or(AntennaError::ResourceLimit("sample count"))?;
    let total_samples = samples_per_frequency
        .checked_mul(wire.frequencies.len())
        .ok_or(AntennaError::ResourceLimit("sample count"))?;
    if total_samples > MAX_TOTAL_SAMPLES {
        return Err(AntennaError::ResourceLimit("sample count"));
    }

    let mut previous_frequency = None;
    let mut cross_polar_presence = None;
    for pattern in &mut wire.frequencies {
        budget.charge(1)?;
        if pattern.frequency_hz == 0
            || previous_frequency.is_some_and(|previous| previous >= pattern.frequency_hz)
        {
            return Err(AntennaError::Invalid(
                "frequencies must be positive and increasing",
            ));
        }
        previous_frequency = Some(pattern.frequency_hz);
        if pattern.samples.len() != samples_per_frequency {
            return Err(AntennaError::Invalid("sample grid dimensions"));
        }
        validate_quantity(&mut pattern.nominal_gain)?;
        validate_efficiency(&mut pattern.efficiency)?;
        let mut peak = f64::NEG_INFINITY;
        for sample in &mut pattern.samples {
            budget.charge(1)?;
            canonicalize_zero(&mut sample.co_polar_gain_dbi);
            if !valid_gain(sample.co_polar_gain_dbi) {
                return Err(AntennaError::Invalid("out-of-range co-polar gain"));
            }
            peak = peak.max(sample.co_polar_gain_dbi);
            if let Some(cross) = &mut sample.cross_polar_gain_dbi {
                canonicalize_zero(cross);
                if !valid_gain(*cross) {
                    return Err(AntennaError::Invalid("out-of-range cross-polar gain"));
                }
            }
            let present = sample.cross_polar_gain_dbi.is_some();
            if cross_polar_presence.is_some_and(|expected| expected != present) {
                return Err(AntennaError::Invalid("partial cross-polar sample plane"));
            }
            cross_polar_presence = Some(present);
        }
        let allowed_error = pattern.nominal_gain.uncertainty_db + wire.normalization_tolerance_db;
        if (peak - pattern.nominal_gain.value_dbi).abs() > allowed_error {
            return Err(AntennaError::Invalid(
                "nominal gain does not match pattern peak",
            ));
        }
        validate_poles(pattern, wire.azimuth_degrees.len())?;
    }
    Ok(())
}

fn validate_axis(
    axis: &mut [f64],
    maximum_count: usize,
    minimum: f64,
    maximum: f64,
    inclusive_maximum: bool,
    name: &'static str,
    budget: &mut WorkBudget,
) -> Result<(), AntennaError> {
    if axis.len() < 2 || axis.len() > maximum_count {
        return Err(AntennaError::ResourceLimit(name));
    }
    let mut previous = None;
    for value in axis {
        budget.charge(1)?;
        canonicalize_zero(value);
        let in_range = value.is_finite()
            && *value >= minimum
            && if inclusive_maximum {
                *value <= maximum
            } else {
                *value < maximum
            };
        if !in_range || previous.is_some_and(|prior| prior >= *value) {
            return Err(AntennaError::Invalid(name));
        }
        previous = Some(*value);
    }
    Ok(())
}

fn validate_poles(pattern: &FrequencyPattern, azimuth_count: usize) -> Result<(), AntennaError> {
    let south = &pattern.samples[..azimuth_count];
    let north = &pattern.samples[pattern.samples.len() - azimuth_count..];
    for pole in [south, north] {
        let first = pole[0];
        if pole.iter().any(|sample| {
            (sample.co_polar_gain_dbi - first.co_polar_gain_dbi).abs() > POLE_GAIN_TOLERANCE_DBI
                || match (sample.cross_polar_gain_dbi, first.cross_polar_gain_dbi) {
                    (Some(left), Some(right)) => (left - right).abs() > POLE_GAIN_TOLERANCE_DBI,
                    (None, None) => false,
                    _ => true,
                }
        }) {
            return Err(AntennaError::Invalid("azimuth-dependent pole gain"));
        }
    }
    Ok(())
}

fn validate_quantity(quantity: &mut QuantityDbi) -> Result<(), AntennaError> {
    canonicalize_zero(&mut quantity.value_dbi);
    canonicalize_zero(&mut quantity.uncertainty_db);
    if !valid_gain(quantity.value_dbi)
        || !quantity.uncertainty_db.is_finite()
        || !(0.0..=30.0).contains(&quantity.uncertainty_db)
    {
        return Err(AntennaError::Invalid("gain uncertainty"));
    }
    Ok(())
}

fn valid_gain(value: f64) -> bool {
    value.is_finite() && (-300.0..=100.0).contains(&value)
}

fn validate_efficiency(efficiency: &mut Efficiency) -> Result<(), AntennaError> {
    canonicalize_zero(&mut efficiency.fraction);
    canonicalize_zero(&mut efficiency.uncertainty);
    if !efficiency.fraction.is_finite()
        || !efficiency.uncertainty.is_finite()
        || !(0.0..=1.0).contains(&efficiency.fraction)
        || !(0.0..=1.0).contains(&efficiency.uncertainty)
        || efficiency.fraction - efficiency.uncertainty < 0.0
        || efficiency.fraction + efficiency.uncertainty > 1.0
    {
        return Err(AntennaError::Invalid("efficiency uncertainty"));
    }
    Ok(())
}

fn canonicalize_quaternion(quaternion: &mut QuaternionWxyz) -> Result<(), AntennaError> {
    let values = [quaternion.w, quaternion.x, quaternion.y, quaternion.z];
    if values
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1.0)
    {
        return Err(AntennaError::Invalid("mount orientation component"));
    }
    let norm = quaternion
        .w
        .hypot(quaternion.x)
        .hypot(quaternion.y)
        .hypot(quaternion.z);
    if (norm - 1.0).abs() > QUATERNION_NORM_TOLERANCE {
        return Err(AntennaError::Invalid("non-unit mount orientation"));
    }
    quaternion.w /= norm;
    quaternion.x /= norm;
    quaternion.y /= norm;
    quaternion.z /= norm;
    let first = [quaternion.w, quaternion.x, quaternion.y, quaternion.z]
        .into_iter()
        .find(|value| *value != 0.0)
        .ok_or(AntennaError::Invalid("zero mount orientation"))?;
    if first.is_sign_negative() {
        quaternion.w = -quaternion.w;
        quaternion.x = -quaternion.x;
        quaternion.y = -quaternion.y;
        quaternion.z = -quaternion.z;
    }
    canonicalize_zero(&mut quaternion.w);
    canonicalize_zero(&mut quaternion.x);
    canonicalize_zero(&mut quaternion.y);
    canonicalize_zero(&mut quaternion.z);
    Ok(())
}

fn validate_identifier(value: &str, name: &'static str) -> Result<(), AntennaError> {
    validate_text(value, name)?;
    if value.trim() != value {
        return Err(AntennaError::Invalid(name));
    }
    Ok(())
}

fn validate_source_uri(value: &str) -> Result<(), AntennaError> {
    validate_text(value, "source URI")?;
    let remainder = ["https://", "http://", "file://", "urn:"]
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix));
    if remainder.is_none_or(|suffix| {
        suffix.is_empty() || !suffix.bytes().all(|byte| (b'!'..=b'~').contains(&byte))
    }) {
        return Err(AntennaError::Invalid("source URI"));
    }
    Ok(())
}

fn validate_text(value: &str, name: &'static str) -> Result<(), AntennaError> {
    if value.is_empty()
        || value.len() > MAX_TEXT_BYTES
        || !value.bytes().all(|byte| (b' '..=b'~').contains(&byte))
    {
        return Err(AntennaError::Invalid(name));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), AntennaError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AntennaError::Invalid("source SHA-256"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LicenseToken<'a> {
    Identifier(&'a str),
    And,
    Or,
    With,
    LeftParen,
    RightParen,
}

fn valid_spdx_expression(source: &str) -> bool {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b' ' {
            cursor += 1;
            continue;
        }
        if bytes[cursor] == b'(' {
            tokens.push(LicenseToken::LeftParen);
            cursor += 1;
        } else if bytes[cursor] == b')' {
            tokens.push(LicenseToken::RightParen);
            cursor += 1;
        } else {
            let start = cursor;
            while cursor < bytes.len()
                && (bytes[cursor].is_ascii_alphanumeric()
                    || matches!(bytes[cursor], b'-' | b'.' | b'+' | b'_' | b':'))
            {
                cursor += 1;
            }
            if cursor == start {
                return false;
            }
            let token = &source[start..cursor];
            tokens.push(match token {
                "AND" => LicenseToken::And,
                "OR" => LicenseToken::Or,
                "WITH" => LicenseToken::With,
                _ => LicenseToken::Identifier(token),
            });
        }
    }
    if tokens.is_empty() || tokens.len() > MAX_TEXT_BYTES / 2 {
        return false;
    }
    let mut parser = LicenseParser { tokens, cursor: 0 };
    parser.parse_or(0) && parser.cursor == parser.tokens.len()
}

struct LicenseParser<'a> {
    tokens: Vec<LicenseToken<'a>>,
    cursor: usize,
}

impl LicenseParser<'_> {
    fn parse_or(&mut self, depth: usize) -> bool {
        if !self.parse_and(depth) {
            return false;
        }
        while self.consume(LicenseToken::Or) {
            if !self.parse_and(depth) {
                return false;
            }
        }
        true
    }

    fn parse_and(&mut self, depth: usize) -> bool {
        if !self.parse_atom(depth) {
            return false;
        }
        while self.consume(LicenseToken::And) {
            if !self.parse_atom(depth) {
                return false;
            }
        }
        true
    }

    fn parse_atom(&mut self, depth: usize) -> bool {
        if depth > 32 {
            return false;
        }
        match self.peek() {
            Some(LicenseToken::Identifier(_)) => {
                self.cursor += 1;
                if self.consume(LicenseToken::With) {
                    if matches!(self.peek(), Some(LicenseToken::Identifier(_))) {
                        self.cursor += 1;
                    } else {
                        return false;
                    }
                }
                true
            }
            Some(LicenseToken::LeftParen) => {
                self.cursor += 1;
                if !self.parse_or(depth + 1) || !self.consume(LicenseToken::RightParen) {
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    fn peek(&self) -> Option<LicenseToken<'_>> {
        self.tokens.get(self.cursor).copied()
    }

    fn consume(&mut self, expected: LicenseToken<'_>) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }
}

fn canonicalize_zero(value: &mut f64) {
    if *value == 0.0 {
        *value = 0.0;
    }
}
