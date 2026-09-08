use kyberia_domain::{
    analysis::{ExactU64, VersionedArtifact},
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{ContentHash, FloorId, FrameId, ObservationId, Text},
    units::{CoordinateMeters, Dbm, Meters},
};
use kyberia_spatial_analysis::*;
use sha2::{Digest, Sha256};

fn artifact(definition: &MetricDefinition, bytes: &[u8]) -> VersionedArtifact {
    VersionedArtifact {
        version: definition.version().clone(),
        sha256: ContentHash::from_sha256(Sha256::digest(bytes).into()),
        byte_length: ExactU64::new(bytes.len() as u64),
        media_type: Text::new(METRIC_DEFINITION_MEDIA_TYPE).unwrap(),
    }
}

#[test]
fn builtin_is_complete_and_help_and_compute_share_identity() {
    let definition = MetricDefinition::signal_rssi().unwrap();
    assert_eq!(definition.id().as_str(), "wifi.rssi");
    assert_eq!(definition.revision().get(), 1);
    assert_eq!(definition.unit(), PhysicalUnit::Dbm);
    assert_eq!(definition.spatial_method(), SpatialMethod::PointValue);
    assert_eq!(definition.uncertainty(), UncertaintyMethod::NotReported);
    assert_eq!(definition.compliance(), ComplianceDirection::HigherIsBetter);
    assert_eq!(
        definition.compatibility(),
        &Compatibility::ExactEvidenceContract
    );

    let hash = definition.content_hash().unwrap();
    assert_eq!(definition.ui_help().unwrap().definition_hash, hash);
    assert_eq!(definition.compute_contract().unwrap().definition_hash, hash);
    assert_eq!(definition.ui_help().unwrap().unit, definition.unit());
    assert_eq!(
        definition.compute_contract().unwrap().unit,
        definition.unit()
    );
    assert_eq!(
        definition.compute_contract().unwrap().spatial_method,
        definition.spatial_method()
    );
    assert_eq!(
        definition.compute_contract().unwrap().unknown,
        UnknownCompatibility::PropagateReason
    );
}

#[test]
fn canonical_bytes_include_all_semantic_fields_and_round_trip() {
    let definition = MetricDefinition::signal_rssi().unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for field in [
        "id",
        "version",
        "semantic_description",
        "unit",
        "valid_range",
        "evidence_requirements",
        "aggregation",
        "spatial_method",
        "selection",
        "uncertainty",
        "unknown_compatibility",
        "compatibility",
        "visualization",
        "compliance",
    ] {
        assert!(json.get(field).is_some(), "missing field {field}");
    }
    assert_eq!(
        MetricDefinition::from_canonical_bytes(&bytes).unwrap(),
        definition
    );
    assert_eq!(
        serde_json::from_slice::<MetricDefinition>(&bytes).unwrap(),
        definition
    );

    let mut future = json.clone();
    future["schema"] = serde_json::json!("kyberia.metric-definition/2");
    assert!(matches!(
        MetricDefinition::from_canonical_bytes(&serde_json::to_vec(&future).unwrap()),
        Err(MetricDefinitionError::MalformedBytes)
    ));

    let mut unknown = json;
    unknown["untrusted"] = serde_json::json!(true);
    assert!(matches!(
        MetricDefinition::from_canonical_bytes(&serde_json::to_vec(&unknown).unwrap()),
        Err(MetricDefinitionError::MalformedBytes)
    ));

    let mut wrong_unit = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
    wrong_unit["unit"] = serde_json::json!("mbps");
    assert_eq!(
        MetricDefinition::from_canonical_bytes(&serde_json::to_vec(&wrong_unit).unwrap()),
        Err(MetricDefinitionError::InvalidConfiguration(
            "signal aggregation requires dBm",
        ))
    );
}

