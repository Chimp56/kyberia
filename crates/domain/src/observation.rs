//! V2 normalized evidence envelope with explicit V1 read migration.
//! Unknown values retain reasons; derived predictions cannot enter scan payloads.
use crate::{ValidationError, evidence::*, identity::*, spatial::PoseReference, time::*, units::*};
use serde::{Deserialize, Serialize};
mod wire;
pub use wire::{DecodedObservation, ObservationDecodeReceipt, ObservationInputVersion};

/// Current observation schema only. Other domain contracts retain their own V1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationSchemaVersion {
    #[serde(rename = "2")]
    V2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    NativeApi,
    NativeMonitor,
    RemoteApi,
    DatabaseImport,
    PacketImport,
    Controller,
    SpectrumDevice,
    Mobile,
    SyntheticFixture,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceDescriptor {
    pub source_id: SourceId,
    pub collector_id: CollectorId,
    pub sensor_id: Evidence<SensorId>,
    pub adapter_id: Evidence<AdapterId>,
    pub kind: SourceKind,
    pub source_name: Text,
    /// Upstream software version, distinct from its data format/schema version.
    pub source_version: Evidence<Text>,
    pub source_schema_version: Text,
    pub adapter_name: Text,
    pub adapter_version: Text,
    pub parser_version: Text,
    pub driver_version: Evidence<Text>,
    pub os_version: Evidence<Text>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    Ghz2_4,
    Ghz5,
    Ghz6,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelContext {
    pub band: Evidence<Band>,
    pub primary_channel: Evidence<std::num::NonZeroU16>,
    pub primary_frequency: Evidence<Hertz>,
    pub center_frequency: Evidence<Hertz>,
    /// Second segment for non-contiguous operation, not an invented center.
    pub second_center_frequency: Evidence<Hertz>,
    pub width: Evidence<Megahertz>,
    /// Bit i marks punctured 20 MHz segment i, low frequency first.
    pub puncturing: Evidence<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DwellContext {
    pub schedule_id: Evidence<ChannelScheduleId>,
    pub cycle_index: Evidence<u64>,
    pub tuned_channel: ChannelContext,
    pub window: Evidence<MonotonicWindow>,
    pub reported_duration: Evidence<Seconds>,
    pub method_version: Text,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadioIdentityEvidence {
    pub physical_device: Evidence<PhysicalDeviceId>,
    pub radio: Evidence<RadioId>,
    pub bss: Evidence<BssId>,
    pub bssid: Evidence<MacAddress>,
    pub ess: Evidence<EssId>,
    pub mld: Evidence<MldId>,
    pub link_id: Evidence<u8>,
    pub client: Evidence<ClientId>,
    /// Inferred grouping must be pinned and independently auditable.
    pub grouping_evidence: Evidence<ArtifactReference>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CalibrationState {
    Uncalibrated,
    Reference { id: CalibrationId, version: Text },
    OutsideValidRange { id: CalibrationId, version: Text },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainSignal {
    pub chain_index: u8,
    pub rssi_dbm: Evidence<Dbm>,
    pub noise_dbm: Evidence<Dbm>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalReading {
    /// Values exactly as reported in dBm; no silent calibration or averaging.
    pub rssi_dbm: Evidence<Dbm>,
    pub noise_dbm: Evidence<Dbm>,
    pub chains: Vec<ChainSignal>,
    pub calibration: Evidence<CalibrationState>,
    pub measurement_method: Text,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScanObservation {
    pub identity: RadioIdentityEvidence,
    pub ssid: Evidence<Ssid>,
    pub signal: SignalReading,
    pub information_elements: Evidence<ArtifactReference>,
    /// Source APIs may report cached results. Age unknown is not fresh.
    pub result_age: Evidence<Seconds>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameMetadata {
    pub identity: RadioIdentityEvidence,
    pub signal: SignalReading,
    pub frame_type: Evidence<u8>,
    pub frame_subtype: Evidence<u8>,
    pub retry: Evidence<bool>,
    pub length_bytes: u32,
    pub phy_rate_mbps: Evidence<Mbps>,
    pub raw_information_elements: Evidence<ArtifactReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityFlag {
    Throttled,
    Hopping,
    Stale,
    InferredTimestamp,
    Malformed,
    Saturated,
    DroppedEvents,
    Disconnected,
    PartialCapture,
    ContradictorySourceFields,
    UnknownCalibration,
    ClockUncertain,
    SyntheticFixture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureHealth {
    pub dropped_events: Evidence<u64>,
    pub queued_events: Evidence<u64>,
    pub connected: Evidence<bool>,
    pub diagnostic: Text,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ObservationPayload {
    Scan(ScanObservation),
    Frame(FrameMetadata),
    Health(CaptureHealth),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierPolicy {
    ProjectPseudonymized,
    OwnedInfrastructure,
    ExplicitResearchConsent,
    Redacted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PayloadRetention {
    Discarded,
    Retained {
        authorization_reference: Text,
        retention_deadline: UtcTimestamp,
    },
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyState {
    pub policy_version: Text,
    pub identifiers: IdentifierPolicy,
    pub payload: PayloadRetention,
}

/// Serializable staging fields. Construct an ObservationEnvelope to validate
/// cross-field invariants before admitting this data to canonical storage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeData {
    pub schema_version: ObservationSchemaVersion,
    pub id: ObservationId,
    pub session_id: SessionId,
    pub source: SourceDescriptor,
    pub time: CaptureTime,
    pub pose: Evidence<PoseReference>,
    pub channel: Evidence<ChannelContext>,
    pub dwell: Evidence<DwellContext>,
    pub privacy: PrivacyState,
    pub quality: Vec<QualityFlag>,
    pub raw_source: Evidence<ArtifactReference>,
    pub payload: ObservationPayload,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ObservationEnvelope(EnvelopeData);
impl ObservationEnvelope {
    pub fn new(data: EnvelopeData) -> Result<Self, ValidationError> {
        if data.quality.len() > 32 {
            return Err(ValidationError::ResourceLimit("quality flags"));
        }
        if matches!(data.source.kind, SourceKind::SyntheticFixture)
            && !data.quality.contains(&QualityFlag::SyntheticFixture)
        {
            return Err(ValidationError::Inconsistent("fixture must be labeled"));
        }
        if let (Evidence::Known(time), Evidence::Known(model)) =
            (&data.time.monotonic, &data.time.synchronization)
            && time.epoch != model.epoch
        {
            return Err(ValidationError::ClockEpochMismatch);
        }
        if let (Evidence::Known(time), Evidence::Known(dwell)) = (&data.time.monotonic, &data.dwell)
            && let Evidence::Known(window) = dwell.window
            && !window.contains(*time)
        {
            return Err(ValidationError::Inconsistent("capture outside dwell"));
        }
        let signal = match &data.payload {
            ObservationPayload::Scan(scan) => Some(&scan.signal),
            ObservationPayload::Frame(frame) => {
                if matches!(frame.frame_type, Evidence::Known(t) if t > 3)
                    || matches!(frame.frame_subtype, Evidence::Known(t) if t > 15)
                {
                    return Err(ValidationError::OutOfRange("frame type/subtype"));
                }
                Some(&frame.signal)
            }
            ObservationPayload::Health(_) => None,
        };
        if let Some(signal) = signal {
            if signal.chains.len() > 16 {
                return Err(ValidationError::ResourceLimit("signal chains"));
            }
            let mut indices = std::collections::BTreeSet::new();
            if signal.chains.iter().any(|c| !indices.insert(c.chain_index)) {
                return Err(ValidationError::Inconsistent("duplicate signal chain"));
            }
        }
        Ok(Self(data))
    }
    pub const fn data(&self) -> &EnvelopeData {
        &self.0
    }
    pub fn into_data(self) -> EnvelopeData {
        self.0
    }
}
impl TryFrom<EnvelopeData> for ObservationEnvelope {
    type Error = ValidationError;
    fn try_from(data: EnvelopeData) -> Result<Self, Self::Error> {
        Self::new(data)
    }
}
impl From<ObservationEnvelope> for EnvelopeData {
    fn from(e: ObservationEnvelope) -> Self {
        e.0
    }
}
