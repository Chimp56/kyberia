use crate::{Result, StoreError};
use kyberia_domain::identity::ProjectId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

/// Initial directory-container schema. Versioned observation formats remain
/// independent of this storage manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub project_id: ProjectId,
    pub name: String,
    pub revision: u64,
    pub created_utc_ms: i64,
    pub updated_utc_ms: i64,
    pub required_features: Vec<String>,
    pub artifacts: BTreeMap<String, ArtifactEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    MapSource,
    NormalizedObservations,
    RawCapture,
    AnalysisManifest,
    NumericalLayer,
    Annotation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactEntry {
    pub kind: ArtifactKind,
    pub bytes: u64,
    pub media_type: String,
    /// Source/provenance record ID supplied by the owning application use case.
    pub provenance_id: String,
}

pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn validate_text(text: &str, max: usize) -> Result<()> {
    if text.trim().is_empty() || text.len() > max || text.chars().any(char::is_control) {
        return Err(StoreError::Invalid(
            "empty, oversized or control-containing text".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StoreError::Invalid(
            "expected lowercase SHA-256 digest".into(),
        ));
    }
    Ok(())
}

impl ArtifactEntry {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.media_type, 255)?;
        validate_text(&self.provenance_id, 1024)?;
        if self.bytes > MAX_ARTIFACT_BYTES {
            return Err(StoreError::Invalid(
                "artifact exceeds 64 MiB chunk limit".into(),
            ));
        }
        Ok(())
    }
}

impl BundleManifest {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.name, 1024)?;
        if self.schema_version == 0
            || self.revision > i64::MAX as u64
            || self.created_utc_ms < 0
            || self.updated_utc_ms < self.created_utc_ms
        {
            return Err(StoreError::Invalid(
                "invalid schema or manifest timestamps".into(),
            ));
        }
        if self.artifacts.len() > 10_000 || self.required_features.len() > 128 {
            return Err(StoreError::Invalid(
                "manifest inventory exceeds resource limit".into(),
            ));
        }
        for feature in &self.required_features {
            validate_text(feature, 128)?;
        }
        for (hash, entry) in &self.artifacts {
            validate_hash(hash)?;
            entry.validate()?;
        }
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(StoreError::Invalid(
                "manifest exceeds resource limit".into(),
            ));
        }
        Ok(bytes)
    }
}