#[test]
fn registry_is_sorted_and_rejects_duplicate_versions() {
    let rssi = MetricDefinition::signal_rssi().unwrap();
    let signal = MetricDefinition::new(
        Text::new("wifi.signal/1").unwrap(),
        SignalAggregationSelection::new(AggregateMethod::MedianDbm),
    )
    .unwrap();
    let registry = MetricRegistry::new(vec![rssi.clone(), signal.clone()]).unwrap();
    let ids: Vec<_> = registry.list().map(|metric| metric.id().as_str()).collect();
    assert_eq!(ids, vec!["wifi.rssi", "wifi.signal"]);
    assert_eq!(
        registry.lookup(
            &MetricId::new("wifi.rssi").unwrap(),
            MetricVersion::new(1).unwrap()
        ),
        Some(&rssi)
    );
    assert_eq!(
        MetricRegistry::new(vec![rssi.clone(), rssi]),
        Err(RegistryError::DuplicateVersion)
    );
}

#[test]
fn typed_identifiers_and_ordered_collections_reject_invalid_wire_values() {
    for value in ["", "Upper", "_leading", "trailing_", "a//b", "a/"] {
        assert_eq!(
            MetricId::new(value),
            Err(MetricDefinitionError::InvalidIdentifier)
        );
    }
    assert_eq!(
        MetricId::new("a".repeat(129)),
        Err(MetricDefinitionError::InvalidIdentifier)
    );
    assert_eq!(
        MetricVersion::new(0),
        Err(MetricDefinitionError::InvalidVersion)
    );
    assert!(serde_json::from_str::<MetricVersion>("0").is_err());

    let definition = MetricDefinition::signal_rssi().unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let mut duplicate_capability: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    duplicate_capability["evidence_requirements"]["capabilities"][0]["capabilities"] =
        serde_json::json!(["monitor_frames", "monitor_frames"]);
    assert_eq!(
        MetricDefinition::from_canonical_bytes(&serde_json::to_vec(&duplicate_capability).unwrap()),
        Err(MetricDefinitionError::InvalidConfiguration(
            "duplicate capability"
        ))
    );

    let mut duplicate_evidence: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    duplicate_evidence["evidence_requirements"]["evidence"] =
        serde_json::json!(["observed", "observed"]);
    assert_eq!(
        MetricDefinition::from_canonical_bytes(&serde_json::to_vec(&duplicate_evidence).unwrap()),
        Err(MetricDefinitionError::InvalidConfiguration(
            "duplicate evidence class"
        ))
    );

    let mut unsorted_selection: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    unsorted_selection["selection"]["filters"] = serde_json::json!(["channel", "band"]);
    assert_eq!(
        MetricDefinition::from_canonical_bytes(&serde_json::to_vec(&unsorted_selection).unwrap()),
        Err(MetricDefinitionError::InvalidConfiguration(
            "selection dimensions must be sorted"
        ))
    );
}

#[test]
fn strict_ranges_sizes_and_layer_dimensions_are_bounded() {
    let definition = MetricDefinition::signal_rssi().unwrap();
    let mut invalid_range: serde_json::Value =
        serde_json::from_slice(&definition.canonical_bytes().unwrap()).unwrap();
    invalid_range["valid_range"]["minimum"] = serde_json::json!(101.0);
    assert_eq!(
        MetricDefinition::from_canonical_bytes(&serde_json::to_vec(&invalid_range).unwrap()),
        Err(MetricDefinitionError::InvalidRange)
    );
    assert_eq!(
        MetricDefinition::from_canonical_bytes(&vec![b' '; MAX_METRIC_DEFINITION_BYTES + 1]),
        Err(MetricDefinitionError::ResourceLimit(
            "metric definition bytes"
        ))
    );
    assert_eq!(
        check_layer_operation(
            LayerOperation::Minimum,
            PhysicalUnit::Dbm,
            Some(PhysicalUnit::Dbm)
        ),
        Ok(PhysicalUnit::Dbm)
    );
    assert_eq!(
        check_layer_operation(
            LayerOperation::Difference,
            PhysicalUnit::Dbm,
            Some(PhysicalUnit::Mbps)
        ),
        Err(DimensionError::IncompatibleUnits)
    );
    assert_eq!(
        check_layer_operation(
            LayerOperation::Difference,
            PhysicalUnit::Dbm,
            Some(PhysicalUnit::Dbm)
        ),
        Ok(PhysicalUnit::Db)
    );
    assert_eq!(
        check_layer_operation(LayerOperation::Rank, PhysicalUnit::Dbm, None),
        Ok(PhysicalUnit::Dimensionless)
    );
    assert!(matches!(
        check_layer_operation(
            LayerOperation::Difference,
            PhysicalUnit::Categorical,
            Some(PhysicalUnit::Categorical)
        ),
        Err(DimensionError::UnsupportedOperation(
            "categorical arithmetic"
        ))
    ));
}

