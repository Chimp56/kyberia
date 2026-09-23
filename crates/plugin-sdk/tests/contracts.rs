use kyberia_plugin_sdk::{
    Capability, CapabilityOffer, CapabilityRequirement, CompatibilityRange, ContractError,
    ContractOffer, ContractRequirement, DataContract, HostDescriptor, MANIFEST_SCHEMA,
    MANIFEST_SCHEMA_VERSION, PluginKind, PluginManifest, PluginRegistry, PluginRuntime,
    ResourceLimits, SemanticVersion, canonical_manifest_bytes, plugin_reference, validate_manifest,
    verify_component, verify_reference,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn version(major: u16, minor: u16, patch: u16) -> SemanticVersion {
    SemanticVersion::new(major, minor, patch)
}

fn range(minimum: SemanticVersion, maximum: SemanticVersion) -> CompatibilityRange {
    CompatibilityRange {
        minimum_inclusive: minimum,
        maximum_exclusive: maximum,
    }
}

fn limits() -> ResourceLimits {
    ResourceLimits {
        max_input_bytes: 1024,
        max_output_bytes: 2048,
        max_records: 64,
        max_memory_bytes: 8 * 1024 * 1024,
        max_fuel: 100_000,
        max_wall_time_ms: 500,
    }
}

fn role(kind: PluginKind) -> (DataContract, DataContract, Vec<Capability>) {
    match kind {
        PluginKind::Collector => (
            DataContract::CaptureBatch,
            DataContract::ObservationBatch,
            vec![Capability::CaptureEventsRead, Capability::ObservationsEmit],
        ),
        PluginKind::Metric => (
            DataContract::MetricInput,
            DataContract::DerivedLayer,
            vec![
                Capability::DerivedLayersEmit,
                Capability::GeometryRead,
                Capability::ObservationsRead,
            ],
        ),
        PluginKind::Export => (
            DataContract::ExportView,
            DataContract::ExportArtifact,
            vec![Capability::ExportsCreate, Capability::ProjectRead],
        ),
    }
}

fn host() -> HostDescriptor {
    let capabilities = [
        Capability::CaptureEventsRead,
        Capability::DerivedLayersEmit,
        Capability::ExportsCreate,
        Capability::GeometryRead,
        Capability::ObservationsEmit,
        Capability::ObservationsRead,
        Capability::ProjectRead,
    ]
    .into_iter()
    .map(|capability| CapabilityOffer {
        capability,
        version: version(1, 0, 0),
    })
    .collect();
    let contracts = [
        DataContract::CaptureBatch,
        DataContract::DerivedLayer,
        DataContract::ExportArtifact,
        DataContract::ExportView,
        DataContract::MetricInput,
        DataContract::ObservationBatch,
    ]
    .into_iter()
    .map(|contract| ContractOffer {
        contract,
        version: version(1, 0, 0),
    })
    .collect();
    HostDescriptor {
        api_version: version(1, 2, 0),
        capabilities,
        contracts,
        resource_limits: ResourceLimits {
            max_input_bytes: 4096,
            max_output_bytes: 4096,
            max_records: 256,
            max_memory_bytes: 16 * 1024 * 1024,
            max_fuel: 1_000_000,
            max_wall_time_ms: 2000,
        },
        max_manifest_bytes: 64 * 1024,
        max_plugins: 16,
    }
}

fn manifest(kind: PluginKind, name: &str) -> PluginManifest {
    let component = b"component fixture";
    let (input, output, capabilities) = role(kind);
    let compatible = range(version(1, 0, 0), version(2, 0, 0));
    PluginManifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        schema_version: MANIFEST_SCHEMA_VERSION,
        plugin_id: format!("org.example.{name}"),
        plugin_version: version(1, 0, 0),
        kind,
        runtime: PluginRuntime::WasmComponent,
        component_sha256: format!("{:x}", Sha256::digest(component)),
        component_size_bytes: component.len() as u64,
        host_api: compatible.clone(),
        input: ContractRequirement {
            contract: input,
            versions: compatible.clone(),
        },
        output: ContractRequirement {
            contract: output,
            versions: compatible.clone(),
        },
        capabilities: capabilities
            .into_iter()
            .map(|capability| CapabilityRequirement {
                capability,
                versions: compatible.clone(),
            })
            .collect(),
        requested_resources: limits(),
    }
}

#[test]
fn collector_metric_and_export_negotiate_distinct_contracts() {
    for kind in [
        PluginKind::Collector,
        PluginKind::Metric,
        PluginKind::Export,
    ] {
        let admitted = validate_manifest(&manifest(kind, "sample"), &host()).unwrap();
        assert_eq!(admitted.kind, kind);
        assert_eq!(admitted.runtime, PluginRuntime::WasmComponent);
        assert_eq!(
            admitted
                .capabilities
                .iter()
                .map(|offer| offer.capability)
                .collect::<Vec<_>>(),
            role(kind).2
        );
        assert_eq!(admitted.input, role(kind).0);
        assert_eq!(admitted.output, role(kind).1);
    }
}

