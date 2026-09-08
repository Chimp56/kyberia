//! Canonical capture-publication metadata shared by composition and storage.
//!
//! The wire contract stays in the domain crate so the project store can
//! validate capture manifests without depending on a collector or the
//! composition layer.  Raw source bytes and adapter DTOs remain outside this
//! contract.

use crate::{
    ValidationError,
    capability::CapabilityDocument,
    evidence::{ArtifactReference, Evidence, SchemaVersion},
    identity::{ContentHash, ObservationId, Text},
    observation::SourceKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_CAPTURE_OBSERVATIONS: usize = 4_096;
pub const MAX_CAPTURE_SOURCE_RECORDS: usize = 4_164;
pub const MAX_CAPTURE_SOURCE_RECORD_BYTES: u64 = 16_384;
pub const MAX_CAPTURE_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
pub const CAPTURE_MANIFEST_METHOD_VERSION: &str = "native-observation-pipeline/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTerminalStatus {
    Ok,
    Partial,
    PermissionRequired,
    Unsupported,
    Unavailable,
    Error,
    Timeout,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureCompletion {
    status: CaptureTerminalStatus,
    reason: Text,
    partial: bool,
    observation_count: u16,
}

impl CaptureCompletion {
    pub fn new(
        status: CaptureTerminalStatus,
        reason: Text,
        partial: bool,
        observation_count: u16,
    ) -> Result<Self, ValidationError> {
        if usize::from(observation_count) > MAX_CAPTURE_OBSERVATIONS {
            return Err(ValidationError::ResourceLimit("capture observations"));
        }
        Ok(Self {
            status,
            reason,
            partial,
            observation_count,
        })
    }

    pub const fn status(&self) -> CaptureTerminalStatus {
        self.status
    }

    pub fn reason(&self) -> &Text {
        &self.reason
    }

    pub const fn partial(&self) -> bool {
        self.partial
    }

    pub const fn observation_count(&self) -> u16 {
        self.observation_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecordManifest {
    reference: ArtifactReference,
}

impl SourceRecordManifest {
    pub const fn new(reference: ArtifactReference) -> Self {
        Self { reference }
    }

    pub const fn reference(&self) -> &ArtifactReference {
        &self.reference
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawSourceDisposition {
    NotRetained,
    Retained,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureManifest {
    schema_version: SchemaVersion,
    method_version: Text,
    evidence_origin: SourceKind,
    collector_build: ContentHash,
    capabilities: Evidence<CapabilityDocument>,
    completion: CaptureCompletion,
    source_records: Vec<SourceRecordManifest>,
    raw_source_disposition: RawSourceDisposition,
    /// IDs in source stream order; the chunk adapter separately sorts rows by
    /// canonical observation ID for deterministic columnar bytes.
    observation_ids_in_source_order: Vec<ObservationId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureManifestWire {
    schema_version: SchemaVersion,
    method_version: Text,
    evidence_origin: SourceKind,
    collector_build: ContentHash,
    capabilities: Evidence<CapabilityDocument>,
    completion: CaptureCompletion,
    source_records: Vec<SourceRecordManifest>,
    raw_source_disposition: RawSourceDisposition,
    observation_ids_in_source_order: Vec<ObservationId>,
}

impl<'de> Deserialize<'de> for CaptureManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = CaptureManifestWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.method_version,
            wire.evidence_origin,
            wire.collector_build,
            wire.capabilities,
            wire.completion,
            wire.source_records,
            wire.raw_source_disposition,
            wire.observation_ids_in_source_order,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl CaptureManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: SchemaVersion,
        method_version: Text,
        evidence_origin: SourceKind,
        collector_build: ContentHash,
        capabilities: Evidence<CapabilityDocument>,
        completion: CaptureCompletion,
        source_records: Vec<SourceRecordManifest>,
        raw_source_disposition: RawSourceDisposition,
        observation_ids_in_source_order: Vec<ObservationId>,
    ) -> Result<Self, ValidationError> {
        let manifest = Self {
            schema_version,
            method_version,
            evidence_origin,
            collector_build,
            capabilities,
            completion,
            source_records,
            raw_source_disposition,
            observation_ids_in_source_order,
        };
        manifest.validate_shape()?;
        Ok(manifest)
    }

    fn validate_shape(&self) -> Result<(), ValidationError> {
        if self.schema_version != SchemaVersion::V1
            || self.method_version.as_str() != CAPTURE_MANIFEST_METHOD_VERSION
        {
            return Err(ValidationError::UnsupportedSchema);
        }
        if self.source_records.len() > MAX_CAPTURE_SOURCE_RECORDS {
            return Err(ValidationError::ResourceLimit("capture source records"));
        }
        if self.observation_ids_in_source_order.len() > MAX_CAPTURE_OBSERVATIONS {
            return Err(ValidationError::ResourceLimit("capture observations"));
        }
        if usize::from(self.completion.observation_count)
            != self.observation_ids_in_source_order.len()
        {
            return Err(ValidationError::Inconsistent(
                "capture completion observation count",
            ));
        }
        let mut source_hashes = BTreeSet::new();
        for source in &self.source_records {
            if source.reference.byte_length > MAX_CAPTURE_SOURCE_RECORD_BYTES
                || !source_hashes.insert(source.reference.sha256)
            {
                return Err(ValidationError::Inconsistent(
                    "capture source reference inventory",
                ));
            }
        }
        let mut observation_ids = BTreeSet::new();
        if self
            .observation_ids_in_source_order
            .iter()
            .any(|id| !observation_ids.insert(*id))
        {
            return Err(ValidationError::Inconsistent(
                "capture observation identity inventory",
            ));
        }
        Ok(())
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_CAPTURE_MANIFEST_BYTES {
            return Err("capture manifest exceeds resource limit".into());
        }
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("capture manifest decoding failed: {error}"))?;
        let canonical = manifest.canonical_bytes()?;
        if canonical != bytes {
            return Err("capture manifest bytes are not canonical".into());
        }
        Ok(manifest)
    }

    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub fn method_version(&self) -> &Text {
        &self.method_version
    }

    pub const fn evidence_origin(&self) -> &SourceKind {
        &self.evidence_origin
    }

    pub const fn collector_build(&self) -> ContentHash {
        self.collector_build
    }

    pub const fn capabilities(&self) -> &Evidence<CapabilityDocument> {
        &self.capabilities
    }

    pub const fn completion(&self) -> &CaptureCompletion {
        &self.completion
    }

    pub fn source_records(&self) -> &[SourceRecordManifest] {
        &self.source_records
    }

    pub const fn raw_source_disposition(&self) -> RawSourceDisposition {
        self.raw_source_disposition
    }

    pub fn observation_ids_in_source_order(&self) -> &[ObservationId] {
        &self.observation_ids_in_source_order
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate_shape()
            .map_err(|error| format!("invalid capture manifest: {error}"))?;
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("capture manifest encoding failed: {error}"))?;
        if bytes.len() > MAX_CAPTURE_MANIFEST_BYTES {
            return Err("capture manifest exceeds resource limit".into());
        }
        Ok(bytes)
    }

    pub fn terminal(&self) -> bool {
        self.completion.status != CaptureTerminalStatus::Ok
            || self.completion.partial
            || self.observation_ids_in_source_order.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability::{CapabilityDocument, RawPayloadPolicy},
        evidence::{Evidence, UnknownReason},
        identity::{CollectorId, ObservationId},
        time::CaptureTime,
    };
    use std::collections::BTreeMap;

    fn manifest() -> CaptureManifest {
        let collector_id = CollectorId::from_bytes([1; 16]).unwrap();
        CaptureManifest::new(
            SchemaVersion::V1,
            Text::new(CAPTURE_MANIFEST_METHOD_VERSION).unwrap(),
            SourceKind::NativeApi,
            ContentHash::from_sha256([2; 32]),
            Evidence::Known(CapabilityDocument {
                schema_version: SchemaVersion::V1,
                collector_id,
                collector_version: Text::new("test/v1").unwrap(),
                probed_at: CaptureTime {
                    wall: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    monotonic: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                    synchronization: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
                },
                entries: BTreeMap::new(),
                raw_payload_policy: RawPayloadPolicy::Discard,
            }),
            CaptureCompletion::new(
                CaptureTerminalStatus::Ok,
                Text::new("complete").unwrap(),
                false,
                1,
            )
            .unwrap(),
            vec![],
            RawSourceDisposition::NotRetained,
            vec![ObservationId::from_bytes([3; 16]).unwrap()],
        )
        .unwrap()
    }

    #[test]
    fn canonical_decode_rejects_duplicate_ids_and_unsupported_method() {
        let bytes = manifest().canonical_bytes().unwrap();
        let mut duplicate = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        let id = duplicate["observation_ids_in_source_order"][0].clone();
        duplicate["observation_ids_in_source_order"] = serde_json::json!([id.clone(), id]);
        duplicate["completion"]["observation_count"] = serde_json::json!(2);
        let duplicate_bytes = serde_json::to_vec(&duplicate).unwrap();
        assert!(CaptureManifest::from_canonical_bytes(&duplicate_bytes).is_err());

        let mut unsupported = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        unsupported["method_version"] = serde_json::json!("capture/v2");
        let unsupported_bytes = serde_json::to_vec(&unsupported).unwrap();
        assert!(CaptureManifest::from_canonical_bytes(&unsupported_bytes).is_err());
    }

    #[test]
    fn canonical_decode_rejects_unknown_fields_and_count_mismatch() {
        let bytes = manifest().canonical_bytes().unwrap();
        let mut unknown = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        unknown["future"] = serde_json::json!(true);
        assert!(
            CaptureManifest::from_canonical_bytes(&serde_json::to_vec(&unknown).unwrap()).is_err()
        );

        let mut mismatch = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        mismatch["completion"]["observation_count"] = serde_json::json!(0);
        assert!(
            CaptureManifest::from_canonical_bytes(&serde_json::to_vec(&mismatch).unwrap()).is_err()
        );
    }
}