#[test]
fn legacy_signal_artifacts_preserve_wire_bytes_and_hashes() {
    let selection = SignalAggregationSelection::new(AggregateMethod::MedianDbm);
    let legacy =
        SignalMetricDefinition::new(Text::new("legacy-label").unwrap(), selection).unwrap();
    let bytes = legacy.canonical_bytes().unwrap();
    assert_eq!(
        bytes,
        br#"{"schema":"kyberia.signal-metric-definition/1","version":"legacy-label","signal_aggregation":{"algorithm_version":"kyberia-wifi-signal/1","method":{"method":"median_dbm"}}}"#
    );
    let artifact = VersionedArtifact {
        version: legacy.version().clone(),
        sha256: ContentHash::from_sha256(Sha256::digest(&bytes).into()),
        byte_length: ExactU64::new(bytes.len() as u64),
        media_type: Text::new(SIGNAL_METRIC_DEFINITION_MEDIA_TYPE).unwrap(),
    };
    let binding =
        MetricDefinitionBinding::from_artifact_bytes(artifact.clone(), &bytes, selection).unwrap();
    assert_eq!(binding.artifact().media_type, artifact.media_type);
    assert_eq!(binding.canonical_bytes().unwrap(), bytes);
    assert_eq!(binding.definition_hash().unwrap(), artifact.sha256);
    assert_eq!(binding.signal_aggregation(), selection);
    assert_eq!(binding.spatial_method(), None);
    assert_eq!(binding.definition().version(), legacy.version());
}

#[test]
fn current_metric_spatial_method_is_authoritative() {
    let definition = MetricDefinition::signal_rssi().unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let binding = definition.bind(artifact(&definition, &bytes)).unwrap();
    let floor_id = FloorId::from_bytes([3; 16]).unwrap();
    let frame_id = FrameId::from_bytes([4; 16]).unwrap();
    let inputs = Inputs {
        floor_id,
        frame_id,
        evidence_plane: InputEvidencePlane::Synthetic,
        metric_definition: binding,
        source_artifact: ArtifactReference {
            sha256: ContentHash::from_sha256([5; 32]),
            media_type: Text::new("application/kyberia-test-fixture").unwrap(),
            byte_length: 0,
        },
        samples: vec![Sample {
            observation_id: ObservationId::from_bytes([6; 16]).unwrap(),
            floor_id,
            frame_id,
            position: Point2 {
                x: CoordinateMeters::new(0.0).unwrap(),
                y: CoordinateMeters::new(0.0).unwrap(),
            },
            value: Evidence::Known(Dbm::new(-45.0).unwrap()),
            position_covariance: Evidence::Unknown(UnknownReason::NotMeasured),
        }],
    };
    let config = Config {
        method: Method::Idw { power: 2.0 },
        support_radius: Meters::new(5.0).unwrap(),
        minimum_locations: 1,
        maximum_neighbors: 1,
        extrapolation: Extrapolation::Disabled,
    };
    assert!(matches!(
        Model::new(inputs.clone(), config),
        Err(Error::SpatialMethodMismatch)
    ));

    let point_config = Config {
        method: Method::PointValue,
        ..config
    };
    let model = Model::new(inputs, point_config).unwrap();
    let cell = model
        .estimate(
            Point2 {
                x: CoordinateMeters::new(1.0).unwrap(),
                y: CoordinateMeters::new(0.0).unwrap(),
            },
            &mut || false,
        )
        .unwrap();
    assert_eq!(cell.class, CellClass::Unknown);
    assert_eq!(
        cell.value,
        Evidence::Unknown(UnknownReason::OutsideEvidenceSupport)
    );
}

#[test]
fn verified_binding_keeps_compute_on_same_canonical_definition() {
    let definition = MetricDefinition::signal_rssi().unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let binding = definition.bind(artifact(&definition, &bytes)).unwrap();
    assert_eq!(
        binding.definition_hash().unwrap(),
        definition.content_hash().unwrap()
    );
    assert_eq!(
        binding.definition().ui_help().unwrap().definition_hash,
        binding.definition_hash().unwrap()
    );
}
