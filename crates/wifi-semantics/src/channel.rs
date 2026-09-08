//! Channel geometry and the intentionally small, versioned spectral-mask model.
//!
//! This module describes frequency geometry; it does not decide whether a
//! channel is legal in a jurisdiction.  Regulatory availability, DFS state,
//! power limits, and client support are separate inputs to the planner.  The
//! stable part implemented here is the 20 MHz channel-center numbering used to
//! construct 2.4, 5, and 6 GHz geometries.

use kyberia_domain::units::Megahertz;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CHANNEL_GEOMETRY_VERSION: ChannelGeometryVersion = ChannelGeometryVersion::V1;
const CENTER_CANONICAL_TOLERANCE_MHZ: f64 = 1e-8;

/// Versioned wire/semantic tag for channel geometry.  New validation or
/// numbering rules require a new closed variant and replay migration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelGeometryVersion {
    #[serde(rename = "kyberia-wifi-channel/1")]
    V1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiBand {
    Ghz2_4,
    Ghz5,
    Ghz6,
}

impl WifiBand {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ghz2_4 => "2.4 GHz",
            Self::Ghz5 => "5 GHz",
            Self::Ghz6 => "6 GHz",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelWidth {
    Mhz20,
    Mhz40,
    Mhz80,
    Mhz160,
    Mhz320,
    /// Two independent 80 MHz segments.  The two center frequencies are
    /// stored separately; the eight puncturing bits are low-to-high across
    /// the first segment and then the second segment.
    Mhz80Plus80,
}

impl ChannelWidth {
    pub const fn nominal_mhz(self) -> u16 {
        match self {
            Self::Mhz20 => 20,
            Self::Mhz40 => 40,
            Self::Mhz80 => 80,
            Self::Mhz160 => 160,
            Self::Mhz320 => 320,
            Self::Mhz80Plus80 => 160,
        }
    }

    pub const fn segment_count(self) -> usize {
        match self {
            Self::Mhz20 => 1,
            Self::Mhz40 => 2,
            Self::Mhz80 => 4,
            Self::Mhz160 => 8,
            Self::Mhz320 => 16,
            Self::Mhz80Plus80 => 8,
        }
    }

    const fn is_noncontiguous(self) -> bool {
        matches!(self, Self::Mhz80Plus80)
    }
}

/// Low-to-high 20 MHz segment puncturing.  Bits outside a geometry's segment
/// count are rejected when the mask is attached to that geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct PuncturingMask(u16);

impl PuncturingMask {
    pub const NONE: Self = Self(0);

    pub const fn new(bits: u16) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn is_punctured(self, segment: usize) -> bool {
        segment < 16 && self.0 & (1u16 << segment) != 0
    }
}

impl TryFrom<u16> for PuncturingMask {
    type Error = GeometryError;

