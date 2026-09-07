//! Capabilities are runtime evidence. An absent entry resolves to unknown.
use crate::{
    ValidationError,
    evidence::{Evidence, SchemaVersion, UnknownReason},
    identity::{CollectorId, Text},
    time::CaptureTime,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    NearbyScan,
    CurrentLink,
    MonitorFrames,
    ChannelControl,
    ChannelHopping,
    Radiotap,
    NoiseDbm,
    PerChainSignal,
    Fcs,
    RetryFlag,
    PhyMetadata,
    ConcurrentManagedMonitor,
    ActiveProbes,
    SpectrumSweep,
    Gps,
    Pose,
    Band2Ghz,
    Band5Ghz,
    Band6Ghz,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CapabilityState {
    Available {
        evidence: Text,
    },
    Conditional {
        condition: Text,
        evidence: Text,
    },
    Unavailable {
        reason: UnknownReason,
        remediation: Text,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityDocument {
    pub schema_version: SchemaVersion,
    pub collector_id: CollectorId,
    pub collector_version: Text,
    pub probed_at: CaptureTime,
    pub entries: BTreeMap<Capability, CapabilityState>,
    pub raw_payload_policy: RawPayloadPolicy,
}
impl CapabilityDocument {
    pub fn capability(&self, capability: &Capability) -> Evidence<&CapabilityState> {
        match self.entries.get(capability) {
            Some(value) => Evidence::Known(value),
            None => Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        }
    }
    pub fn require_available(&self, required: &[Capability]) -> Result<(), ValidationError> {
        for capability in required {
            if !matches!(
                self.entries.get(capability),
                Some(CapabilityState::Available { .. })
            ) {
                return Err(ValidationError::Inconsistent(
                    "required capability not available",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RawPayloadPolicy {
    Discard,
    ExplicitRetention {
        authorization_reference: Text,
        max_bytes: std::num::NonZeroU64,
    },
}
