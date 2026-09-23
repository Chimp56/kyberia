use sha2::{Digest, Sha256};

use crate::{
    CANONICAL_MANIFEST_FORMAT, Capability, DataContract, HostDescriptor, MANIFEST_SCHEMA,
    MANIFEST_SCHEMA_VERSION, NegotiatedPlugin, PluginKind, PluginManifest, PluginReference,
    ResourceLimits,
};

const MAX_PLUGIN_ID_BYTES: usize = 253;
/// Maximum number of nested JSON object/array delimiters in a raw manifest.
/// The root object counts as one.
pub const MAX_MANIFEST_NESTING_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContractError {
    InvalidManifest(&'static str),
    UnsupportedManifestSchema(u16),
    InvalidPluginId,
    InvalidDigest,
    InvalidVersionRange(&'static str),
    InvalidCapabilitySet,
    InvalidContractPair,
    DuplicateHostOffer(&'static str),
    IncompatibleApi,
    IncompatibleCapability(Capability),
    IncompatibleContract(DataContract),
    ResourceLimit(&'static str),
    ComponentMismatch,
    DuplicatePlugin(String),
    InvalidReference,
    Encoding,
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(field) => write!(formatter, "invalid manifest field: {field}"),
            Self::UnsupportedManifestSchema(version) => {
                write!(formatter, "unsupported manifest schema version: {version}")
            }
            Self::InvalidPluginId => formatter.write_str("plugin ID is not canonical reverse-DNS"),
            Self::InvalidDigest => formatter.write_str("component digest is not lowercase SHA-256"),
            Self::InvalidVersionRange(field) => write!(formatter, "invalid version range: {field}"),
            Self::InvalidCapabilitySet => {
                formatter.write_str("capabilities do not match plugin kind")
            }
            Self::InvalidContractPair => {
                formatter.write_str("input/output contracts do not match plugin kind")
            }
            Self::DuplicateHostOffer(kind) => write!(formatter, "duplicate host {kind} offer"),
            Self::IncompatibleApi => {
                formatter.write_str("host API version is outside the declared range")
            }
            Self::IncompatibleCapability(capability) => {
                write!(
                    formatter,
                    "host does not satisfy capability {}",
                    capability.id()
                )
            }
            Self::IncompatibleContract(contract) => {
                write!(
                    formatter,
                    "host does not satisfy data contract {}",
                    contract.id()
                )
            }
            Self::ResourceLimit(name) => {
                write!(formatter, "requested resource exceeds host limit: {name}")
            }
            Self::ComponentMismatch => {
                formatter.write_str("component bytes do not match the manifest identity")
            }
            Self::DuplicatePlugin(plugin_id) => {
                write!(formatter, "duplicate plugin ID: {plugin_id}")
            }
            Self::InvalidReference => {
                formatter.write_str("plugin reference does not match the manifest")
            }
            Self::Encoding => {
                formatter.write_str("manifest encoding is invalid or cannot be canonicalized")
            }
        }
    }
}

impl std::error::Error for ContractError {}

/// Parse and validate untrusted manifest bytes against a host's advertised
/// contract. The raw-byte length is checked before Serde constructs the typed
/// manifest. Callers must also bound transport/file reads before buffering
/// bytes; this check is not a runtime sandbox or a general heap quota.
pub fn parse_and_validate_manifest(
    bytes: &[u8],
    host: &HostDescriptor,
) -> Result<crate::ValidatedPluginDeclaration, ContractError> {
    validate_host(host)?;
    if bytes.len() as u64 > host.max_manifest_bytes {
        return Err(ContractError::ResourceLimit("manifest_bytes"));
    }
    check_manifest_nesting_depth(bytes)?;
    let manifest =
        serde_json::from_slice::<PluginManifest>(bytes).map_err(|_| ContractError::Encoding)?;
    let negotiated = validate_manifest(&manifest, host)?;
    Ok(crate::ValidatedPluginDeclaration {
        manifest,
        negotiated,
    })
}

/// Enforce the SDK's depth ceiling without allocating parser or typed-model
/// state. Invalid JSON syntax is still reported by Serde after this preflight.
fn check_manifest_nesting_depth(bytes: &[u8]) -> Result<(), ContractError> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_MANIFEST_NESTING_DEPTH {
                    return Err(ContractError::ResourceLimit("manifest_depth"));
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    Ok(())
}

/// Validate an already-deserialized manifest. For untrusted serialized input,
/// use [`parse_and_validate_manifest`] so the raw byte limit is checked before
/// Serde allocates the typed representation.
pub fn validate_manifest(
    manifest: &PluginManifest,
    host: &HostDescriptor,
) -> Result<NegotiatedPlugin, ContractError> {
    validate_host(host)?;
    validate_manifest_shape(manifest, host.max_manifest_bytes)?;

    if !manifest.host_api.contains(host.api_version) {
        return Err(ContractError::IncompatibleApi);
    }

    let mut capabilities = Vec::with_capacity(manifest.capabilities.len());
    for requirement in &manifest.capabilities {
        let offered = host
            .capabilities
            .iter()
            .find(|offer| offer.capability == requirement.capability)
            .filter(|offer| requirement.versions.contains(offer.version));
        let Some(offered) = offered else {
            return Err(ContractError::IncompatibleCapability(
                requirement.capability,
            ));
        };
        capabilities.push(offered.clone());
    }

    let mut negotiated_contract_versions = Vec::with_capacity(2);
    for requirement in [&manifest.input, &manifest.output] {
        let offered = host
            .contracts
            .iter()
            .find(|offer| offer.contract == requirement.contract)
            .filter(|offer| requirement.versions.contains(offer.version));
        let Some(offered) = offered else {
            return Err(ContractError::IncompatibleContract(requirement.contract));
        };
        negotiated_contract_versions.push(offered.version);
    }

    validate_requested_resources(&manifest.requested_resources, &host.resource_limits)?;
    let project_reference = plugin_reference(manifest)?;
    Ok(NegotiatedPlugin {
        project_reference,
        kind: manifest.kind,
        runtime: manifest.runtime,
        capabilities,
        input: manifest.input.contract,
        input_version: negotiated_contract_versions[0],
        output: manifest.output.contract,
        output_version: negotiated_contract_versions[1],
        requested_resources: manifest.requested_resources.clone(),
    })
}

pub fn verify_component(manifest: &PluginManifest, bytes: &[u8]) -> Result<(), ContractError> {
    if bytes.len() as u64 != manifest.component_size_bytes {
        return Err(ContractError::ComponentMismatch);
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    if digest != manifest.component_sha256 {
        return Err(ContractError::ComponentMismatch);
    }
    Ok(())
}

pub fn canonical_manifest_bytes(manifest: &PluginManifest) -> Result<Vec<u8>, ContractError> {
    validate_manifest_shape(manifest, u64::MAX)?;
    serde_json::to_vec(manifest).map_err(|_| ContractError::Encoding)
}

pub fn plugin_reference(manifest: &PluginManifest) -> Result<PluginReference, ContractError> {
    let canonical = canonical_manifest_bytes(manifest)?;
    let mut hasher = Sha256::new();
    hasher.update(CANONICAL_MANIFEST_FORMAT.as_bytes());
    hasher.update([0]);
    hasher.update((canonical.len() as u64).to_be_bytes());
    hasher.update(canonical);
    Ok(PluginReference {
        plugin_id: manifest.plugin_id.clone(),
        plugin_version: manifest.plugin_version,
        component_sha256: manifest.component_sha256.clone(),
        component_size_bytes: manifest.component_size_bytes,
        manifest_schema_version: manifest.schema_version,
        canonical_manifest_format: CANONICAL_MANIFEST_FORMAT.to_owned(),
        canonical_manifest_sha256: format!("{:x}", hasher.finalize()),
    })
}

pub fn verify_reference(
    manifest: &PluginManifest,
    reference: &PluginReference,
) -> Result<(), ContractError> {
    if plugin_reference(manifest)? == *reference {
        Ok(())
    } else {
        Err(ContractError::InvalidReference)
    }
}

pub(crate) fn validate_manifest_shape(
    manifest: &PluginManifest,
    max_manifest_bytes: u64,
) -> Result<(), ContractError> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(ContractError::InvalidManifest("schema"));
    }
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(ContractError::UnsupportedManifestSchema(
            manifest.schema_version,
        ));
    }
    if !is_canonical_plugin_id(&manifest.plugin_id) {
        return Err(ContractError::InvalidPluginId);
    }
    if !is_lower_hex_digest(&manifest.component_sha256) || manifest.component_size_bytes == 0 {
        return Err(ContractError::InvalidDigest);
    }
    if !manifest.host_api.is_valid() {
        return Err(ContractError::InvalidVersionRange("host_api"));
    }
    if !manifest.input.versions.is_valid() {
        return Err(ContractError::InvalidVersionRange("input"));
    }
    if !manifest.output.versions.is_valid() {
        return Err(ContractError::InvalidVersionRange("output"));
    }

    let (input, output, capabilities) = role_contracts(manifest.kind);
    if manifest.input.contract != input || manifest.output.contract != output {
        return Err(ContractError::InvalidContractPair);
    }
    if manifest.capabilities.len() != capabilities.len()
        || manifest
            .capabilities
            .iter()
            .map(|requirement| requirement.capability)
            .ne(capabilities.iter().copied())
    {
        return Err(ContractError::InvalidCapabilitySet);
    }
    for requirement in &manifest.capabilities {
        if !requirement.versions.is_valid() {
            return Err(ContractError::InvalidVersionRange(
                requirement.capability.id(),
            ));
        }
    }
    validate_positive_limits(&manifest.requested_resources)?;

    let bytes = serde_json::to_vec(manifest).map_err(|_| ContractError::Encoding)?;
    if bytes.len() as u64 > max_manifest_bytes {
        return Err(ContractError::ResourceLimit("manifest_bytes"));
    }
    Ok(())
}

