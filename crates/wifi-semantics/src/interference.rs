//! Deterministic channel coupling and effective-interference arithmetic.
//!
//! The coupling model is an explanatory approximation, not a regulatory
//! emission mask or a final Wi-Fi SINR implementation. It uses fixed 1 MHz
//! midpoint bins, flat transmitter power over each active 20 MHz segment, and
//! a desired receiver response with a unit 20 MHz core and linear 5 MHz
//! shoulders. The coefficient is the receiver-weighted fraction of the
//! interferer's total integrated received power, so it is bounded but
//! intentionally asymmetric for unequal channel widths.
//!
//! Sionna path-gain output, when present, is an input to a caller's received
//! power field.  This module never consumes or emits Sionna SINR.

use crate::channel::ChannelGeometry;
use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{BssId, MldId, ObservationId, PhysicalDeviceId, RadioId},
    units::{Dbm, Milliwatts, Probability},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const INTERFERENCE_ALGORITHM_VERSION: InterferenceAlgorithmVersion =
    InterferenceAlgorithmVersion::V1;
pub const MAX_INTERFERERS: usize = 10_000;
pub const SPECTRAL_SAMPLE_MHZ: f64 = 1.0;
const MASK_CORE_HALF_WIDTH_MHZ: f64 = 10.0;
const MASK_SHOULDER_WIDTH_MHZ: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterferenceAlgorithmVersion {
    #[serde(rename = "kyberia-wifi-interference/1")]
    V1,
}

/// The only shipped method is deliberately closed and versioned.  A future
/// standards-derived mask must add a new variant and validation fixtures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpectralMaskMethod {
    Trapezoid20MhzReceiverV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedGeometryReason {
    UnknownBand,
    FutureWidth,
    FutureChannelNumbering,
    NonContiguousUnsupported,
}

/// A geometry at an analysis boundary can be known, absent, or explicitly
/// unsupported.  Unsupported is separate from malformed: an unknown future
/// value must not be silently interpreted as a zero-coupling channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum GeometryEvidence {
    Known(ChannelGeometry),
    Unknown(UnknownReason),
    Unsupported(UnsupportedGeometryReason),
}

/// Completeness is an assertion about the observation set as a whole. An
/// incomplete capture cannot be interpreted as measured absence, even when
/// every row that happened to arrive has a known zero contribution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ObservationSetStatus {
    CompleteMeasured,
    CompleteScenario,
    Incomplete(UnknownReason),
}

impl ObservationSetStatus {
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::CompleteMeasured | Self::CompleteScenario)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CouplingValue {
    Known(Probability),
    Unknown(UnknownReason),
    Unsupported(UnsupportedGeometryReason),
}

impl CouplingValue {
    pub const fn known(value: Probability) -> Self {
        Self::Known(value)
    }
}

/// A caller must label every utilization value with its evidence class.  A
/// raw probability without this enum is intentionally not accepted by the
/// interference API.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "class", content = "utilization", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum UtilizationEvidence {
    MeasuredAdvertisedBssLoad(Probability),
    MeasuredCca(Probability),
    ObservedFrameLowerBound(Probability),
    SpectrumOccupancy(Probability),
    Inferred(Probability),
    ScenarioAssumption(Probability),
}

impl UtilizationEvidence {
    pub const fn value(self) -> Probability {
        match self {
            Self::MeasuredAdvertisedBssLoad(value)
            | Self::MeasuredCca(value)
            | Self::ObservedFrameLowerBound(value)
            | Self::SpectrumOccupancy(value)
            | Self::Inferred(value)
            | Self::ScenarioAssumption(value) => value,
        }
    }