#[test]
fn negotiation_records_the_host_versions_selected_within_manifest_ranges() {
    let mut host = host();
    for offer in &mut host.capabilities {
        if matches!(
            offer.capability,
            Capability::CaptureEventsRead | Capability::ObservationsEmit
        ) {
            offer.version = version(1, 4, 2);
        }
    }
    for offer in &mut host.contracts {
        match offer.contract {
            DataContract::CaptureBatch => offer.version = version(1, 3, 1),
            DataContract::ObservationBatch => offer.version = version(1, 8, 0),
            _ => {}
        }
    }

    let admitted = validate_manifest(&manifest(PluginKind::Collector, "collector"), &host).unwrap();
    assert!(
        admitted
            .capabilities
            .iter()
            .all(|offer| offer.version == version(1, 4, 2))
    );
    assert_eq!(admitted.input_version, version(1, 3, 1));
    assert_eq!(admitted.output_version, version(1, 8, 0));
}

#[test]
fn compatibility_ranges_are_half_open() {
    let compatible = range(version(1, 0, 0), version(2, 0, 0));
    assert!(compatible.contains(version(1, 0, 0)));
    assert!(compatible.contains(version(1, 99, 99)));
    assert!(!compatible.contains(version(2, 0, 0)));
    assert!(!range(version(2, 0, 0), version(1, 0, 0)).is_valid());
}

#[test]
fn kind_cannot_request_another_kind_contract_or_authority() {
    let mut collector = manifest(PluginKind::Collector, "collector");
    collector.output.contract = DataContract::DerivedLayer;
    assert_eq!(
        validate_manifest(&collector, &host()),
        Err(ContractError::InvalidContractPair)
    );

    let mut metric = manifest(PluginKind::Metric, "metric");
    metric.capabilities.pop();
    assert_eq!(
        validate_manifest(&metric, &host()),
        Err(ContractError::InvalidCapabilitySet)
    );

    let mut exporter = manifest(PluginKind::Export, "exporter");
    exporter.capabilities[0].capability = Capability::CaptureEventsRead;
    assert_eq!(
        validate_manifest(&exporter, &host()),
        Err(ContractError::InvalidCapabilitySet)
    );
}

#[test]
fn host_api_capability_and_data_contract_versions_are_negotiated_independently() {
    let collector = manifest(PluginKind::Collector, "collector");
    let mut api_host = host();
    api_host.api_version = version(2, 0, 0);
    assert_eq!(
        validate_manifest(&collector, &api_host),
        Err(ContractError::IncompatibleApi)
    );

    let mut capability_host = host();
    capability_host.capabilities[0].version = version(2, 0, 0);
    assert!(matches!(
        validate_manifest(&collector, &capability_host),
        Err(ContractError::IncompatibleCapability(_))
    ));

    let mut contract_host = host();
    contract_host.contracts[0].version = version(2, 0, 0);
    assert!(matches!(
        validate_manifest(&collector, &contract_host),
        Err(ContractError::IncompatibleContract(_))
    ));
}

#[test]
fn component_bytes_must_match_manifest_digest_and_length() {
    let plugin = manifest(PluginKind::Collector, "collector");
    verify_component(&plugin, b"component fixture").unwrap();
    assert_eq!(
        verify_component(&plugin, b"different"),
        Err(ContractError::ComponentMismatch)
    );
    assert_eq!(
        verify_component(&plugin, b"component fixture!"),
        Err(ContractError::ComponentMismatch)
    );
}

#[test]
fn manifest_reference_binds_every_declared_field() {
    let plugin = manifest(PluginKind::Metric, "metric");
    let reference = plugin_reference(&plugin).unwrap();
    verify_reference(&plugin, &reference).unwrap();

    let mut changed = plugin.clone();
    changed.requested_resources.max_fuel += 1;
    assert_eq!(
        verify_reference(&changed, &reference),
        Err(ContractError::InvalidReference)
    );
}

#[test]
fn canonical_manifest_encoding_is_repeatable() {
    let plugin = manifest(PluginKind::Export, "exporter");
    assert_eq!(
        canonical_manifest_bytes(&plugin).unwrap(),
        canonical_manifest_bytes(&plugin).unwrap()
    );
    assert_eq!(
        plugin_reference(&plugin).unwrap(),
        plugin_reference(&plugin).unwrap()
    );
}

