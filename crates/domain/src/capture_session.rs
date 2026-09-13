//! Durable provenance for one native acquisition session.
//!
//! This contract deliberately keeps foreign collector UUIDs beside, rather
//! than in place of, RF Atlas canonical identities.  The adapter/composition
//! layer proves that the foreign UUIDs and mapping rows came from one decoded
//! stream; this module validates the canonical record and its manifest/envelope
//! closure without importing an adapter type.

use crate::{
    ValidationError,
    capture::{
        CaptureManifest, CaptureTerminalStatus, MAX_CAPTURE_OBSERVATIONS, RawSourceDisposition,
    },
    evidence::{ArtifactReference, Evidence},
    identity::{
        AdapterId, BssId, ClockEpochId, CollectorId, ContentHash, ObservationId, RadioId, SensorId,
        SessionId, SourceId, Text,
    },
    observation::{ObservationEnvelope, ObservationPayload, PayloadRetention, PrivacyState},
};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Canonical session records are bounded independently from the capture
/// manifest so malformed mapping evidence cannot consume an unbounded buffer.
pub const MAX_CAPTURE_SESSION_BYTES: usize = 1024 * 1024;
pub const MAX_SESSION_SOURCE_MAPPINGS: usize = 4_164;
pub const MAX_SESSION_OBSERVATION_MAPPINGS: usize = MAX_CAPTURE_OBSERVATIONS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureSessionSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingEvidenceSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

/// A UUID emitted by the foreign collector.  It is checked and preserved
/// byte-for-byte; it is not one of the domain's canonical 128-bit IDs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct NativeUuid(String);