fn role_contracts(kind: PluginKind) -> (DataContract, DataContract, &'static [Capability]) {
    match kind {
        PluginKind::Collector => (
            DataContract::CaptureBatch,
            DataContract::ObservationBatch,
            &[Capability::CaptureEventsRead, Capability::ObservationsEmit],
        ),
        PluginKind::Metric => (
            DataContract::MetricInput,
            DataContract::DerivedLayer,
            &[
                Capability::DerivedLayersEmit,
                Capability::GeometryRead,
                Capability::ObservationsRead,
            ],
        ),
        PluginKind::Export => (
            DataContract::ExportView,
            DataContract::ExportArtifact,
            &[Capability::ExportsCreate, Capability::ProjectRead],
        ),
    }
}

pub(crate) fn validate_host(host: &HostDescriptor) -> Result<(), ContractError> {
    validate_positive_limits(&host.resource_limits)?;
    if host.max_manifest_bytes == 0 || host.max_plugins == 0 {
        return Err(ContractError::ResourceLimit("host_limits"));
    }
    let mut capabilities = host
        .capabilities
        .iter()
        .map(|offer| offer.capability)
        .collect::<Vec<_>>();
    capabilities.sort_unstable();
    if capabilities.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ContractError::DuplicateHostOffer("capability"));
    }
    let mut contracts = host
        .contracts
        .iter()
        .map(|offer| offer.contract)
        .collect::<Vec<_>>();
    contracts.sort_unstable();
    if contracts.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ContractError::DuplicateHostOffer("contract"));
    }
    Ok(())
}

