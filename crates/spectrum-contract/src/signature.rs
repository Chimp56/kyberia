use crate::{
    ProcessingLimits, SpectrumError, SpectrumSweep,
    model::{FrequencyGrid, PowerUnit, SpectrumBin},
    sha256,
};
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{ContentHash, SessionId, SourceId},
    spatial::PoseReference,
    time::MonotonicWindow,
};
use serde::{Deserialize, Serialize};

pub const SIGNATURE_INPUT_SCHEMA_V1: &str = "kyberia.spectrum-signature-input/1";
pub const EVENT_SCHEMA_V2: &str = "kyberia.spectrum-event/2";
pub const SIGNATURE_RULE_SET_V2: &str = "kyberia.spectrum-pattern-rules/2";
pub const TEMPORAL_POLICY_V1: &str = "kyberia.spectrum-temporal-eligibility/1";
pub const MIN_PERSISTENCE_SWEEPS: u32 = 5;
pub const MIN_PERSISTENCE_SPAN_NANOSECONDS: u64 = 1_000_000_000;
pub const MAX_INTER_SWEEP_GAP_NANOSECONDS: u64 = 300_000_000;
const PERSISTENCE_THRESHOLD_PARTS_PER_MILLION: u32 = 800_000;
/// A classified frequency bin must be determinate in at least 80% of all
/// sweeps in the event. Conditional persistence alone is insufficient.
pub const MIN_PERSISTENT_BIN_COVERAGE_PARTS_PER_MILLION: u32 = 800_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpectrumEventId(ContentHash);