impl NativeUuid {
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.len() != 36
            || !value.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            })
            || !value.bytes().any(|byte| byte != b'0' && byte != b'-')
        {
            return Err(ValidationError::InvalidText);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NativeUuid {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<NativeUuid> for String {
    fn from(value: NativeUuid) -> Self {
        value.0
    }
}

impl<'de> Deserialize<'de> for NativeUuid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMappingEvidenceV1 {
    source_key: Text,
    source_id: SourceId,
    sensor_id: Evidence<SensorId>,
    adapter_id: Evidence<AdapterId>,
}

impl SourceMappingEvidenceV1 {
    pub fn new(
        source_key: Text,
        source_id: SourceId,
        sensor_id: Evidence<SensorId>,
        adapter_id: Evidence<AdapterId>,
    ) -> Self {
        Self {
            source_key,
            source_id,
            sensor_id,
            adapter_id,
        }
    }

    pub fn source_key(&self) -> &Text {
        &self.source_key
    }

    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }

    pub const fn sensor_id(&self) -> &Evidence<SensorId> {
        &self.sensor_id
    }

    pub const fn adapter_id(&self) -> &Evidence<AdapterId> {
        &self.adapter_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationMappingEvidenceV1 {
    observation_key: Text,
    observation_id: ObservationId,
    transmitter_radio: Evidence<RadioId>,
    transmitter_bss: Evidence<BssId>,
    identity_evidence: Evidence<ArtifactReference>,
}

impl ObservationMappingEvidenceV1 {
    pub fn new(
        observation_key: Text,
        observation_id: ObservationId,
        transmitter_radio: Evidence<RadioId>,
        transmitter_bss: Evidence<BssId>,
        identity_evidence: Evidence<ArtifactReference>,
    ) -> Self {
        Self {
            observation_key,
            observation_id,
            transmitter_radio,
            transmitter_bss,
            identity_evidence,
        }
    }

    pub fn observation_key(&self) -> &Text {
        &self.observation_key
    }

    pub const fn observation_id(&self) -> ObservationId {
        self.observation_id
    }

    pub const fn transmitter_radio(&self) -> &Evidence<RadioId> {
        &self.transmitter_radio
    }

    pub const fn transmitter_bss(&self) -> &Evidence<BssId> {
        &self.transmitter_bss
    }

    pub const fn identity_evidence(&self) -> &Evidence<ArtifactReference> {
        &self.identity_evidence
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MappingEvidenceV1 {
    schema_version: MappingEvidenceSchemaVersion,
    registry_version: Text,
    source_mappings: Vec<SourceMappingEvidenceV1>,
    observation_mappings: Vec<ObservationMappingEvidenceV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MappingEvidenceWire {
    schema_version: MappingEvidenceSchemaVersion,
    registry_version: Text,
    source_mappings: Vec<SourceMappingEvidenceV1>,
    observation_mappings: Vec<ObservationMappingEvidenceV1>,
}

impl<'de> Deserialize<'de> for MappingEvidenceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = MappingEvidenceWire::deserialize(deserializer)?;
        if wire.schema_version != MappingEvidenceSchemaVersion::V1 {
            return Err(serde::de::Error::custom(
                "unsupported mapping evidence schema",
            ));
        }
        let mut source_mappings = wire.source_mappings;
        let mut observation_mappings = wire.observation_mappings;
        source_mappings.sort_by(|left, right| left.source_key.cmp(&right.source_key));
        observation_mappings
            .sort_by(|left, right| left.observation_key.cmp(&right.observation_key));
        let mapping = Self {
            schema_version: wire.schema_version,
            registry_version: wire.registry_version,
            source_mappings,
            observation_mappings,
        };
        mapping.validate_shape().map_err(serde::de::Error::custom)?;
        Ok(mapping)
    }
}

impl MappingEvidenceV1 {
    pub fn new(
        registry_version: Text,
        mut source_mappings: Vec<SourceMappingEvidenceV1>,
        mut observation_mappings: Vec<ObservationMappingEvidenceV1>,
    ) -> Result<Self, ValidationError> {
        if source_mappings.len() > MAX_SESSION_SOURCE_MAPPINGS {
            return Err(ValidationError::ResourceLimit("session source mappings"));
        }
        if observation_mappings.len() > MAX_SESSION_OBSERVATION_MAPPINGS {
            return Err(ValidationError::ResourceLimit(
                "session observation mappings",
            ));
        }
        source_mappings.sort_by(|left, right| left.source_key.cmp(&right.source_key));
        observation_mappings
            .sort_by(|left, right| left.observation_key.cmp(&right.observation_key));
        let mapping = Self {
            schema_version: MappingEvidenceSchemaVersion::V1,
            registry_version,
            source_mappings,
            observation_mappings,
        };
        mapping.validate_shape()?;
        Ok(mapping)
    }

    fn validate_shape(&self) -> Result<(), ValidationError> {
        if self.schema_version != MappingEvidenceSchemaVersion::V1 {
            return Err(ValidationError::UnsupportedSchema);
        }
        if self.source_mappings.len() > MAX_SESSION_SOURCE_MAPPINGS
            || self.observation_mappings.len() > MAX_SESSION_OBSERVATION_MAPPINGS
        {
            return Err(ValidationError::ResourceLimit("session mappings"));
        }
        let mut source_keys = BTreeSet::new();
        let mut source_ids = BTreeSet::new();
        for mapping in &self.source_mappings {
            if !source_keys.insert(mapping.source_key.as_str())
                || !source_ids.insert(mapping.source_id)
            {
                return Err(ValidationError::Inconsistent(
                    "duplicate session source mapping",
                ));
            }
        }
        let mut observation_keys = BTreeSet::new();
        let mut observation_ids = BTreeSet::new();
        for mapping in &self.observation_mappings {
            if !observation_keys.insert(mapping.observation_key.as_str())
                || !observation_ids.insert(mapping.observation_id)
            {
                return Err(ValidationError::Inconsistent(
                    "duplicate session observation mapping",
                ));
            }
            if (matches!(mapping.transmitter_radio, Evidence::Known(_))
                || matches!(mapping.transmitter_bss, Evidence::Known(_)))
                && !matches!(mapping.identity_evidence, Evidence::Known(_))
            {
                return Err(ValidationError::Inconsistent(
                    "known transmitter requires identity evidence",
                ));
            }
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> MappingEvidenceSchemaVersion {
        self.schema_version
    }

    pub fn registry_version(&self) -> &Text {
        &self.registry_version
    }

    pub fn source_mappings(&self) -> &[SourceMappingEvidenceV1] {
        &self.source_mappings
    }

    pub fn observation_mappings(&self) -> &[ObservationMappingEvidenceV1] {
        &self.observation_mappings
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSessionRecordV1 {
    schema_version: CaptureSessionSchemaVersion,
    session_id: SessionId,
    collector_id: CollectorId,
    clock_epoch_id: ClockEpochId,
    process_session_uuid: NativeUuid,
    source_clock_uuid: NativeUuid,
    manifest_hash: ContentHash,
    mapping: MappingEvidenceV1,
    privacy: PrivacyState,
    terminal: CaptureTerminalStatus,
    terminal_reason: Text,
    partial: bool,
    observation_count: u16,
    exit_code: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureSessionRecordWire {
    schema_version: CaptureSessionSchemaVersion,
    session_id: SessionId,
    collector_id: CollectorId,
    clock_epoch_id: ClockEpochId,
    process_session_uuid: NativeUuid,
    source_clock_uuid: NativeUuid,
    manifest_hash: ContentHash,
    mapping: MappingEvidenceV1,
    privacy: PrivacyState,
    terminal: CaptureTerminalStatus,
    terminal_reason: Text,
    partial: bool,
    observation_count: u16,
    exit_code: i32,
}

impl<'de> Deserialize<'de> for CaptureSessionRecordV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CaptureSessionRecordWire::deserialize(deserializer)?;
        Self::from_parts(
            wire.schema_version,
            wire.session_id,
            wire.collector_id,
            wire.clock_epoch_id,
            wire.process_session_uuid,
            wire.source_clock_uuid,
            wire.manifest_hash,
            wire.mapping,
            wire.privacy,
            wire.terminal,
            wire.terminal_reason,
            wire.partial,
            wire.observation_count,
            wire.exit_code,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn validate_terminal_exit(
    terminal: CaptureTerminalStatus,
    partial: bool,
    observation_count: u16,
    exit_code: i32,
) -> Result<(), ValidationError> {
    let exit_matches = match terminal {
        CaptureTerminalStatus::Ok => exit_code == 0,
        CaptureTerminalStatus::Partial => exit_code == 2,
        CaptureTerminalStatus::PermissionRequired => exit_code == 77,
        CaptureTerminalStatus::Unsupported | CaptureTerminalStatus::Unavailable => exit_code == 69,
        CaptureTerminalStatus::Error => exit_code == 70,
        CaptureTerminalStatus::Timeout => exit_code == 124,
        CaptureTerminalStatus::Cancelled => matches!(exit_code, 130 | 143),
    };
    if !exit_matches {
        return Err(ValidationError::Inconsistent(
            "capture session terminal and exit code",
        ));
    }
    if partial != (terminal != CaptureTerminalStatus::Ok && observation_count > 0) {
        return Err(ValidationError::Inconsistent(
            "capture session partial terminal flag",
        ));
    }
    Ok(())
}

impl CaptureSessionRecordV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: SessionId,
        collector_id: CollectorId,
        clock_epoch_id: ClockEpochId,
        process_session_uuid: NativeUuid,
        source_clock_uuid: NativeUuid,
        manifest_hash: ContentHash,
        mapping: MappingEvidenceV1,
        privacy: PrivacyState,
        terminal: CaptureTerminalStatus,
        terminal_reason: Text,
        partial: bool,
        observation_count: u16,
        exit_code: i32,
    ) -> Result<Self, ValidationError> {
        Self::from_parts(
            CaptureSessionSchemaVersion::V1,
            session_id,
            collector_id,
            clock_epoch_id,
            process_session_uuid,
            source_clock_uuid,
            manifest_hash,
            mapping,
            privacy,
            terminal,
            terminal_reason,
            partial,
            observation_count,
            exit_code,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        schema_version: CaptureSessionSchemaVersion,
        session_id: SessionId,
        collector_id: CollectorId,
        clock_epoch_id: ClockEpochId,
        process_session_uuid: NativeUuid,
        source_clock_uuid: NativeUuid,
        manifest_hash: ContentHash,
        mapping: MappingEvidenceV1,
        privacy: PrivacyState,
        terminal: CaptureTerminalStatus,
        terminal_reason: Text,
        partial: bool,
        observation_count: u16,
        exit_code: i32,
    ) -> Result<Self, ValidationError> {
        if schema_version != CaptureSessionSchemaVersion::V1 {
            return Err(ValidationError::UnsupportedSchema);
        }
        if usize::from(observation_count) > MAX_CAPTURE_OBSERVATIONS {
            return Err(ValidationError::ResourceLimit("session observations"));
        }
        validate_terminal_exit(terminal, partial, observation_count, exit_code)?;
        mapping.validate_shape()?;
        Ok(Self {
            schema_version,
            session_id,
            collector_id,
            clock_epoch_id,
            process_session_uuid,
            source_clock_uuid,
            manifest_hash,
            mapping,
            privacy,
            terminal,
            terminal_reason,
            partial,
            observation_count,
            exit_code,
        })
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_CAPTURE_SESSION_BYTES {
            return Err("capture session record exceeds resource limit".into());
        }
        let record: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("capture session decoding failed: {error}"))?;
        let canonical = record.canonical_bytes()?;
        if canonical != bytes {
            return Err("capture session bytes are not canonical".into());
        }
        Ok(record)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("capture session encoding failed: {error}"))?;
        if bytes.is_empty() || bytes.len() > MAX_CAPTURE_SESSION_BYTES {
            return Err("capture session record exceeds resource limit".into());
        }
        Ok(bytes)
    }

    /// Validate the record against the exact canonical manifest and envelopes
    /// that the outward adapter produced. Empty captures may still carry
    /// source mappings from a capabilities record, so only observation rows
    /// are required to equal the envelope set.
    pub fn validate_against_manifest(
        &self,
        manifest: &CaptureManifest,
        observations: &[ObservationEnvelope],
    ) -> Result<(), ValidationError> {
        let manifest_bytes = manifest
            .canonical_bytes()
            .map_err(|_| ValidationError::Inconsistent("capture manifest canonical bytes"))?;
        let manifest_hash = ContentHash::from_sha256(Sha256::digest(manifest_bytes).into());
        if self.manifest_hash != manifest_hash {
            return Err(ValidationError::Inconsistent(
                "capture session manifest hash",
            ));
        }
        if usize::from(self.observation_count) != observations.len()
            || self.observation_count != manifest.completion().observation_count()
            || manifest.observation_ids_in_source_order().len() != observations.len()
        {
            return Err(ValidationError::Inconsistent(
                "capture session observation count",
            ));
        }
        if self.terminal != manifest.completion().status()
            || self.terminal_reason != *manifest.completion().reason()
            || self.partial != manifest.completion().partial()
        {
            return Err(ValidationError::Inconsistent(
                "capture session terminal evidence",
            ));
        }
        if let Evidence::Known(capabilities) = manifest.capabilities()
            && capabilities.collector_id != self.collector_id
        {
            return Err(ValidationError::Inconsistent(
                "capture session capability collector",
            ));
        }
        let expected_raw_disposition = match self.privacy.payload {
            PayloadRetention::Retained { .. } => RawSourceDisposition::Retained,
            PayloadRetention::Discarded | PayloadRetention::NotApplicable => {
                RawSourceDisposition::NotRetained
            }
        };
        if manifest.raw_source_disposition() != expected_raw_disposition {
            return Err(ValidationError::Inconsistent(
                "capture session privacy retention",
            ));
        }

        let mut observation_ids = BTreeSet::new();
        let mut source_ids = BTreeSet::new();
        for (index, observation) in observations.iter().enumerate() {
            let data = observation.data();
            if data.id != manifest.observation_ids_in_source_order()[index]
                || !observation_ids.insert(data.id)
                || data.session_id != self.session_id
                || data.source.collector_id != self.collector_id
                || data.privacy != self.privacy
            {
                return Err(ValidationError::Inconsistent(
                    "capture session observation closure",
                ));
            }
            if let Evidence::Known(time) = &data.time.monotonic
                && time.epoch != self.clock_epoch_id
            {
                return Err(ValidationError::ClockEpochMismatch);
            }
            if let Evidence::Known(model) = &data.time.synchronization
                && model.epoch != self.clock_epoch_id
            {
                return Err(ValidationError::ClockEpochMismatch);
            }
            let source_mapping = self
                .mapping
                .source_mappings
                .iter()
                .find(|mapping| mapping.source_id == data.source.source_id)
                .ok_or(ValidationError::Inconsistent(
                    "capture session source mapping closure",
                ))?;
            if source_mapping.sensor_id != data.source.sensor_id
                || source_mapping.adapter_id != data.source.adapter_id
            {
                return Err(ValidationError::Inconsistent(
                    "capture session source mapping evidence",
                ));
            }
            let observation_mapping = self
                .mapping
                .observation_mappings
                .iter()
                .find(|mapping| mapping.observation_id == data.id)
                .ok_or(ValidationError::Inconsistent(
                    "capture session observation mapping closure",
                ))?;
            match &data.payload {
                ObservationPayload::Scan(scan) => {
                    if observation_mapping.transmitter_radio != scan.identity.radio
                        || observation_mapping.transmitter_bss != scan.identity.bss
                        || observation_mapping.identity_evidence != scan.identity.grouping_evidence
                    {
                        return Err(ValidationError::Inconsistent(
                            "capture session observation mapping evidence",
                        ));
                    }
                }
                ObservationPayload::Frame(frame) => {
                    if observation_mapping.transmitter_radio != frame.identity.radio
                        || observation_mapping.transmitter_bss != frame.identity.bss
                        || observation_mapping.identity_evidence != frame.identity.grouping_evidence
                    {
                        return Err(ValidationError::Inconsistent(
                            "capture session observation mapping evidence",
                        ));
                    }
                }
                ObservationPayload::Health(_) => {
                    if matches!(observation_mapping.transmitter_radio, Evidence::Known(_))
                        || matches!(observation_mapping.transmitter_bss, Evidence::Known(_))
                        || matches!(observation_mapping.identity_evidence, Evidence::Known(_))
                    {
                        return Err(ValidationError::Inconsistent(
                            "capture session health mapping evidence",
                        ));
                    }
                }
            }
            if let Evidence::Known(reference) = &data.raw_source
                && !manifest
                    .source_records()
                    .iter()
                    .any(|source| source.reference() == reference)
            {
                return Err(ValidationError::Inconsistent(
                    "capture session raw source reference closure",
                ));
            }
            source_ids.insert(data.source.source_id);
        }
        let mapped_observation_ids: BTreeSet<_> = self
            .mapping
            .observation_mappings
            .iter()
            .map(ObservationMappingEvidenceV1::observation_id)
            .collect();
        if mapped_observation_ids != observation_ids {
            return Err(ValidationError::Inconsistent(
                "capture session observation mapping closure",
            ));
        }
        let mapped_source_ids: BTreeSet<_> = self
            .mapping
            .source_mappings
            .iter()
            .map(SourceMappingEvidenceV1::source_id)
            .collect();
        if !source_ids.is_subset(&mapped_source_ids) {
            return Err(ValidationError::Inconsistent(
                "capture session source mapping closure",
            ));
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> CaptureSessionSchemaVersion {
        self.schema_version
    }

    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub const fn collector_id(&self) -> CollectorId {
        self.collector_id
    }

    pub const fn clock_epoch_id(&self) -> ClockEpochId {
        self.clock_epoch_id
    }

    pub fn process_session_uuid(&self) -> &NativeUuid {
        &self.process_session_uuid
    }

    pub fn source_clock_uuid(&self) -> &NativeUuid {
        &self.source_clock_uuid
    }

    pub const fn manifest_hash(&self) -> ContentHash {
        self.manifest_hash
    }

    /// Replace the manifest hash after the record has been checked with a
    /// fixed-width placeholder. Content hashes always have the same
    /// canonical representation, so this lets an outer composition boundary
    /// perform its bounded serialization check without cloning the mapping.
    pub fn with_manifest_hash(mut self, manifest_hash: ContentHash) -> Self {
        self.manifest_hash = manifest_hash;
        self
    }

    pub const fn mapping(&self) -> &MappingEvidenceV1 {
        &self.mapping
    }

    pub const fn privacy(&self) -> &PrivacyState {
        &self.privacy
    }

    pub const fn terminal(&self) -> CaptureTerminalStatus {
        self.terminal
    }

    pub const fn terminal_reason(&self) -> &Text {
        &self.terminal_reason
    }

    pub const fn partial(&self) -> bool {
        self.partial
    }

    pub const fn observation_count(&self) -> u16 {
        self.observation_count
    }

    pub const fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability::{CapabilityDocument, RawPayloadPolicy},
        capture::{CaptureCompletion, SourceRecordManifest},
        evidence::{SchemaVersion, UnknownReason},
        observation::{EnvelopeData, IdentifierPolicy},
        time::{CaptureTime, ClockModel, MonotonicTimestamp, UtcTimestamp},
    };
    use std::collections::BTreeMap;

    fn id(byte: u8) -> SessionId {
        SessionId::from_bytes([byte; 16]).unwrap()
    }

    fn privacy() -> PrivacyState {
        PrivacyState {
            policy_version: Text::new("privacy/v1").unwrap(),
            identifiers: IdentifierPolicy::Redacted,
            payload: crate::observation::PayloadRetention::Discarded,
        }
    }

    fn manifest(collector: CollectorId) -> CaptureManifest {
        let completion = crate::capture::CaptureCompletion::new(
            CaptureTerminalStatus::PermissionRequired,
            Text::new("permission required").unwrap(),
            false,
            0,
        )
        .unwrap();
        CaptureManifest::new(
            SchemaVersion::V1,
            Text::new(crate::capture::CAPTURE_MANIFEST_METHOD_VERSION).unwrap(),
            crate::observation::SourceKind::NativeApi,
            ContentHash::from_sha256([9; 32]),
            Evidence::Known(CapabilityDocument {
                schema_version: SchemaVersion::V1,
                collector_id: collector,
                collector_version: Text::new("test/v1").unwrap(),
                probed_at: CaptureTime {
                    wall: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    monotonic: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    synchronization: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                },
                entries: BTreeMap::new(),
                raw_payload_policy: RawPayloadPolicy::Discard,
            }),
            completion,
            vec![],
            crate::capture::RawSourceDisposition::NotRetained,
            vec![],
        )
        .unwrap()
    }

    fn empty_record(manifest: &CaptureManifest) -> CaptureSessionRecordV1 {
        let bytes = manifest.canonical_bytes().unwrap();
        let hash = ContentHash::from_sha256(Sha256::digest(bytes).into());
        CaptureSessionRecordV1::new(
            id(1),
            CollectorId::from_bytes([2; 16]).unwrap(),
            ClockEpochId::from_bytes([3; 16]).unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000001").unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000002").unwrap(),
            hash,
            MappingEvidenceV1::new(Text::new("registry/v1").unwrap(), vec![], vec![]).unwrap(),
            privacy(),
            CaptureTerminalStatus::PermissionRequired,
            Text::new("permission required").unwrap(),
            false,
            0,
            77,
        )
        .unwrap()
    }

    fn populated_fixture() -> (CaptureSessionRecordV1, CaptureManifest, ObservationEnvelope) {
        let wire: serde_json::Value = serde_json::from_str(include_str!(
            "../../capture-adapter/tests/fixtures/macos-valid-canonical.json"
        ))
        .unwrap();
        let envelope: ObservationEnvelope =
            serde_json::from_value(wire["envelope"].clone()).unwrap();
        let data = envelope.data();
        let raw_reference = match &data.raw_source {
            Evidence::Known(reference) => reference.clone(),
            Evidence::Unknown(_) => panic!("fixture must retain a raw reference"),
        };
        let (radio, bss, grouping_evidence) = match &data.payload {
            ObservationPayload::Scan(scan) => (
                scan.identity.radio.clone(),
                scan.identity.bss.clone(),
                scan.identity.grouping_evidence.clone(),
            ),
            ObservationPayload::Frame(frame) => (
                frame.identity.radio.clone(),
                frame.identity.bss.clone(),
                frame.identity.grouping_evidence.clone(),
            ),
            ObservationPayload::Health(_) => panic!("fixture must carry a radio payload"),
        };
        let completion = CaptureCompletion::new(
            CaptureTerminalStatus::PermissionRequired,
            Text::new("permission required").unwrap(),
            true,
            1,
        )
        .unwrap();
        let manifest = CaptureManifest::new(
            SchemaVersion::V1,
            Text::new(crate::capture::CAPTURE_MANIFEST_METHOD_VERSION).unwrap(),
            data.source.kind.clone(),
            ContentHash::from_sha256([9; 32]),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            completion,
            vec![SourceRecordManifest::new(raw_reference)],
            RawSourceDisposition::NotRetained,
            vec![data.id],
        )
        .unwrap();
        let manifest_hash =
            ContentHash::from_sha256(Sha256::digest(manifest.canonical_bytes().unwrap()).into());
        let mapping = MappingEvidenceV1::new(
            Text::new("registry/v1").unwrap(),
            vec![SourceMappingEvidenceV1::new(
                Text::new("source").unwrap(),
                data.source.source_id,
                data.source.sensor_id.clone(),
                data.source.adapter_id.clone(),
            )],
            vec![ObservationMappingEvidenceV1::new(
                Text::new("observation").unwrap(),
                data.id,
                radio,
                bss,
                grouping_evidence,
            )],
        )
        .unwrap();
        let record = CaptureSessionRecordV1::new(
            data.session_id,
            data.source.collector_id,
            ClockEpochId::from_bytes([3; 16]).unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000001").unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000002").unwrap(),
            manifest_hash,
            mapping,
            data.privacy.clone(),
            CaptureTerminalStatus::PermissionRequired,
            Text::new("permission required").unwrap(),
            true,
            1,
            77,
        )
        .unwrap();
        (record, manifest, envelope)
    }

    #[test]
    fn empty_terminal_preserves_canonical_ids_and_foreign_evidence() {
        let collector = CollectorId::from_bytes([2; 16]).unwrap();
        let manifest = manifest(collector);
        let record = empty_record(&manifest);
        record.validate_against_manifest(&manifest, &[]).unwrap();
        let bytes = record.canonical_bytes().unwrap();
        let decoded = CaptureSessionRecordV1::from_canonical_bytes(&bytes).unwrap();
        assert_eq!(decoded.session_id(), id(1));
        assert_eq!(decoded.collector_id(), collector);
        assert_eq!(
            decoded.clock_epoch_id(),
            ClockEpochId::from_bytes([3; 16]).unwrap()
        );
        assert_eq!(
            decoded.process_session_uuid().as_str(),
            "00000000-0000-4000-8000-000000000001"
        );
        assert_eq!(decoded.observation_count(), 0);
    }

    #[test]
    fn source_mapping_is_allowed_for_empty_capability_only_capture() {
        let collector = CollectorId::from_bytes([2; 16]).unwrap();
        let manifest = manifest(collector);
        let source = SourceMappingEvidenceV1::new(
            Text::new("receiver-1:en0").unwrap(),
            SourceId::from_bytes([4; 16]).unwrap(),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        );
        let bytes = manifest.canonical_bytes().unwrap();
        let record = CaptureSessionRecordV1::new(
            id(1),
            collector,
            ClockEpochId::from_bytes([3; 16]).unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000001").unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000002").unwrap(),
            ContentHash::from_sha256(Sha256::digest(bytes).into()),
            MappingEvidenceV1::new(Text::new("registry/v1").unwrap(), vec![source], vec![])
                .unwrap(),
            privacy(),
            CaptureTerminalStatus::PermissionRequired,
            Text::new("permission required").unwrap(),
            false,
            0,
            77,
        )
        .unwrap();
        record.validate_against_manifest(&manifest, &[]).unwrap();
        assert_eq!(record.mapping().source_mappings().len(), 1);
    }

    #[test]
    fn native_uuid_rejects_uppercase_zero_and_malformed_values() {
        for value in [
            "00000000-0000-0000-0000-000000000000",
            "00000000-0000-4000-8000-00000000000A",
            "not-a-uuid",
        ] {
            assert!(NativeUuid::new(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn terminal_and_exit_code_are_checked_before_manifest_binding() {
        let collector = CollectorId::from_bytes([2; 16]).unwrap();
        let manifest = manifest(collector);
        let bytes = manifest.canonical_bytes().unwrap();
        let result = CaptureSessionRecordV1::new(
            id(1),
            collector,
            ClockEpochId::from_bytes([3; 16]).unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000001").unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000002").unwrap(),
            ContentHash::from_sha256(Sha256::digest(bytes).into()),
            MappingEvidenceV1::new(Text::new("registry/v1").unwrap(), vec![], vec![]).unwrap(),
            privacy(),
            CaptureTerminalStatus::PermissionRequired,
            Text::new("permission required").unwrap(),
            false,
            0,
            0,
        );
        assert!(result.is_err());

        let partial_ok = CaptureSessionRecordV1::new(
            id(1),
            collector,
            ClockEpochId::from_bytes([3; 16]).unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000001").unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000002").unwrap(),
            ContentHash::from_sha256(Sha256::digest(manifest.canonical_bytes().unwrap()).into()),
            MappingEvidenceV1::new(Text::new("registry/v1").unwrap(), vec![], vec![]).unwrap(),
            privacy(),
            CaptureTerminalStatus::Ok,
            Text::new("completed").unwrap(),
            true,
            0,
            0,
        );
        assert!(partial_ok.is_err());

        let partial_empty = CaptureSessionRecordV1::new(
            id(1),
            collector,
            ClockEpochId::from_bytes([3; 16]).unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000001").unwrap(),
            NativeUuid::new("00000000-0000-4000-8000-000000000002").unwrap(),
            ContentHash::from_sha256(Sha256::digest(manifest.canonical_bytes().unwrap()).into()),
            MappingEvidenceV1::new(Text::new("registry/v1").unwrap(), vec![], vec![]).unwrap(),
            privacy(),
            CaptureTerminalStatus::Partial,
            Text::new("partial").unwrap(),
            false,
            0,
            2,
        );
        assert!(partial_empty.is_ok());
    }

    #[test]
    fn manifest_binding_rejects_contradictory_clock_mapping_and_raw_evidence() {
        let (record, manifest, envelope) = populated_fixture();
        record
            .validate_against_manifest(&manifest, std::slice::from_ref(&envelope))
            .unwrap();

        let reject = |label: &str, data: EnvelopeData| {
            let candidate = ObservationEnvelope::new(data).unwrap();
            let result = record.validate_against_manifest(&manifest, &[candidate]);
            assert!(result.is_err(), "accepted contradictory {label}");
        };

        let mut data = envelope.data().clone();
        data.time.monotonic = Evidence::Known(MonotonicTimestamp {
            epoch: ClockEpochId::from_bytes([48; 16]).unwrap(),
            nanoseconds: 1,
        });
        reject("clock epoch", data);

        let mut data = envelope.data().clone();
        data.time.synchronization = Evidence::Known(ClockModel {
            epoch: ClockEpochId::from_bytes([49; 16]).unwrap(),
            reference_monotonic_nanoseconds: 1,
            reference_utc: UtcTimestamp(0),
            offset_to_reference: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            drift: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            error: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            method_version: Text::new("clock/v1").unwrap(),
        });
        reject("clock model epoch", data);

        let mut data = envelope.data().clone();
        data.source.sensor_id = Evidence::Known(SensorId::from_bytes([41; 16]).unwrap());
        reject("sensor mapping", data);

        let mut data = envelope.data().clone();
        data.source.adapter_id = Evidence::Known(AdapterId::from_bytes([42; 16]).unwrap());
        reject("adapter mapping", data);

        let mut data = envelope.data().clone();
        let ObservationPayload::Scan(scan) = &mut data.payload else {
            panic!("fixture must carry a scan payload")
        };
        scan.identity.radio = Evidence::Known(RadioId::from_bytes([43; 16]).unwrap());
        reject("radio mapping", data);

        let mut data = envelope.data().clone();
        let ObservationPayload::Scan(scan) = &mut data.payload else {
            panic!("fixture must carry a scan payload")
        };
        scan.identity.bss = Evidence::Known(BssId::from_bytes([44; 16]).unwrap());
        reject("BSS mapping", data);

        let mut data = envelope.data().clone();
        let ObservationPayload::Scan(scan) = &mut data.payload else {
            panic!("fixture must carry a scan payload")
        };
        scan.identity.grouping_evidence = Evidence::Known(ArtifactReference {
            sha256: ContentHash::from_sha256([45; 32]),
            media_type: Text::new("application/octet-stream").unwrap(),
            byte_length: 45,
        });
        reject("grouping evidence", data);

        let mut data = envelope.data().clone();
        data.raw_source = Evidence::Known(ArtifactReference {
            sha256: ContentHash::from_sha256([46; 32]),
            media_type: Text::new("application/octet-stream").unwrap(),
            byte_length: 46,
        });
        reject("raw source membership", data);

        let original_reference = match &envelope.data().raw_source {
            Evidence::Known(reference) => reference,
            Evidence::Unknown(_) => panic!("fixture must retain a raw reference"),
        };
        let mut data = envelope.data().clone();
        data.raw_source = Evidence::Known(ArtifactReference {
            sha256: original_reference.sha256,
            media_type: Text::new("application/octet-stream").unwrap(),
            byte_length: original_reference.byte_length,
        });
        reject("raw source reference equality", data);
    }

    #[test]
    fn mapping_rejects_duplicate_keys_ids_and_unproved_transmitter() {
        let source_id = SourceId::from_bytes([4; 16]).unwrap();
        let source = || {
            SourceMappingEvidenceV1::new(
                Text::new("receiver").unwrap(),
                source_id,
                Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            )
        };
        assert!(
            MappingEvidenceV1::new(
                Text::new("registry/v1").unwrap(),
                vec![source(), source()],
                vec![]
            )
            .is_err()
        );
        let observation_id = ObservationId::from_bytes([5; 16]).unwrap();
        let observation = || {
            ObservationMappingEvidenceV1::new(
                Text::new("observation").unwrap(),
                observation_id,
                Evidence::Known(RadioId::from_bytes([6; 16]).unwrap()),
                Evidence::Unknown(UnknownReason::NotAdvertised),
                Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            )
        };
        assert!(
            MappingEvidenceV1::new(
                Text::new("registry/v1").unwrap(),
                vec![],
                vec![observation()]
            )
            .is_err()
        );
    }

    #[test]
    fn mapping_limits_reject_oversized_source_evidence() {
        let mappings = (0..=MAX_SESSION_SOURCE_MAPPINGS)
            .map(|index| {
                let mut id_bytes = [0; 16];
                id_bytes[..8].copy_from_slice(&(u64::try_from(index + 1).unwrap()).to_be_bytes());
                SourceMappingEvidenceV1::new(
                    Text::new(format!("receiver-{index}")).unwrap(),
                    SourceId::from_bytes(id_bytes).unwrap(),
                    Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                )
            })
            .collect();
        assert!(
            MappingEvidenceV1::new(Text::new("registry/v1").unwrap(), mappings, vec![]).is_err()
        );
    }

    #[test]
    fn canonical_mapping_order_is_independent_of_input_order() {
        let source_id = SourceId::from_bytes([4; 16]).unwrap();
        let a = SourceMappingEvidenceV1::new(
            Text::new("a").unwrap(),
            source_id,
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        );
        let b = SourceMappingEvidenceV1::new(
            Text::new("b").unwrap(),
            SourceId::from_bytes([5; 16]).unwrap(),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        );
        let left = MappingEvidenceV1::new(
            Text::new("registry/v1").unwrap(),
            vec![b.clone(), a.clone()],
            vec![],
        )
        .unwrap();
        let right =
            MappingEvidenceV1::new(Text::new("registry/v1").unwrap(), vec![a, b], vec![]).unwrap();
        assert_eq!(left, right);
        assert_eq!(left.source_mappings()[0].source_key().as_str(), "a");
    }

    #[test]
    fn manifest_hash_and_terminal_mismatch_are_rejected() {
        let collector = CollectorId::from_bytes([2; 16]).unwrap();
        let manifest = manifest(collector);
        let mut record = empty_record(&manifest);
        record.manifest_hash = ContentHash::from_sha256([8; 32]);
        assert!(record.validate_against_manifest(&manifest, &[]).is_err());
    }
}