fn validate_requested_resources(
    requested: &ResourceLimits,
    supported: &ResourceLimits,
) -> Result<(), ContractError> {
    validate_positive_limits(requested)?;
    for (name, request, maximum) in [
        (
            "input_bytes",
            requested.max_input_bytes,
            supported.max_input_bytes,
        ),
        (
            "output_bytes",
            requested.max_output_bytes,
            supported.max_output_bytes,
        ),
        ("records", requested.max_records, supported.max_records),
        (
            "memory_bytes",
            requested.max_memory_bytes,
            supported.max_memory_bytes,
        ),
        ("fuel", requested.max_fuel, supported.max_fuel),
        (
            "wall_time_ms",
            requested.max_wall_time_ms,
            supported.max_wall_time_ms,
        ),
    ] {
        if request > maximum {
            return Err(ContractError::ResourceLimit(name));
        }
    }
    Ok(())
}

fn validate_positive_limits(limits: &ResourceLimits) -> Result<(), ContractError> {
    if limits.max_input_bytes == 0 {
        return Err(ContractError::ResourceLimit("input_bytes"));
    }
    if limits.max_output_bytes == 0 {
        return Err(ContractError::ResourceLimit("output_bytes"));
    }
    if limits.max_records == 0 {
        return Err(ContractError::ResourceLimit("records"));
    }
    if limits.max_memory_bytes == 0 {
        return Err(ContractError::ResourceLimit("memory_bytes"));
    }
    if limits.max_fuel == 0 {
        return Err(ContractError::ResourceLimit("fuel"));
    }
    if limits.max_wall_time_ms == 0 {
        return Err(ContractError::ResourceLimit("wall_time_ms"));
    }
    Ok(())
}

fn is_canonical_plugin_id(value: &str) -> bool {
    if value.len() > MAX_PLUGIN_ID_BYTES || !value.is_ascii() {
        return false;
    }
    let labels = value.split('.').collect::<Vec<_>>();
    labels.len() >= 2
        && labels.iter().all(|label| {
            let bytes = label.as_bytes();
            if bytes.is_empty() || bytes.len() > 63 {
                return false;
            }
            let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
            alphanumeric(bytes[0])
                && alphanumeric(bytes[bytes.len() - 1])
                && bytes
                    .iter()
                    .all(|byte| alphanumeric(*byte) || *byte == b'-')
        })
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