    pub const fn class(self) -> UtilizationClass {
        match self {
            Self::MeasuredAdvertisedBssLoad(_) => UtilizationClass::MeasuredAdvertisedBssLoad,
            Self::MeasuredCca(_) => UtilizationClass::MeasuredCca,
            Self::ObservedFrameLowerBound(_) => UtilizationClass::ObservedFrameLowerBound,
            Self::SpectrumOccupancy(_) => UtilizationClass::SpectrumOccupancy,
            Self::Inferred(_) => UtilizationClass::Inferred,
            Self::ScenarioAssumption(_) => UtilizationClass::ScenarioAssumption,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtilizationClass {
    MeasuredAdvertisedBssLoad,
    MeasuredCca,
    ObservedFrameLowerBound,
    SpectrumOccupancy,
    Inferred,
    ScenarioAssumption,
}

/// Zero is a valid result in the linear domain, but zero cannot be converted
/// to a finite dBm value.  Positive values use the domain's strict mW type.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "milliwatts", rename_all = "snake_case")]
pub enum LinearPower {
    Zero,
    Positive(Milliwatts),
}

impl LinearPower {
    pub const fn is_zero(self) -> bool {
        matches!(self, Self::Zero)
    }

    pub const fn as_milliwatts(self) -> Option<Milliwatts> {
        match self {
            Self::Zero => None,
            Self::Positive(value) => Some(value),
        }
    }

    fn from_f64(value: f64) -> Result<Self, InterferenceError> {
        if !value.is_finite() || value < 0.0 {
            return Err(InterferenceError::NumericalFailure);
        }
        if value == 0.0 {
            Ok(Self::Zero)
        } else {
            Ok(Self::Positive(
                Milliwatts::new(value).map_err(|_| InterferenceError::NumericalFailure)?,
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SameBssPolicy {
    /// Same-BSS/MLD evidence is coordinated and omitted from self-
    /// interference.  It is still retained as an inspectable exclusion.
    ExcludeCoordinated,
    /// The caller asserts that same-BSS links should be scored independently
    /// for this scenario (for example, a deliberately adversarial load case).
    CountIndependently,
    /// Same-BSS/MLD evidence without a caller policy is an error.  This is
    /// available for safety-critical integrations that cannot assume policy.
    RejectAmbiguous,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioIdentity {
    pub radio_id: RadioId,
    pub physical_device_id: Option<PhysicalDeviceId>,
    pub bss_id: Option<BssId>,
    pub mld_id: Option<MldId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesiredChannel {
    pub identity: RadioIdentity,
    pub geometry: GeometryEvidence,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfererInput {
    /// Identifies the immutable evidence row.  The canonical radio identity
    /// below is what performs physical-radio deduplication.
    pub input_id: ObservationId,
    pub identity: RadioIdentity,
    pub geometry: GeometryEvidence,
    pub received_power: Evidence<LinearPower>,
    pub utilization: Evidence<UtilizationEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    SamePhysicalRadio,
    SameBssCoordinated,
    SameMldCoordinated,
    NoSpectralCoupling,
    UnknownGeometry,
    UnsupportedGeometry,
    UnknownReceivedPower,
    UnknownUtilization,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InterfererContribution {
    /// All evidence rows collapsed into this physical-radio contribution,
    /// sorted by immutable identity.  This is how duplicate BSSID/MLD views
    /// remain inspectable without double-counting the radio.
    pub input_ids: Vec<ObservationId>,
    /// All caller-supplied aliases observed for this canonical radio, kept in
    /// deterministic input order.
    pub identities: Vec<RadioIdentity>,
    pub canonical_radio_id: RadioId,
    /// The source geometry evidence remains visible even when its coupling or
    /// effective power is unknown/unsupported.
    pub geometry: GeometryEvidence,
    pub coupling: CouplingValue,
    pub utilization: Evidence<UtilizationEvidence>,
    pub received_power: Evidence<LinearPower>,
    pub effective_power: Evidence<LinearPower>,
    pub disposition: ContributionDisposition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum ContributionDisposition {
    Included,
    Excluded(ExclusionReason),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PowerSummary {
    pub milliwatts: Evidence<LinearPower>,
    /// Zero has no finite dBm representation and is explicitly reported as
    /// NotMeasured.  This avoids presenting `-inf` as a valid domain Dbm.
    pub dbm: Evidence<Dbm>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterferenceAssumption {
    OneMhzNumericalIntegration,
    TrapezoidTwentyMhzCoreAndFiveMhzShoulder,
    ReceivedPowerIsTotalIntegratedInterfererPower,
    UtilizationMultipliesReceivedLinearPower,
    WiFiSemanticsComputedOutsidePropagationEngine,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectiveInterference {
    pub algorithm_version: InterferenceAlgorithmVersion,
    pub method: SpectralMaskMethod,
    pub same_bss_policy: SameBssPolicy,
    pub observation_set: ObservationSetStatus,
    pub aggregate_status: InterferenceAggregateStatus,
    pub total: PowerSummary,
    pub contributions: Vec<InterfererContribution>,
    pub input_ids: Vec<ObservationId>,
    pub assumptions: Vec<InterferenceAssumption>,
}

/// A typed aggregate state keeps unsupported or incomplete totals visible at
/// the API boundary. Callers never need to infer total state by walking
/// individual contributions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum InterferenceAggregateStatus {
    Complete,
    Incomplete(UnknownReason),
    Unknown(UnknownReason),
    Unsupported(UnsupportedGeometryReason),
    NumericalFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterferenceError {
    ResourceLimit,
    DuplicateInput(ObservationId),
    DesiredGeometryUnknown(UnknownReason),
    DesiredGeometryUnsupported(UnsupportedGeometryReason),
    AmbiguousDuplicate(RadioId),
    AmbiguousBssAlias(BssId),
    AmbiguousSameBss(RadioId),
    NumericalFailure,
}

impl std::fmt::Display for InterferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for InterferenceError {}

/// Compute the receiver-weighted fraction of an interferer's total integrated
/// received power. Each active 20 MHz segment contributes twenty 1 MHz
/// midpoint bins with equal transmitter power; the denominator is therefore
/// the interferer's complete active-channel power. The desired receiver has a
/// unit response through the 20 MHz core and a linear response to zero across
/// the next 5 MHz on each side. This is bounded and deterministic, but
/// intentionally asymmetric for unequal widths.
pub fn coupling(
    desired: &ChannelGeometry,
    interferer: &ChannelGeometry,
    method: SpectralMaskMethod,
) -> CouplingValue {
    match method {
        SpectralMaskMethod::Trapezoid20MhzReceiverV1 => {}
    }
    let desired_segments: Vec<_> = desired
        .active_subchannels()
        .map(|segment| segment.center_frequency.get())
        .collect();
    let interferer_segments: Vec<_> = interferer
        .active_subchannels()
        .map(|segment| segment.center_frequency.get())
        .collect();
    if desired_segments.is_empty() || interferer_segments.is_empty() {
        return CouplingValue::Known(Probability::new(0.0).expect("zero is a probability"));
    }

    let mut accepted_power_bins = 0.0;
    for center in interferer_segments {
        // Midpoints are -9.5, -8.5, ..., +9.5 MHz around every 20 MHz
        // segment center. Every bin has equal transmitter power.
        for index in 0..20 {
            let offset = (index as f64 + 0.5) * SPECTRAL_SAMPLE_MHZ - 10.0;
            let frequency = center + offset;
            accepted_power_bins += receiver_response(&desired_segments, frequency);
        }
    }
    let total_interferer_bins = interferer
        .active_subchannels()
        .count()
        .saturating_mul(20)
        .max(1);
    // Quantization removes platform-dependent last-bit drift while remaining
    // far below the precision warranted by this V1 approximation.
    let coefficient = ((accepted_power_bins / total_interferer_bins as f64).clamp(0.0, 1.0) * 1e12)
        .round()
        / 1e12;
    CouplingValue::Known(Probability::new(coefficient).expect("bounded coupling is a probability"))
}

fn receiver_response(desired_segment_centers: &[f64], frequency: f64) -> f64 {
    desired_segment_centers
        .iter()
        .map(|center| {
            let distance = (frequency - *center).abs();
            if distance <= MASK_CORE_HALF_WIDTH_MHZ {
                1.0
            } else if distance < MASK_CORE_HALF_WIDTH_MHZ + MASK_SHOULDER_WIDTH_MHZ {
                (MASK_CORE_HALF_WIDTH_MHZ + MASK_SHOULDER_WIDTH_MHZ - distance)
                    / MASK_SHOULDER_WIDTH_MHZ
            } else {
                0.0
            }
        })
        .fold(0.0, f64::max)
}

/// Apply explicit utilization and received-power evidence to each interferer,
/// deduplicate canonical radios, and return a fully inspectable contribution
/// list. A missing nonzero contributor or incomplete observation set makes the
/// aggregate unknown; a complete measured/scenario empty set may be known
/// zero, and explicit zero power remains distinct from missing evidence.
pub fn effective_interference(
    desired: &DesiredChannel,
    interferers: &[InterfererInput],
    method: SpectralMaskMethod,
    same_bss_policy: SameBssPolicy,
    observation_set: ObservationSetStatus,
) -> Result<EffectiveInterference, InterferenceError> {
    if interferers.len() > MAX_INTERFERERS {
        return Err(InterferenceError::ResourceLimit);
    }
    let desired_geometry = match &desired.geometry {
        GeometryEvidence::Known(geometry) => geometry,
        GeometryEvidence::Unknown(reason) => {
            return Err(InterferenceError::DesiredGeometryUnknown(reason.clone()));
        }
        GeometryEvidence::Unsupported(reason) => {
            return Err(InterferenceError::DesiredGeometryUnsupported(*reason));
        }
    };
    let groups = deduplicate(interferers)?;
    let input_ids = interferers
        .iter()
        .map(|item| item.input_id)
        .collect::<BTreeSet<_>>();
    let mut contributions = Vec::with_capacity(groups.len());
    let mut known_powers = Vec::new();
    let mut aggregate_issue = None;

    for group in groups {
        let primary = group
            .first()
            .expect("deduplication creates nonempty groups");
        let same_relation = relation(&desired.identity, &group);
        let policy_exclusion = match same_relation {
            Some(Relation::PhysicalRadio) => Some(ExclusionReason::SamePhysicalRadio),
            Some(Relation::Bss) if matches!(same_bss_policy, SameBssPolicy::ExcludeCoordinated) => {
                Some(ExclusionReason::SameBssCoordinated)
            }
            Some(Relation::Mld) if matches!(same_bss_policy, SameBssPolicy::ExcludeCoordinated) => {
                Some(ExclusionReason::SameMldCoordinated)
            }
            Some(Relation::Bss | Relation::Mld)
                if matches!(same_bss_policy, SameBssPolicy::RejectAmbiguous) =>
            {
                return Err(InterferenceError::AmbiguousSameBss(
                    primary.identity.radio_id,
                ));
            }
            _ => None,
        };

        let coupling_value = match (&primary.geometry, &desired.geometry) {
            (GeometryEvidence::Known(interferer_geometry), GeometryEvidence::Known(_)) => {
                coupling(desired_geometry, interferer_geometry, method)
            }
            (GeometryEvidence::Unknown(reason), _) | (_, GeometryEvidence::Unknown(reason)) => {
                CouplingValue::Unknown(reason.clone())
            }
            (GeometryEvidence::Unsupported(reason), _) => CouplingValue::Unsupported(*reason),
            (_, GeometryEvidence::Unsupported(reason)) => CouplingValue::Unsupported(*reason),
        };

        let mut disposition = policy_exclusion.map(ContributionDisposition::Excluded);
        let effective = if policy_exclusion.is_some() {
            Evidence::Known(LinearPower::Zero)
        } else {
            effective_product(
                &coupling_value,
                &primary.received_power,
                &primary.utilization,
                &mut disposition,
            )
        };
        if policy_exclusion.is_none() {
            match &coupling_value {
                CouplingValue::Unsupported(reason) => record_aggregate_issue(
                    &mut aggregate_issue,
                    InterferenceAggregateStatus::Unsupported(*reason),
                ),
                CouplingValue::Unknown(reason) => record_aggregate_issue(
                    &mut aggregate_issue,
                    InterferenceAggregateStatus::Unknown(reason.clone()),
                ),
                CouplingValue::Known(_) => {}
            }
        }
        if policy_exclusion.is_none()
            && let Evidence::Unknown(reason) = &effective
        {
            if *reason == UnknownReason::SolverFailure {
                record_aggregate_issue(
                    &mut aggregate_issue,
                    InterferenceAggregateStatus::NumericalFailure,
                );
            } else {
                record_aggregate_issue(
                    &mut aggregate_issue,
                    InterferenceAggregateStatus::Unknown(reason.clone()),
                );
            }
        }
        if disposition.is_none() {
            if let Evidence::Known(LinearPower::Positive(value)) = effective {
                known_powers.push(value);
            }
            disposition = Some(ContributionDisposition::Included);
        }
        // A known zero product can legitimately come from a non-overlapping
        // channel; retain an exclusion reason for explainability while keeping
        // total power known.
        if matches!(coupling_value, CouplingValue::Known(value) if value.get() == 0.0)
            && policy_exclusion.is_none()
        {
            disposition = Some(ContributionDisposition::Excluded(
                ExclusionReason::NoSpectralCoupling,
            ));
        }
        contributions.push(InterfererContribution {
            input_ids: group.iter().map(|item| item.input_id).collect(),
            identities: group.iter().map(|item| item.identity.clone()).collect(),
            canonical_radio_id: primary.identity.radio_id,
            geometry: primary.geometry.clone(),
            coupling: coupling_value,
            utilization: primary.utilization.clone(),
            received_power: primary.received_power.clone(),
            effective_power: effective,
            disposition: disposition.expect("every contribution has a disposition"),
        });
    }

    let aggregate_status = aggregate_issue.unwrap_or_else(|| match &observation_set {
        ObservationSetStatus::CompleteMeasured | ObservationSetStatus::CompleteScenario => {
            InterferenceAggregateStatus::Complete
        }
        ObservationSetStatus::Incomplete(reason) => {
            InterferenceAggregateStatus::Incomplete(reason.clone())
        }
    });
    let total_milliwatts = match &aggregate_status {
        InterferenceAggregateStatus::Complete => Evidence::Known(sum_linear(&known_powers)?),
        InterferenceAggregateStatus::Incomplete(reason)
        | InterferenceAggregateStatus::Unknown(reason) => Evidence::Unknown(reason.clone()),
        InterferenceAggregateStatus::Unsupported(_) => {
            Evidence::Unknown(UnknownReason::UnsupportedCapability)
        }
        InterferenceAggregateStatus::NumericalFailure => {
            Evidence::Unknown(UnknownReason::SolverFailure)
        }
    };
    let total = PowerSummary {
        dbm: power_dbm(&total_milliwatts),
        milliwatts: total_milliwatts,
    };
    Ok(EffectiveInterference {
        algorithm_version: INTERFERENCE_ALGORITHM_VERSION,
        method,
        same_bss_policy,
        observation_set,
        aggregate_status,
        total,
        contributions,
        input_ids: input_ids.into_iter().collect(),
        assumptions: vec![
            InterferenceAssumption::OneMhzNumericalIntegration,
            InterferenceAssumption::TrapezoidTwentyMhzCoreAndFiveMhzShoulder,
            InterferenceAssumption::ReceivedPowerIsTotalIntegratedInterfererPower,
            InterferenceAssumption::UtilizationMultipliesReceivedLinearPower,
            InterferenceAssumption::WiFiSemanticsComputedOutsidePropagationEngine,
        ],
    })
}

fn record_aggregate_issue(
    current: &mut Option<InterferenceAggregateStatus>,
    candidate: InterferenceAggregateStatus,
) {
    let replace = current
        .as_ref()
        .is_none_or(|existing| aggregate_status_rank(&candidate) > aggregate_status_rank(existing));
    if replace {
        *current = Some(candidate);
    }
}

fn aggregate_status_rank(status: &InterferenceAggregateStatus) -> u8 {
    match status {
        InterferenceAggregateStatus::Complete => 0,
        InterferenceAggregateStatus::Incomplete(_) => 1,
        InterferenceAggregateStatus::Unknown(_) => 2,
        // Unsupported input is kept as the aggregate state even if another
        // contribution also encounters a numerical failure; the caller must
        // be able to see that the requested aggregate is unsupported without
        // inspecting arbitrary contribution rows.
        InterferenceAggregateStatus::NumericalFailure => 3,
        InterferenceAggregateStatus::Unsupported(_) => 4,
    }
}

fn effective_product(
    coupling: &CouplingValue,
    power: &Evidence<LinearPower>,
    utilization: &Evidence<UtilizationEvidence>,
    disposition: &mut Option<ContributionDisposition>,
) -> Evidence<LinearPower> {
    if matches!(coupling, CouplingValue::Known(value) if value.get() == 0.0) {
        return Evidence::Known(LinearPower::Zero);
    }
    if matches!(power, Evidence::Known(LinearPower::Zero)) {
        return Evidence::Known(LinearPower::Zero);
    }
    if matches!(utilization, Evidence::Known(value) if value.value().get() == 0.0) {
        return Evidence::Known(LinearPower::Zero);
    }
    let coefficient = match coupling {
        CouplingValue::Known(value) => value.get(),
        CouplingValue::Unknown(reason) => {
            *disposition = Some(ContributionDisposition::Excluded(
                ExclusionReason::UnknownGeometry,
            ));
            return Evidence::Unknown(reason.clone());
        }
        CouplingValue::Unsupported(reason) => {
            *disposition = Some(ContributionDisposition::Excluded(
                ExclusionReason::UnsupportedGeometry,
            ));
            let _ = reason;
            return Evidence::Unknown(UnknownReason::UnsupportedCapability);
        }
    };
    let received = match power {
        Evidence::Known(value) => *value,
        Evidence::Unknown(reason) => {
            *disposition = Some(ContributionDisposition::Excluded(
                ExclusionReason::UnknownReceivedPower,
            ));
            return Evidence::Unknown(reason.clone());
        }
    };
    if received.is_zero() || coefficient == 0.0 {
        return Evidence::Known(LinearPower::Zero);
    }
    let utilization = match utilization {
        Evidence::Known(value) => value.value().get(),
        Evidence::Unknown(reason) => {
            *disposition = Some(ContributionDisposition::Excluded(
                ExclusionReason::UnknownUtilization,
            ));
            return Evidence::Unknown(reason.clone());
        }
    };
    let received_milliwatts = received
        .as_milliwatts()
        .expect("nonzero received power has mW")
        .get();
    let product = received_milliwatts * coefficient * utilization;
    // A positive physical product underflowing to IEEE zero is numerical
    // failure, not evidence of a zero-power interferer.
    if product == 0.0 && received_milliwatts > 0.0 && coefficient > 0.0 && utilization > 0.0 {
        return Evidence::Unknown(UnknownReason::SolverFailure);
    }
    LinearPower::from_f64(product).map_or(
        Evidence::Unknown(UnknownReason::SolverFailure),
        Evidence::Known,
    )
}

fn sum_linear(values: &[Milliwatts]) -> Result<LinearPower, InterferenceError> {
    if values.is_empty() {
        return Ok(LinearPower::Zero);
    }
    let mut sorted: Vec<_> = values.iter().map(|value| value.get()).collect();
    sorted.sort_by(f64::total_cmp);
    let maximum = *sorted.last().expect("nonempty powers");
    let scaled = sorted.iter().map(|value| value / maximum).sum::<f64>();
    LinearPower::from_f64(maximum * scaled)
}

fn power_dbm(power: &Evidence<LinearPower>) -> Evidence<Dbm> {
    match power {
        Evidence::Unknown(reason) => Evidence::Unknown(reason.clone()),
        Evidence::Known(LinearPower::Zero) => Evidence::Unknown(UnknownReason::NotMeasured),
        Evidence::Known(LinearPower::Positive(value)) => Dbm::new(10.0 * value.get().log10())
            .map(Evidence::Known)
            .unwrap_or(Evidence::Unknown(UnknownReason::SolverFailure)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    PhysicalRadio,
    Bss,
    Mld,
}

fn relation(desired: &RadioIdentity, group: &[&InterfererInput]) -> Option<Relation> {
    if group
        .iter()
        .any(|item| item.identity.radio_id == desired.radio_id)
    {
        return Some(Relation::PhysicalRadio);
    }
    if desired.bss_id.is_some()
        && group
            .iter()
            .any(|item| item.identity.bss_id == desired.bss_id)
    {
        return Some(Relation::Bss);
    }
    if desired.mld_id.is_some()
        && group
            .iter()
            .any(|item| item.identity.mld_id == desired.mld_id)
    {
        return Some(Relation::Mld);
    }
    None
}

fn deduplicate(
    interferers: &[InterfererInput],
) -> Result<Vec<Vec<&InterfererInput>>, InterferenceError> {
    let mut by_radio: BTreeMap<RadioId, Vec<&InterfererInput>> = BTreeMap::new();
    let mut bss_owners: BTreeMap<BssId, RadioId> = BTreeMap::new();
    let mut input_ids = BTreeSet::new();
    for item in interferers {
        if !input_ids.insert(item.input_id) {
            return Err(InterferenceError::DuplicateInput(item.input_id));
        }
        if let Some(bss) = item.identity.bss_id
            && let Some(owner) = bss_owners.insert(bss, item.identity.radio_id)
            && owner != item.identity.radio_id
        {
            return Err(InterferenceError::AmbiguousBssAlias(bss));
        }
        by_radio
            .entry(item.identity.radio_id)
            .or_default()
            .push(item);
    }
    let mut groups = Vec::with_capacity(by_radio.len());
    for (radio_id, mut group) in by_radio {
        group.sort_by_key(|item| item.input_id);
        let first = group[0];
        if group.iter().skip(1).any(|item| {
            item.identity.physical_device_id != first.identity.physical_device_id
                || item.geometry != first.geometry
                || item.received_power != first.received_power
                || item.utilization != first.utilization
        }) {
            return Err(InterferenceError::AmbiguousDuplicate(radio_id));
        }
        groups.push(group);
    }
    Ok(groups)
}
