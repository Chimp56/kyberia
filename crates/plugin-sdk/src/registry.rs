use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{
    ContractError, HostDescriptor, NegotiatedPlugin, PluginManifest, PluginReference,
    canonical_manifest_bytes, validate_host, validate_manifest,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredPlugin {
    pub manifest: PluginManifest,
    pub negotiated: NegotiatedPlugin,
}

/// Deterministic declaration registry. It does not load component bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginRegistry {
    entries: BTreeMap<String, RegisteredPlugin>,
    fingerprint_sha256: String,
}

impl PluginRegistry {
    pub fn build(
        manifests: impl IntoIterator<Item = PluginManifest>,
        host: &HostDescriptor,
    ) -> Result<Self, ContractError> {
        validate_host(host)?;
        let mut entries = BTreeMap::new();
        for (index, manifest) in manifests.into_iter().enumerate() {
            if index >= host.max_plugins {
                return Err(ContractError::ResourceLimit("plugin_count"));
            }
            let negotiated = validate_manifest(&manifest, host)?;
            let plugin_id = manifest.plugin_id.clone();
            if entries
                .insert(
                    plugin_id.clone(),
                    RegisteredPlugin {
                        manifest,
                        negotiated,
                    },
                )
                .is_some()
            {
                return Err(ContractError::DuplicatePlugin(plugin_id));
            }
        }

        let mut hasher = Sha256::new();
        hasher.update(b"rfatlas.plugin-registry-v1\0");
        for entry in entries.values() {
            let canonical = canonical_manifest_bytes(&entry.manifest)?;
            hasher.update((canonical.len() as u64).to_be_bytes());
            hasher.update(canonical);
        }
        Ok(Self {
            entries,
            fingerprint_sha256: format!("{:x}", hasher.finalize()),
        })
    }

    pub fn entries(&self) -> &BTreeMap<String, RegisteredPlugin> {
        &self.entries
    }

    pub fn fingerprint_sha256(&self) -> &str {
        &self.fingerprint_sha256
    }

    /// A project reference resolves only the exact manifest and component hash.
    pub fn resolve(&self, reference: &PluginReference) -> Option<&RegisteredPlugin> {
        self.entries
            .get(&reference.plugin_id)
            .filter(|entry| entry.negotiated.project_reference == *reference)
    }
}