impl SpectrumEventId {
    pub const fn content_hash(self) -> ContentHash {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepEvidenceReference {
    pub sequence: u64,
    pub sha256: ContentHash,
    pub capture_window: Evidence<MonotonicWindow>,
    pub pose: Evidence<PoseReference>,
    pub calibration_profile_sha256: Evidence<ContentHash>,
}

/// Exact, deterministic inputs to the pattern rules. Its canonical SHA-256 is
/// the identity of this analyzed evidence bundle, not an emitter identifier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureInputDocument {
    pub schema: String,
    pub rule_set: String,
    pub temporal_policy: String,
    pub source_id: SourceId,
    pub session_id: SessionId,
    pub grid: FrequencyGrid,
    pub power_unit: PowerUnit,
    pub threshold_milli_dbm: i32,
    pub sweeps: Vec<SweepEvidenceReference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignaturePattern {
    Unknown,
    NarrowbandPersistentPattern,
    WidebandPersistentPattern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureReason {
    PersistentNarrowbandEnergy,
    PersistentWidebandEnergy,
    InsufficientSweepCount,
    SequenceGap,
    MissingOrNonmonotonicTime,
    InsufficientTimeSpan,
    ExcessiveInterSweepGap,
    CalibrationUnavailable,
    ClippedEvidence,
    InsufficientFrequencyTimeSupport,
    InsufficientFrequencyLocalCoverage,
    PowerUnitUnsupported,
    NoSupportedPersistentPattern,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalSupport {
    pub sequence_is_contiguous: bool,
    pub monotonic_starts_are_strict: bool,
    pub start_span_nanoseconds: Evidence<u64>,
    pub maximum_inter_sweep_start_gap_nanoseconds: Evidence<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureAssessment {
    pub rule_set: String,
    pub temporal_policy: String,
    pub pattern: SignaturePattern,
    pub reason: SignatureReason,
    /// Deterministic support score, not a calibrated probability of emitter
    /// identity or classifier correctness.
    pub confidence_parts_per_million: u32,
    pub observed_cells: u64,
    pub occupied_cells: u64,
    pub frequency_time_support_parts_per_million: u32,
    pub active_sweep_support_parts_per_million: u32,
    pub persistent_occupied_bin_count: u32,
    pub widest_contiguous_persistent_bins: u32,
    pub minimum_persistent_bin_coverage_parts_per_million: u32,
    pub minimum_persistent_bin_support_parts_per_million: u32,
    pub temporal_support: TemporalSupport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpectrumEventDocument {
    pub schema: String,
    pub identity: SpectrumEventId,
    pub input: SignatureInputDocument,
    pub assessment: SignatureAssessment,
}

/// Deterministic, bounded signature result tied to the exact validated sweeps.
#[derive(Clone, Debug, PartialEq)]
pub struct SpectrumEvent {
    document: SpectrumEventDocument,
    canonical_bytes: Vec<u8>,
}

impl SpectrumEvent {
    /// `threshold_milli_dbm` is applied only to `DbmPerBin`. `DbmPerHertz`
    /// sweeps are retained in the input and receive an explicit unknown
    /// assessment until equivalent-noise-bandwidth normalization is modeled.
    pub fn from_sweeps(
        sweeps: &[SpectrumSweep],
        threshold_milli_dbm: i32,
        limits: ProcessingLimits,
    ) -> Result<Self, SpectrumError> {
        limits.validate()?;
        if sweeps.is_empty() || sweeps.len() > limits.max_sweeps_per_event {
            return Err(SpectrumError::ResourceLimit("spectrum event sweep count"));
        }
        if !(-500_000..=200_000).contains(&threshold_milli_dbm) {
            return Err(SpectrumError::Invalid("signature threshold"));
        }

        let mut ordered: Vec<&SpectrumSweep> = sweeps.iter().collect();
        ordered.sort_by_key(|sweep| sweep.document().sequence);
        let first = ordered[0].document();
        let work_units = ordered
            .len()
            .checked_mul(first.grid.bin_count as usize)
            .ok_or(SpectrumError::ResourceLimit("signature work"))?;
        if work_units > limits.max_work_units {
            return Err(SpectrumError::ResourceLimit("signature work"));
        }

        for pair in ordered.windows(2) {
            if pair[0].document().sequence == pair[1].document().sequence {
                return Err(SpectrumError::Invalid("duplicate sweep sequence"));
            }
        }
        for sweep in &ordered {
            let document = sweep.document();
            if document.session_id != first.session_id
                || document.source.source_id != first.source.source_id
                || document.grid != first.grid
                || document.power_unit != first.power_unit
            {
                return Err(SpectrumError::Invalid("incompatible spectrum sweeps"));
            }
        }

        let mut references = Vec::with_capacity(ordered.len());
        for sweep in &ordered {
            let document = sweep.document();
            let calibration_profile_sha256 = match &document.calibration {
                Evidence::Known(profile) => Evidence::Known(profile.profile_sha256),
                Evidence::Unknown(reason) => Evidence::Unknown(reason.clone()),
            };
            references.push(SweepEvidenceReference {
                sequence: document.sequence,
                sha256: sweep.sha256(),
                capture_window: document.capture_window.clone(),
                pose: document.pose.clone(),
                calibration_profile_sha256,
            });
        }
        let input = SignatureInputDocument {
            schema: SIGNATURE_INPUT_SCHEMA_V1.to_owned(),
            rule_set: SIGNATURE_RULE_SET_V2.to_owned(),
            temporal_policy: TEMPORAL_POLICY_V1.to_owned(),
            source_id: first.source.source_id,
            session_id: first.session_id,
            grid: first.grid,
            power_unit: first.power_unit,
            threshold_milli_dbm,
            sweeps: references,
        };
        let input_bytes = serde_json::to_vec(&input).map_err(|_| SpectrumError::Serialization)?;
        if input_bytes.len() > limits.max_canonical_bytes {
            return Err(SpectrumError::ResourceLimit("signature input bytes"));
        }
        let identity = SpectrumEventId(ContentHash::from_sha256(sha256(&input_bytes)));
        let assessment = assess(&ordered, threshold_milli_dbm, input.grid, input.power_unit)?;
        let document = SpectrumEventDocument {
            schema: EVENT_SCHEMA_V2.to_owned(),
            identity,
            input,
            assessment,
        };
        let canonical_bytes =
            serde_json::to_vec(&document).map_err(|_| SpectrumError::Serialization)?;
        if canonical_bytes.len() > limits.max_canonical_bytes {
            return Err(SpectrumError::ResourceLimit(
                "canonical spectrum event bytes",
            ));
        }
        Ok(Self {
            document,
            canonical_bytes,
        })
    }

    /// Decoding is bound to source sweeps and recomputes the complete event.
    /// Shape-valid but fabricated hashes or derived features are rejected.
    pub fn from_canonical_bytes(
        bytes: &[u8],
        sweeps: &[SpectrumSweep],
        threshold_milli_dbm: i32,
        limits: ProcessingLimits,
    ) -> Result<Self, SpectrumError> {
        limits.validate()?;
        if bytes.is_empty() || bytes.len() > limits.max_canonical_bytes {
            return Err(SpectrumError::ResourceLimit(
                "canonical spectrum event bytes",
            ));
        }
        let parsed: SpectrumEventDocument =
            serde_json::from_slice(bytes).map_err(|_| SpectrumError::Serialization)?;
        let parsed_bytes = serde_json::to_vec(&parsed).map_err(|_| SpectrumError::Serialization)?;
        if parsed_bytes != bytes {
            return Err(SpectrumError::NonCanonical);
        }
        if parsed.schema != EVENT_SCHEMA_V2
            || parsed.input.schema != SIGNATURE_INPUT_SCHEMA_V1
            || parsed.input.rule_set != SIGNATURE_RULE_SET_V2
            || parsed.input.temporal_policy != TEMPORAL_POLICY_V1
        {
            return Err(SpectrumError::UnsupportedSchema);
        }
        let event = Self::from_sweeps(sweeps, threshold_milli_dbm, limits)?;
        if event.canonical_bytes != bytes {
            return Err(SpectrumError::EvidenceMismatch);
        }
        Ok(event)
    }

    pub const fn document(&self) -> &SpectrumEventDocument {
        &self.document
    }

    pub const fn identity(&self) -> SpectrumEventId {
        self.document.identity
    }

    pub const fn assessment(&self) -> &SignatureAssessment {
        &self.document.assessment
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
}

fn assess(
    sweeps: &[&SpectrumSweep],
    threshold_milli_dbm: i32,
    grid: FrequencyGrid,
    power_unit: PowerUnit,
) -> Result<SignatureAssessment, SpectrumError> {
    let sequence_is_contiguous = sweeps.windows(2).all(|pair| {
        pair[0]
            .document()
            .sequence
            .checked_add(1)
            .is_some_and(|next| next == pair[1].document().sequence)
    });

    let temporal_support = temporal_support(sweeps);
    if power_unit == PowerUnit::DbmPerHertz {
        let observed_cells = sweeps
            .iter()
            .flat_map(|sweep| sweep.document().bins.iter())
            .filter(|bin| {
                matches!(
                    bin,
                    SpectrumBin::Observed { clipped: false, .. }
                        | SpectrumBin::BelowDetectionThreshold { .. }
                )
            })
            .count() as u64;
        let total_cells = u64::from(grid.bin_count) * sweeps.len() as u64;
        return Ok(SignatureAssessment {
            rule_set: SIGNATURE_RULE_SET_V2.to_owned(),
            temporal_policy: TEMPORAL_POLICY_V1.to_owned(),
            pattern: SignaturePattern::Unknown,
            reason: SignatureReason::PowerUnitUnsupported,
            confidence_parts_per_million: 0,
            observed_cells,
            occupied_cells: 0,
            frequency_time_support_parts_per_million: (observed_cells * 1_000_000 / total_cells)
                as u32,
            active_sweep_support_parts_per_million: 0,
            persistent_occupied_bin_count: 0,
            widest_contiguous_persistent_bins: 0,
            minimum_persistent_bin_coverage_parts_per_million: 0,
            minimum_persistent_bin_support_parts_per_million: 0,
            temporal_support,
        });
    }

    let mut calibrated = true;
    let mut calibration_hash: Option<ContentHash> = None;
    let mut clipped = false;
    for sweep in sweeps {
        match &sweep.document().calibration {
            Evidence::Known(profile) => {
                let flags_pass = matches!(&profile.clipping_checked, Evidence::Known(true))
                    && matches!(&profile.dynamic_range_checked, Evidence::Known(true));
                calibrated &= flags_pass;
                match calibration_hash {
                    Some(existing) if existing != profile.profile_sha256 => calibrated = false,
                    None => calibration_hash = Some(profile.profile_sha256),
                    _ => {}
                }
            }
            Evidence::Unknown(_) => calibrated = false,
        }
        clipped |= sweep
            .document()
            .bins
            .iter()
            .any(|bin| matches!(bin, SpectrumBin::Observed { clipped: true, .. }));
    }

    let mut observed_cells = 0_u64;
    let mut occupied_cells = 0_u64;
    let mut active_sweeps = 0_u32;
    let mut persistent_occupied_bin_count = 0_u32;
    let mut widest_contiguous_persistent_bins = 0_u32;
    let mut persistent_run = 0_u32;
    let mut insufficient_local_coverage = false;
    let mut minimum_persistent_bin_coverage_parts_per_million = u32::MAX;
    let mut minimum_persistent_bin_support_parts_per_million = u32::MAX;

    for bin_index in 0..grid.bin_count as usize {
        let mut above_threshold = 0_u32;
        let mut determinate = 0_u32;
        for sweep in sweeps {
            match &sweep.document().bins[bin_index] {
                SpectrumBin::Observed {
                    power_milli_dbm,
                    clipped: false,
                } => {
                    determinate += 1;
                    observed_cells += 1;
                    if *power_milli_dbm >= threshold_milli_dbm {
                        above_threshold += 1;
                        occupied_cells += 1;
                    }
                }
                SpectrumBin::BelowDetectionThreshold {
                    threshold_milli_dbm: detection_threshold,
                } if *detection_threshold <= threshold_milli_dbm => {
                    determinate += 1;
                    observed_cells += 1;
                }
                SpectrumBin::Observed { clipped: true, .. }
                | SpectrumBin::BelowDetectionThreshold { .. }
                | SpectrumBin::NotObserved { .. } => {}
            }
        }
        if determinate == 0 {
            persistent_run = 0;
            continue;
        }
        let coverage = (u64::from(determinate) * 1_000_000 / sweeps.len() as u64) as u32;
        let persistence = (u64::from(above_threshold) * 1_000_000 / u64::from(determinate)) as u32;
        if above_threshold > 0 && coverage < MIN_PERSISTENT_BIN_COVERAGE_PARTS_PER_MILLION {
            insufficient_local_coverage = true;
        }
        if determinate >= MIN_PERSISTENCE_SWEEPS
            && coverage >= MIN_PERSISTENT_BIN_COVERAGE_PARTS_PER_MILLION
            && persistence >= PERSISTENCE_THRESHOLD_PARTS_PER_MILLION
        {
            persistent_occupied_bin_count += 1;
            persistent_run += 1;
            widest_contiguous_persistent_bins =
                widest_contiguous_persistent_bins.max(persistent_run);
            minimum_persistent_bin_coverage_parts_per_million =
                minimum_persistent_bin_coverage_parts_per_million.min(coverage);
            minimum_persistent_bin_support_parts_per_million =
                minimum_persistent_bin_support_parts_per_million.min(persistence);
        } else {
            persistent_run = 0;
        }
    }

    for sweep in sweeps {
        let active = sweep.document().bins.iter().any(|bin| {
            matches!(bin, SpectrumBin::Observed { power_milli_dbm, clipped: false } if *power_milli_dbm >= threshold_milli_dbm)
        });
        active_sweeps += u32::from(active);
    }

    let total_cells = u64::from(grid.bin_count) * sweeps.len() as u64;
    let frequency_time_support_parts_per_million =
        (observed_cells * 1_000_000 / total_cells) as u32;
    let active_sweep_support_parts_per_million =
        (u64::from(active_sweeps) * 1_000_000 / sweeps.len() as u64) as u32;
    if persistent_occupied_bin_count == 0 {
        minimum_persistent_bin_coverage_parts_per_million = 0;
        minimum_persistent_bin_support_parts_per_million = 0;
    }

    let mut reason = SignatureReason::NoSupportedPersistentPattern;
    let mut pattern = SignaturePattern::Unknown;
    let mut confidence_parts_per_million = 0;

    if !sequence_is_contiguous {
        reason = SignatureReason::SequenceGap;
    } else if sweeps.len() < MIN_PERSISTENCE_SWEEPS as usize {
        reason = SignatureReason::InsufficientSweepCount;
    } else if !temporal_support.monotonic_starts_are_strict
        || !matches!(temporal_support.start_span_nanoseconds, Evidence::Known(_))
        || !matches!(
            temporal_support.maximum_inter_sweep_start_gap_nanoseconds,
            Evidence::Known(_)
        )
    {
        reason = SignatureReason::MissingOrNonmonotonicTime;
    } else if !matches!(
        temporal_support.start_span_nanoseconds,
        Evidence::Known(span) if span >= MIN_PERSISTENCE_SPAN_NANOSECONDS
    ) {
        reason = SignatureReason::InsufficientTimeSpan;
    } else if !matches!(
        temporal_support.maximum_inter_sweep_start_gap_nanoseconds,
        Evidence::Known(gap) if gap <= MAX_INTER_SWEEP_GAP_NANOSECONDS
    ) {
        reason = SignatureReason::ExcessiveInterSweepGap;
    } else if !calibrated {
        reason = SignatureReason::CalibrationUnavailable;
    } else if clipped {
        reason = SignatureReason::ClippedEvidence;
    } else if frequency_time_support_parts_per_million < PERSISTENCE_THRESHOLD_PARTS_PER_MILLION
        || active_sweep_support_parts_per_million < PERSISTENCE_THRESHOLD_PARTS_PER_MILLION
    {
        reason = SignatureReason::InsufficientFrequencyTimeSupport;
    } else if persistent_occupied_bin_count == 0 && insufficient_local_coverage {
        reason = SignatureReason::InsufficientFrequencyLocalCoverage;
    } else if persistent_occupied_bin_count > 0 && widest_contiguous_persistent_bins <= 2 {
        reason = SignatureReason::PersistentNarrowbandEnergy;
        pattern = SignaturePattern::NarrowbandPersistentPattern;
        confidence_parts_per_million = frequency_time_support_parts_per_million
            .min(active_sweep_support_parts_per_million)
            .min(minimum_persistent_bin_coverage_parts_per_million)
            .min(minimum_persistent_bin_support_parts_per_million);
    } else if u64::from(widest_contiguous_persistent_bins) * 4 >= u64::from(grid.bin_count)
        && persistent_occupied_bin_count > 2
    {
        reason = SignatureReason::PersistentWidebandEnergy;
        pattern = SignaturePattern::WidebandPersistentPattern;
        confidence_parts_per_million = frequency_time_support_parts_per_million
            .min(active_sweep_support_parts_per_million)
            .min(minimum_persistent_bin_coverage_parts_per_million)
            .min(minimum_persistent_bin_support_parts_per_million);
    }

    Ok(SignatureAssessment {
        rule_set: SIGNATURE_RULE_SET_V2.to_owned(),
        temporal_policy: TEMPORAL_POLICY_V1.to_owned(),
        pattern,
        reason,
        confidence_parts_per_million,
        observed_cells,
        occupied_cells,
        frequency_time_support_parts_per_million,
        active_sweep_support_parts_per_million,
        persistent_occupied_bin_count,
        widest_contiguous_persistent_bins,
        minimum_persistent_bin_coverage_parts_per_million,
        minimum_persistent_bin_support_parts_per_million,
        temporal_support,
    })
}

fn temporal_support(sweeps: &[&SpectrumSweep]) -> TemporalSupport {
    let mut starts = Vec::with_capacity(sweeps.len());
    let mut epoch = None;
    let mut valid = true;
    for sweep in sweeps {
        match &sweep.document().capture_window {
            Evidence::Known(window) => {
                let start = window.start();
                if epoch.is_some_and(|known| known != start.epoch) {
                    valid = false;
                }
                epoch = Some(start.epoch);
                starts.push(start.nanoseconds);
            }
            Evidence::Unknown(_) => valid = false,
        }
    }
    let monotonic_starts_are_strict = valid && starts.windows(2).all(|pair| pair[1] > pair[0]);
    let (start_span_nanoseconds, maximum_inter_sweep_start_gap_nanoseconds) =
        if monotonic_starts_are_strict {
            let span = starts.last().and_then(|last| last.checked_sub(starts[0]));
            let max_gap = starts
                .windows(2)
                .filter_map(|pair| pair[1].checked_sub(pair[0]))
                .max();
            (
                span.map_or(
                    Evidence::Unknown(UnknownReason::ClockUnavailable),
                    Evidence::Known,
                ),
                max_gap.map_or(
                    Evidence::Unknown(UnknownReason::ClockUnavailable),
                    Evidence::Known,
                ),
            )
        } else {
            (
                Evidence::Unknown(UnknownReason::ClockUnavailable),
                Evidence::Unknown(UnknownReason::ClockUnavailable),
            )
        };
    TemporalSupport {
        sequence_is_contiguous: sweeps.windows(2).all(|pair| {
            pair[0]
                .document()
                .sequence
                .checked_add(1)
                .is_some_and(|next| next == pair[1].document().sequence)
        }),
        monotonic_starts_are_strict,
        start_span_nanoseconds,
        maximum_inter_sweep_start_gap_nanoseconds,
    }
}