    fn try_from(bits: u16) -> Result<Self, Self::Error> {
        Ok(Self(bits))
    }
}

impl From<PuncturingMask> for u16 {
    fn from(mask: PuncturingMask) -> Self {
        mask.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    Invalid(&'static str),
    Unsupported(&'static str),
    UnsupportedPuncturingPattern { width: ChannelWidth, mask: u16 },
}

impl std::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for GeometryError {}

/// All 20 MHz segments, including punctured ones, are retained so a caller
/// can render the requested bandwidth and the exact puncturing mask.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OccupiedSubchannel {
    pub channel: u16,
    pub center_frequency: Megahertz,
    pub punctured: bool,
    pub is_primary: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(try_from = "ChannelGeometryWire", into = "ChannelGeometryWire")]
pub struct ChannelGeometry {
    version: ChannelGeometryVersion,
    band: WifiBand,
    primary_channel: u16,
    center_frequency: Megahertz,
    second_center_frequency: Option<Megahertz>,
    width: ChannelWidth,
    puncturing: PuncturingMask,
}

impl ChannelGeometry {
    /// Construct a 20 MHz geometry directly from a standard channel number.
    /// Wider geometries must supply the bonded center frequency because a
    /// primary channel alone does not identify the secondary/center choice.
    pub fn from_primary(band: WifiBand, primary_channel: u16) -> Result<Self, GeometryError> {
        Self::new(
            band,
            primary_channel,
            channel_center_frequency(band, primary_channel)?,
            None,
            ChannelWidth::Mhz20,
            PuncturingMask::NONE,
        )
    }

    pub fn new(
        band: WifiBand,
        primary_channel: u16,
        center_frequency: Megahertz,
        second_center_frequency: Option<Megahertz>,
        width: ChannelWidth,
        puncturing: PuncturingMask,
    ) -> Result<Self, GeometryError> {
        if primary_channel == 0 {
            return Err(GeometryError::Invalid("primary channel must be nonzero"));
        }
        if !width_supported_in_band(band, width) {
            return Err(GeometryError::Unsupported(
                "channel width is not supported in this band",
            ));
        }
        channel_center_frequency(band, primary_channel)?;
        let expected_segments = width.segment_count();
        let valid_mask = if expected_segments == 16 {
            u16::MAX
        } else {
            (1u16 << expected_segments) - 1
        };
        if puncturing.bits() & !valid_mask != 0 {
            return Err(GeometryError::Invalid(
                "puncturing contains a segment outside the nominal width",
            ));
        }

        if !matches!(puncturing, PuncturingMask::NONE) {
            match width {
                ChannelWidth::Mhz20 | ChannelWidth::Mhz40 => {
                    return Err(GeometryError::Invalid(
                        "puncturing is not modeled for 20/40 MHz geometry",
                    ));
                }
                ChannelWidth::Mhz80 if !VALID_PUNCTURING_80.contains(&puncturing.bits()) => {
                    return Err(GeometryError::UnsupportedPuncturingPattern {
                        width,
                        mask: puncturing.bits(),
                    });
                }
                ChannelWidth::Mhz160 if !VALID_PUNCTURING_160.contains(&puncturing.bits()) => {
                    return Err(GeometryError::UnsupportedPuncturingPattern {
                        width,
                        mask: puncturing.bits(),
                    });
                }
                ChannelWidth::Mhz320 if !VALID_PUNCTURING_320.contains(&puncturing.bits()) => {
                    return Err(GeometryError::UnsupportedPuncturingPattern {
                        width,
                        mask: puncturing.bits(),
                    });
                }
                ChannelWidth::Mhz80Plus80 => {
                    return Err(GeometryError::UnsupportedPuncturingPattern {
                        width,
                        mask: puncturing.bits(),
                    });
                }
                _ => {}
            }
        }

        if width.is_noncontiguous() {
            let second = second_center_frequency.ok_or(GeometryError::Invalid(
                "80+80 geometry requires a second center frequency",
            ))?;
            if center_frequency.get() >= second.get() {
                return Err(GeometryError::Invalid(
                    "80+80 centers must be canonicalized from low to high",
                ));
            }
            // Two 80 MHz blocks need a real spectral gap.  Touching blocks
            // are contiguous 160 MHz and belong to Mhz160 instead.
            if second.get() - center_frequency.get() <= 80.0 {
                return Err(GeometryError::Invalid(
                    "80+80 segments must not overlap or be adjacent",
                ));
            }
        } else if second_center_frequency.is_some() {
            return Err(GeometryError::Invalid(
                "contiguous geometry cannot carry a second center frequency",
            ));
        }

        let (canonical_center, mut segment_centers) =
            canonical_bonded_center(band, width, center_frequency.get())?;
        let canonical_second = if let Some(second) = second_center_frequency {
            let (canonical, second_segments) =
                canonical_bonded_center(band, ChannelWidth::Mhz80, second.get())?;
            if canonical_center.get() >= canonical.get() {
                return Err(GeometryError::Invalid(
                    "80+80 centers must be canonicalized from low to high",
                ));
            }
            if canonical.get() - canonical_center.get() <= 80.0 {
                return Err(GeometryError::Invalid(
                    "80+80 segments must not overlap or be adjacent",
                ));
            }
            segment_centers.extend(second_segments);
            Some(canonical)
        } else {
            None
        };

        let mut primary_index = None;
        let mut seen_centers = BTreeSet::new();
        for (index, frequency) in segment_centers.iter().copied().enumerate() {
            // A segment center must itself be a standard 20 MHz center.  This
            // admits channel geometry without making a jurisdiction claim.
            let channel =
                channel_for_frequency(band, frequency).ok_or(GeometryError::Unsupported(
                    "segment center is outside the supported band numbering",
                ))?;
            if !seen_centers.insert(frequency.to_bits()) {
                return Err(GeometryError::Invalid(
                    "bonded geometry contains duplicate 20 MHz segments",
                ));
            }
            if channel == primary_channel {
                primary_index = Some(index);
            }
        }
        let primary_index = primary_index.ok_or(GeometryError::Invalid(
            "primary channel is not a 20 MHz segment of the bonded geometry",
        ))?;
        if puncturing.is_punctured(primary_index) {
            return Err(GeometryError::Invalid(
                "primary segment cannot be punctured",
            ));
        }
        if (0..expected_segments).all(|index| puncturing.is_punctured(index)) {
            return Err(GeometryError::Invalid(
                "puncturing cannot remove every segment",
            ));
        }

        Ok(Self {
            version: CHANNEL_GEOMETRY_VERSION,
            band,
            primary_channel,
            center_frequency: canonical_center,
            second_center_frequency: canonical_second,
            width,
            puncturing,
        })
    }

    pub fn primary_frequency(&self) -> Megahertz {
        // Construction validated this lookup, so the result cannot fail for
        // a value created through the public constructor or wire decoder.
        channel_center_frequency(self.band, self.primary_channel)
            .expect("validated channel geometry has a primary frequency")
    }

    pub const fn version(&self) -> ChannelGeometryVersion {
        self.version
    }

    pub const fn band(&self) -> WifiBand {
        self.band
    }

    pub const fn primary_channel(&self) -> u16 {
        self.primary_channel
    }

    pub const fn center_frequency(&self) -> Megahertz {
        self.center_frequency
    }

    pub const fn second_center_frequency(&self) -> Option<Megahertz> {
        self.second_center_frequency
    }

    pub const fn width(&self) -> ChannelWidth {
        self.width
    }

    pub const fn puncturing(&self) -> PuncturingMask {
        self.puncturing
    }

    pub fn occupied_subchannels(&self) -> Vec<OccupiedSubchannel> {
        let mut centers = Vec::with_capacity(self.width.segment_count());
        append_segment_centers(&mut centers, self.center_frequency.get(), self.width);
        if let Some(second) = self.second_center_frequency {
            append_segment_centers(&mut centers, second.get(), ChannelWidth::Mhz80);
        }
        let primary_frequency = self.primary_frequency().get();
        centers
            .into_iter()
            .enumerate()
            .map(|(index, frequency)| OccupiedSubchannel {
                channel: channel_for_frequency(self.band, frequency)
                    .expect("validated geometry has standard segment centers"),
                center_frequency: Megahertz::new(frequency)
                    .expect("validated geometry has positive segment centers"),
                punctured: self.puncturing.is_punctured(index),
                is_primary: approx_equal(frequency, primary_frequency),
            })
            .collect()
    }

    pub fn active_subchannels(&self) -> impl Iterator<Item = OccupiedSubchannel> {
        self.occupied_subchannels()
            .into_iter()
            .filter(|segment| !segment.punctured)
    }

    pub fn primary_segment_index(&self) -> usize {
        self.occupied_subchannels()
            .iter()
            .position(|segment| segment.is_primary)
            .expect("validated geometry has a primary segment")
    }
}

/// Frequency mapping that is stable independently of regulatory availability.
/// Only the standardized 20 MHz primary/segment lattices below are accepted;
/// region-specific legality, DFS state, and power limits remain separate
/// policy inputs.  This is deliberately not a complete jurisdiction table.
pub fn channel_center_frequency(band: WifiBand, channel: u16) -> Result<Megahertz, GeometryError> {
    let frequency = match band {
        WifiBand::Ghz2_4 if (1..=13).contains(&channel) => 2407.0 + 5.0 * channel as f64,
        WifiBand::Ghz2_4 if channel == 14 => 2484.0,
        WifiBand::Ghz5 if STANDARD_5_GHZ_CHANNELS.contains(&channel) => {
            5000.0 + 5.0 * channel as f64
        }
        WifiBand::Ghz6 if STANDARD_6_GHZ_CHANNELS.contains(&channel) => {
            5950.0 + 5.0 * channel as f64
        }
        WifiBand::Ghz2_4 => {
            return Err(GeometryError::Unsupported(
                "2.4 GHz channel number is outside the stable center mapping",
            ));
        }
        WifiBand::Ghz5 | WifiBand::Ghz6 => {
            return Err(GeometryError::Unsupported(
                "5/6 GHz channel number is outside the stable center mapping",
            ));
        }
    };
    Megahertz::new(frequency).map_err(|_| GeometryError::Invalid("frequency is not finite"))
}

/// Return the canonical center and the canonical 20 MHz segment centers for a
/// contiguous width. A caller may provide a value within the wire tolerance
/// of a standard center, but storage always uses the exact standard value.
fn canonical_bonded_center(
    band: WifiBand,
    width: ChannelWidth,
    center: f64,
) -> Result<(Megahertz, Vec<f64>), GeometryError> {
    let mut raw_segments = Vec::with_capacity(width.segment_count());
    append_segment_centers(&mut raw_segments, center, width);
    let mut canonical_segments = Vec::with_capacity(raw_segments.len());
    let mut seen_channels = BTreeSet::new();
    for raw in raw_segments {
        let channel = channel_for_frequency(band, raw).ok_or(GeometryError::Unsupported(
            "bonded center has a segment outside the supported channel lattice",
        ))?;
        let standard = channel_center_frequency(band, channel)
            .expect("channel_for_frequency returned a standard channel")
            .get();
        if (raw - standard).abs() > CENTER_CANONICAL_TOLERANCE_MHZ {
            return Err(GeometryError::Invalid(
                "bonded center is shifted from the standardized frequency",
            ));
        }
        if !seen_channels.insert(channel) {
            return Err(GeometryError::Invalid(
                "bonded geometry contains duplicate 20 MHz segments",
            ));
        }
        canonical_segments.push(standard);
    }
    let canonical_center = canonical_segments.iter().sum::<f64>() / canonical_segments.len() as f64;
    if (center - canonical_center).abs() > CENTER_CANONICAL_TOLERANCE_MHZ {
        return Err(GeometryError::Invalid(
            "center frequency is not a standardized bonded center",
        ));
    }
    Ok((
        Megahertz::new(canonical_center)
            .map_err(|_| GeometryError::Invalid("canonical center is not finite"))?,
        canonical_segments,
    ))
}

fn channel_for_frequency(band: WifiBand, frequency: f64) -> Option<u16> {
    if !frequency.is_finite() {
        return None;
    }
    let channel = match band {
        WifiBand::Ghz2_4 if approx_equal(frequency, 2484.0) => 14.0,
        WifiBand::Ghz2_4 => (frequency - 2407.0) / 5.0,
        WifiBand::Ghz5 => (frequency - 5000.0) / 5.0,
        WifiBand::Ghz6 => (frequency - 5950.0) / 5.0,
    };
    let rounded = channel.round();
    if (channel - rounded).abs() > 1e-8 || !(1.0..=233.0).contains(&rounded) {
        return None;
    }
    let candidate = rounded as u16;
    ((channel_center_frequency(band, candidate).ok()?.get() - frequency).abs() <= 1e-8)
        .then_some(candidate)
}

fn append_segment_centers(result: &mut Vec<f64>, center: f64, width: ChannelWidth) {
    let count = if matches!(width, ChannelWidth::Mhz80Plus80) {
        4
    } else {
        width.segment_count()
    };
    let midpoint = (count as f64 - 1.0) / 2.0;
    for index in 0..count {
        result.push(center + (index as f64 - midpoint) * 20.0);
    }
}

const STANDARD_5_GHZ_CHANNELS: &[u16] = &[
    36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144,
    149, 153, 157, 161, 165, 169, 173, 177,
];

// Conservative subset copied as values from Linux cfg80211's
// `valid_puncturing_bitmap` tables in `net/wireless/chan.c` (GPL-2.0-only
// source, values independently represented here). The table is used as an
// evidence-backed admission rule, not as a runtime Linux dependency.
const VALID_PUNCTURING_80: &[u16] = &[0x8, 0x4, 0x2, 0x1];
const VALID_PUNCTURING_160: &[u16] = &[
    0x80, 0x40, 0x20, 0x10, 0x8, 0x4, 0x2, 0x1, 0xc0, 0x30, 0xc, 0x3,
];
const VALID_PUNCTURING_320: &[u16] = &[
    0xc000, 0x3000, 0xc00, 0x300, 0xc0, 0x30, 0xc, 0x3, 0xf000, 0xf00, 0xf0, 0xf, 0xfc00, 0xf300,
    0xf0c0, 0xf030, 0xf00c, 0xf003, 0xc00f, 0x300f, 0xc0f, 0x30f, 0xcf, 0x3f,
];

const STANDARD_6_GHZ_CHANNELS: &[u16] = &[
    1, 5, 9, 13, 17, 21, 25, 29, 33, 37, 41, 45, 49, 53, 57, 61, 65, 69, 73, 77, 81, 85, 89, 93,
    97, 101, 105, 109, 113, 117, 121, 125, 129, 133, 137, 141, 145, 149, 153, 157, 161, 165, 169,
    173, 177, 181, 185, 189, 193, 197, 201, 205, 209, 213, 217, 221, 225, 229, 233,
];

const fn width_supported_in_band(band: WifiBand, width: ChannelWidth) -> bool {
    match band {
        WifiBand::Ghz2_4 => matches!(width, ChannelWidth::Mhz20 | ChannelWidth::Mhz40),
        WifiBand::Ghz5 => !matches!(width, ChannelWidth::Mhz320),
        WifiBand::Ghz6 => true,
    }
}

fn approx_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-8
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelGeometryWire {
    version: ChannelGeometryVersion,
    band: WifiBand,
    primary_channel: u16,
    center_frequency: Megahertz,
    second_center_frequency: Option<Megahertz>,
    width: ChannelWidth,
    puncturing: PuncturingMask,
}

impl TryFrom<ChannelGeometryWire> for ChannelGeometry {
    type Error = GeometryError;

    fn try_from(wire: ChannelGeometryWire) -> Result<Self, Self::Error> {
        if wire.version != CHANNEL_GEOMETRY_VERSION {
            return Err(GeometryError::Unsupported(
                "unknown channel geometry version",
            ));
        }
        Self::new(
            wire.band,
            wire.primary_channel,
            wire.center_frequency,
            wire.second_center_frequency,
            wire.width,
            wire.puncturing,
        )
    }
}

impl From<ChannelGeometry> for ChannelGeometryWire {
    fn from(geometry: ChannelGeometry) -> Self {
        Self {
            version: geometry.version,
            band: geometry.band,
            primary_channel: geometry.primary_channel,
            center_frequency: geometry.center_frequency,
            second_center_frequency: geometry.second_center_frequency,
            width: geometry.width,
            puncturing: geometry.puncturing,
        }
    }
}