#[test]
fn plugin_ids_require_canonical_reverse_dns_labels() {
    for invalid in [
        "Example.org.plugin",
        "org..plugin",
        "org.-plugin",
        "org.plugin-",
        "org.pl_ugin",
        "org.é",
    ] {
        let mut plugin = manifest(PluginKind::Metric, "valid");
        plugin.plugin_id = invalid.to_owned();
        assert_eq!(
            validate_manifest(&plugin, &host()),
            Err(ContractError::InvalidPluginId),
            "{invalid}"
        );
    }
}

#[test]
fn malformed_digest_schema_and_resource_requests_fail_closed() {
    let mut plugin = manifest(PluginKind::Collector, "collector");
    plugin.component_sha256.make_ascii_uppercase();
    assert_eq!(
        validate_manifest(&plugin, &host()),
        Err(ContractError::InvalidDigest)
    );

    let mut plugin = manifest(PluginKind::Collector, "collector");
    plugin.schema_version += 1;
    assert_eq!(
        validate_manifest(&plugin, &host()),
        Err(ContractError::UnsupportedManifestSchema(2))
    );

    let mut plugin = manifest(PluginKind::Collector, "collector");
    plugin.requested_resources.max_memory_bytes = host().resource_limits.max_memory_bytes + 1;
    assert_eq!(
        validate_manifest(&plugin, &host()),
        Err(ContractError::ResourceLimit("memory_bytes"))
    );

    let mut plugin = manifest(PluginKind::Collector, "collector");
    plugin.requested_resources.max_output_bytes = 0;
    assert_eq!(
        validate_manifest(&plugin, &host()),
        Err(ContractError::ResourceLimit("output_bytes"))
    );
}

#[test]
fn host_duplicate_offers_and_manifest_size_are_rejected() {
    let plugin = manifest(PluginKind::Collector, "collector");
    let mut duplicate = host();
    duplicate
        .capabilities
        .push(duplicate.capabilities[0].clone());
    assert_eq!(
        validate_manifest(&plugin, &duplicate),
        Err(ContractError::DuplicateHostOffer("capability"))
    );

    let mut too_small = host();
    too_small.max_manifest_bytes = 8;
    assert_eq!(
        validate_manifest(&plugin, &too_small),
        Err(ContractError::ResourceLimit("manifest_bytes"))
    );
}

#[test]
fn registry_resolution_is_exact_and_order_independent() {
    let first = manifest(PluginKind::Collector, "collector");
    let second = manifest(PluginKind::Metric, "metric");
    let forward = PluginRegistry::build([first.clone(), second.clone()], &host()).unwrap();
    let reverse = PluginRegistry::build([second, first.clone()], &host()).unwrap();
    assert_eq!(forward.fingerprint_sha256(), reverse.fingerprint_sha256());

    let reference = plugin_reference(&first).unwrap();
    assert!(forward.resolve(&reference).is_some());
    let mut wrong_version = reference;
    wrong_version.plugin_version.patch += 1;
    assert!(forward.resolve(&wrong_version).is_none());
}

#[test]
fn registry_rejects_duplicate_ids_and_excess_count() {
    let first = manifest(PluginKind::Metric, "metric");
    assert!(matches!(
        PluginRegistry::build([first.clone(), first.clone()], &host()),
        Err(ContractError::DuplicatePlugin(_))
    ));

    let mut tiny_host = host();
    tiny_host.max_plugins = 1;
    let second = manifest(PluginKind::Export, "exporter");
    assert_eq!(
        PluginRegistry::build([first, second], &tiny_host),
        Err(ContractError::ResourceLimit("plugin_count"))
    );

    let mut invalid_host = host();
    invalid_host.max_plugins = 0;
    assert_eq!(
        PluginRegistry::build([], &invalid_host),
        Err(ContractError::ResourceLimit("host_limits"))
    );
}

#[test]
fn serde_rejects_unknown_manifest_fields_and_capabilities() {
    let mut json: Value = serde_json::to_value(manifest(PluginKind::Export, "exporter")).unwrap();
    json["unreviewed_authority"] = Value::Bool(true);
    assert!(serde_json::from_value::<PluginManifest>(json).is_err());

    let mut json: Value =
        serde_json::to_value(manifest(PluginKind::Collector, "collector")).unwrap();
    json["capabilities"][0]["capability"] = Value::String("filesystem.read_anything".to_owned());
    assert!(serde_json::from_value::<PluginManifest>(json).is_err());
}

#[test]
fn manifest_wire_ids_use_the_canonical_namespaced_contract_ids() {
    let expected = manifest(PluginKind::Collector, "collector");
    let encoded = serde_json::to_value(&expected).unwrap();
    assert_eq!(encoded["input"]["contract"], "rfatlas.capture-batch");
    assert_eq!(encoded["output"]["contract"], "rfatlas.observation-batch");
    assert_eq!(
        encoded["capabilities"][0]["capability"],
        "rfatlas.capture_events.read"
    );
    assert_eq!(
        serde_json::from_value::<PluginManifest>(encoded).unwrap(),
        expected
    );
}
