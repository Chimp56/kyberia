use kyberia_domain::{
    analysis::{ExactU64, VersionedArtifact},
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{ContentHash, FloorId, FrameId, ObservationId, Text},
    units::{CoordinateMeters, Dbm, Meters},
};
use kyberia_rendering_scene::{
    MAX_SCENE_BYTES, PresentationStyle, Rgba, SceneCellClass, SceneDocument, SceneError,
};
use kyberia_spatial_analysis::{
    Config, Extrapolation, Grid, InputEvidencePlane, MAX_CELLS, Method, MetricDefinition, Model,
    Point2, Sample,
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

fn ids() -> (FloorId, FrameId) {
    (
        FloorId::from_bytes([1; 16]).unwrap(),
        FrameId::from_bytes([2; 16]).unwrap(),
    )
}

fn point(x: f64, y: f64) -> Point2 {
    Point2 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
    }
}

fn source_reference(bytes: &[u8]) -> ArtifactReference {
    ArtifactReference {
        sha256: ContentHash::from_sha256(Sha256::digest(bytes).into()),
        media_type: Text::new("application/x-kyberia-test-source").unwrap(),
        byte_length: bytes.len() as u64,
    }
}

fn model(samples: Vec<Sample>, source: &[u8]) -> Model {
    let (floor_id, frame_id) = ids();
    let definition = MetricDefinition::signal_rssi().unwrap();
    let definition_bytes = definition.canonical_bytes().unwrap();
    let binding = definition
        .bind(VersionedArtifact {
            version: definition.version().clone(),
            sha256: ContentHash::from_sha256(Sha256::digest(&definition_bytes).into()),
            byte_length: ExactU64::new(definition_bytes.len() as u64),
            media_type: Text::new(kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE).unwrap(),
        })
        .unwrap();
    let inputs = kyberia_spatial_analysis::Inputs {
        floor_id,
        frame_id,
        evidence_plane: InputEvidencePlane::Measured,
        metric_definition: binding,
        source_artifact: source_reference(source),
        samples,
    };
    Model::new(
        inputs,
        Config {
            method: Method::PointValue,
            support_radius: Meters::new(5.0).unwrap(),
            minimum_locations: 1,
            maximum_neighbors: 1,
            extrapolation: Extrapolation::Disabled,
        },
    )
    .unwrap()
}

fn sample(id: u8, position: Point2, value: Evidence<Dbm>) -> Sample {
    let (floor_id, frame_id) = ids();
    Sample {
        observation_id: ObservationId::from_bytes([id; 16]).unwrap(),
        floor_id,
        frame_id,
        position,
        value,
        position_covariance: Evidence::Unknown(UnknownReason::NotMeasured),
    }
}

fn indexed_sample(index: u32, position: Point2, value: Evidence<Dbm>) -> Sample {
    let (floor_id, frame_id) = ids();
    let mut bytes = [0_u8; 16];
    bytes[0] = 1;
    bytes[4..8].copy_from_slice(&index.to_be_bytes());
    Sample {
        observation_id: ObservationId::from_bytes(bytes).unwrap(),
        floor_id,
        frame_id,
        position,
        value,
        position_covariance: Evidence::Unknown(UnknownReason::NotMeasured),
    }
}

fn tile(source: &[u8]) -> kyberia_spatial_analysis::Tile {
    let m = model(
        vec![sample(
            1,
            point(7.5, 11.5),
            Evidence::Known(Dbm::new(-42.0).unwrap()),
        )],
        source,
    );
    let (floor_id, frame_id) = ids();
    m.tile(
        Grid {
            floor_id,
            frame_id,
            origin: point(0.0, 0.0),
            resolution: Meters::new(1.0).unwrap(),
            column_offset: 7,
            row_offset: 11,
            width: 2,
            height: 1,
        },
        || false,
    )
    .unwrap()
}

#[test]
fn canonical_scene_preserves_evidence_provenance_and_centers() {
    let source = b"stable-source";
    let document = SceneDocument::from_verified_tile(&tile(source), source).unwrap();

    assert_eq!(document.evidence_plane(), InputEvidencePlane::Measured);
    assert_eq!(document.cells().len(), 2);
    assert_eq!(document.cells()[0].class(), SceneCellClass::Observed);
    assert_eq!(
        document.cells()[1].value(),
        &Evidence::Unknown(UnknownReason::OutsideEvidenceSupport)
    );
    assert_eq!(document.cells()[0].support_observations(), 1);
    assert_eq!(document.cells()[0].contributors()[0].location_group(), 0);
    assert_eq!(document.location_groups()[0].observation_ids().len(), 1);
    assert_eq!(
        document.grid().center(0, 0).unwrap(),
        point_scene(7.5, 11.5)
    );
    assert_eq!(
        document.grid().center(1, 0).unwrap(),
        point_scene(8.5, 11.5)
    );
    assert!(document.canonical_bytes().len() < MAX_SCENE_BYTES);

    let decoded = SceneDocument::from_canonical_bytes(document.canonical_bytes()).unwrap();
    assert_eq!(decoded, document);
    assert_eq!(decoded.sha256(), document.sha256());
    assert_eq!(
        decoded.identity().source_artifact().sha256,
        source_reference(source).sha256
    );
}

fn point_scene(x: f64, y: f64) -> kyberia_rendering_scene::ScenePoint {
    kyberia_rendering_scene::ScenePoint {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
    }
}

#[test]
fn palette_is_nonsemantic_and_unknown_reason_is_not_zero() {
    let document = SceneDocument::from_tile(&tile(b"palette-source")).unwrap();
    let before_bytes = document.canonical_bytes().to_vec();
    let before_hash = document.sha256();
    let first = document.cells()[0].value().clone();
    let second = document.cells()[1].value().clone();
    let cool = PresentationStyle {
        known: Rgba {
            red: 1,
            green: 2,
            blue: 3,
            alpha: 255,
        },
        unknown: Rgba {
            red: 4,
            green: 5,
            blue: 6,
            alpha: 255,
        },
    };
    let warm = PresentationStyle {
        known: Rgba {
            red: 200,
            green: 100,
            blue: 20,
            alpha: 255,
        },
        unknown: Rgba {
            red: 30,
            green: 20,
            blue: 10,
            alpha: 255,
        },
    };
    assert_ne!(
        cool.color_for(&document.cells()[0]),
        warm.color_for(&document.cells()[0])
    );
    assert_ne!(
        cool.color_for(&document.cells()[1]),
        warm.color_for(&document.cells()[1])
    );
    assert!(matches!(first, Evidence::Known(_)));
    assert_eq!(
        second,
        Evidence::Unknown(UnknownReason::OutsideEvidenceSupport)
    );
    assert_eq!(document.canonical_bytes(), before_bytes.as_slice());
    assert_eq!(document.sha256(), before_hash);
}

#[test]
fn mutable_tile_contradictions_are_rejected() {
    let mut frame_bad = tile(b"mutation-source");
    frame_bad.grid.frame_id = FrameId::from_bytes([3; 16]).unwrap();
    assert!(matches!(
        SceneDocument::from_tile(&frame_bad),
        Err(SceneError::InvalidTile("frame mismatch"))
    ));

    let mut class_bad = tile(b"mutation-source");
    class_bad.cells[0].class = kyberia_spatial_analysis::CellClass::Unknown;
    assert!(matches!(
        SceneDocument::from_tile(&class_bad),
        Err(SceneError::InvalidTile("value/class mismatch"))
    ));

    let mut contributor_bad = tile(b"mutation-source");
    contributor_bad.cells[0].contributors[0].location_group = 8;
    assert!(matches!(
        SceneDocument::from_tile(&contributor_bad),
        Err(SceneError::InvalidTile("contributor index"))
    ));

    let mut count_bad = tile(b"mutation-source");
    count_bad.cells.pop();
    assert!(matches!(
        SceneDocument::from_tile(&count_bad),
        Err(SceneError::InvalidTile("cell count"))
    ));
}

#[test]
fn source_bytes_and_wire_versions_are_fail_closed() {
    let mut document_tile = tile(b"authenticated-source");
    assert!(matches!(
        SceneDocument::from_verified_tile(&document_tile, b"wrong"),
        Err(SceneError::SourceArtifactMismatch)
    ));
    assert_eq!(
        SceneDocument::from_verified_tile_with_cancellation(&document_tile, b"wrong", || true),
        Err(SceneError::Cancelled)
    );

    let document = SceneDocument::from_tile(&document_tile).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(document.canonical_bytes()).unwrap();
    value["schema"] = serde_json::Value::String("kyberia.render-scene/2".into());
    let future = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        SceneDocument::from_canonical_bytes(&future),
        Err(SceneError::MalformedBytes) | Err(SceneError::UnsupportedVersion)
    ));

    value["schema"] = serde_json::Value::String("kyberia.render-scene/1".into());
    value["unexpected"] = serde_json::Value::Bool(true);
    let unknown = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        SceneDocument::from_canonical_bytes(&unknown),
        Err(SceneError::MalformedBytes)
    ));

    document_tile.schema_version = "kyberia.numeric-rssi-tile/3";
    assert!(matches!(
        SceneDocument::from_tile(&document_tile),
        Err(SceneError::UnsupportedVersion)
    ));
}

#[test]
fn equivalent_input_permutations_have_identical_scene_bytes() {
    let source = b"permutation-source";
    let (floor_id, frame_id) = ids();
    let a = sample(
        1,
        point(0.5, 0.5),
        Evidence::Known(Dbm::new(-42.0).unwrap()),
    );
    let b = sample(
        2,
        point(1.5, 0.5),
        Evidence::Known(Dbm::new(-50.0).unwrap()),
    );
    let first_model = model(vec![a.clone(), b.clone()], source);
    let second_model = model(vec![b, a], source);
    let grid = Grid {
        floor_id,
        frame_id,
        origin: point(0.0, 0.0),
        resolution: Meters::new(1.0).unwrap(),
        column_offset: 0,
        row_offset: 0,
        width: 2,
        height: 1,
    };
    let first = SceneDocument::from_tile(&first_model.tile(grid, || false).unwrap()).unwrap();
    let second = SceneDocument::from_tile(&second_model.tile(grid, || false).unwrap()).unwrap();
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.sha256(), second.sha256());
    let mut reordered_tile = first_model.tile(grid, || false).unwrap();
    reordered_tile.inputs.samples.reverse();
    let reordered = SceneDocument::from_tile(&reordered_tile).unwrap();
    assert_eq!(first.canonical_bytes(), reordered.canonical_bytes());
    assert_eq!(first.sha256(), reordered.sha256());
    assert_eq!(
        SceneDocument::from_canonical_bytes(reordered.canonical_bytes()).unwrap(),
        reordered
    );
}

#[test]
fn observed_cell_cannot_disagree_with_its_measurement_group() {
    let mut measured = tile(b"observed-value-integrity");
    SceneDocument::from_tile(&measured).unwrap();
    measured.cells[0].value = Evidence::Known(Dbm::new(-12.0).unwrap());
    assert!(SceneDocument::from_tile(&measured).is_err());
}

#[test]
fn serialized_observed_cell_cannot_disagree_with_measurement_group() {
    let document = SceneDocument::from_tile(&tile(b"wire-value-integrity")).unwrap();
    SceneDocument::from_canonical_bytes(document.canonical_bytes()).unwrap();
    let original = String::from_utf8(document.canonical_bytes().to_vec()).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
    let old_value = serde_json::to_string(&Evidence::Known(Dbm::new(-42.0).unwrap())).unwrap();
    value["cells"][0]["value"] =
        serde_json::to_value(Evidence::Known(Dbm::new(-12.0).unwrap())).unwrap();
    let replacement = serde_json::to_string(&Evidence::Known(Dbm::new(-12.0).unwrap())).unwrap();
    // Replace only the cell payload while preserving the canonical field order.
    let cells_offset = original.find("\"cells\":[").unwrap();
    let (prefix, cells) = original.split_at(cells_offset);
    assert!(cells.contains(&old_value));
    let tampered = format!("{prefix}{}", cells.replacen(&old_value, &replacement, 1));
    assert!(matches!(
        SceneDocument::from_canonical_bytes(tampered.as_bytes()),
        Err(SceneError::InvalidDocument(
            "observed value differs from measurement group"
        ))
    ));
}

#[test]
fn group_cannot_override_metric_aggregation_method() {
    let mut measured = tile(b"aggregation-method-integrity");
    SceneDocument::from_tile(&measured).unwrap();
    let source = &measured.inputs.samples[0];
    measured.location_groups[0].signal_aggregate = kyberia_wifi_semantics::aggregate_static(
        &[kyberia_wifi_semantics::StaticSignalSample {
            observation_id: source.observation_id,
            rssi: *source.value.as_known().unwrap(),
        }],
        kyberia_wifi_semantics::AggregateMethod::LinearPowerMean,
    )
    .unwrap();
    assert!(SceneDocument::from_tile(&measured).is_err());
}

#[test]
fn serialized_group_method_must_match_scene_selection() {
    let tile = tile(b"serialized-aggregation");
    let scene = SceneDocument::from_tile(&tile).unwrap();
    assert_eq!(
        scene.identity().signal_aggregation(),
        tile.signal_aggregation()
    );
    let bytes = String::from_utf8(scene.canonical_bytes().to_vec()).unwrap();
    let groups = bytes.find("\"location_groups\":[").unwrap();
    let (prefix, suffix) = bytes.split_at(groups);
    let old = "\"method\":\"median_dbm\"";
    assert!(suffix.contains(old));
    let altered = format!(
        "{prefix}{}",
        suffix.replacen(old, "\"method\":\"linear_power_mean\"", 1)
    );
    assert!(matches!(
        SceneDocument::from_canonical_bytes(altered.as_bytes()),
        Err(SceneError::InvalidDocument(
            "group aggregation method mismatch"
        ))
    ));
}

#[test]
fn metric_definition_artifact_and_identity_tampering_are_rejected() {
    let scene = SceneDocument::from_tile(&tile(b"metric-artifact-integrity")).unwrap();
    SceneDocument::from_canonical_bytes(scene.canonical_bytes()).unwrap();
    let original: serde_json::Value = serde_json::from_slice(scene.canonical_bytes()).unwrap();
    for mutation in 0..3 {
        let mut altered = original.clone();
        let expected = match mutation {
            0 => {
                altered["metric_definition_bytes"][0] = serde_json::json!(0);
                "metric definition binding"
            }
            1 => {
                altered["identity"]["metric_definition_hash"] =
                    serde_json::to_value(ContentHash::from_sha256([9; 32])).unwrap();
                "metric identity mismatch"
            }
            _ => {
                altered["identity"]["signal_aggregation"]["method"] =
                    serde_json::to_value(kyberia_wifi_semantics::AggregateMethod::LinearPowerMean)
                        .unwrap();
                "metric definition binding"
            }
        };
        let encoded = serde_json::to_vec(&altered).unwrap();
        let error = SceneDocument::from_canonical_bytes(&encoded).unwrap_err();
        assert_eq!(
            error,
            SceneError::InvalidDocument(expected),
            "mutation {mutation} must fail semantic binding before canonical ordering checks"
        );
    }
}

#[test]
fn interpolation_configuration_survives_roundtrip_and_rejects_invalid_policy() {
    let tile = tile(b"configuration-provenance");
    let scene = SceneDocument::from_tile(&tile).unwrap();
    let reopened = SceneDocument::from_canonical_bytes(scene.canonical_bytes()).unwrap();
    assert_eq!(reopened.configuration(), tile.configuration);
    let mut value: serde_json::Value = serde_json::from_slice(scene.canonical_bytes()).unwrap();
    value["configuration"]["minimum_locations"] = serde_json::json!(0);
    assert!(matches!(
        SceneDocument::from_canonical_bytes(&serde_json::to_vec(&value).unwrap()),
        Err(SceneError::InvalidDocument("configuration"))
    ));
}

#[test]
fn scene_configuration_cannot_override_metric_spatial_method() {
    let mut tile = tile(b"spatial-method-integrity");
    let scene = SceneDocument::from_tile(&tile).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(scene.canonical_bytes()).unwrap();
    value["configuration"]["method"] = serde_json::to_value(Method::Nearest).unwrap();
    assert!(matches!(
        SceneDocument::from_canonical_bytes(&serde_json::to_vec(&value).unwrap()),
        Err(SceneError::InvalidDocument(
            "metric spatial method mismatch"
        ))
    ));
    tile.configuration.method = Method::Nearest;
    assert!(SceneDocument::from_tile(&tile).is_err());
}

#[test]
fn serialized_duplicate_contributors_are_rejected() {
    let scene = SceneDocument::from_tile(&tile(b"duplicate-wire-support")).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(scene.canonical_bytes()).unwrap();
    let mut contribution = value["cells"][0]["contributors"][0].clone();
    contribution["weight"] = serde_json::json!(0.5);
    value["cells"][0]["contributors"] = serde_json::json!([contribution.clone(), contribution]);
    let error =
        SceneDocument::from_canonical_bytes(&serde_json::to_vec(&value).unwrap()).unwrap_err();
    assert_eq!(error, SceneError::InvalidDocument("duplicate contributor"));
}

#[test]
fn disabled_extrapolation_cannot_admit_extrapolated_cells() {
    let mut tile = tile(b"extrapolation-policy");
    tile.cells[0].class = kyberia_spatial_analysis::CellClass::Extrapolated;
    assert!(SceneDocument::from_tile(&tile).is_err());
}

#[test]
fn direct_admission_rejects_forged_interpolation_and_shifted_observed_grid() {
    let mut forged = tile(b"numerical-replay");
    forged.cells[0].class = kyberia_spatial_analysis::CellClass::Interpolated;
    forged.cells[0].value = Evidence::Known(Dbm::new(-12.0).unwrap());
    assert!(SceneDocument::from_tile(&forged).is_err());
    let mut shifted = tile(b"numerical-replay");
    shifted.grid.origin = point(100.0, 100.0);
    assert!(SceneDocument::from_tile(&shifted).is_err());
}

#[test]
fn cancellation_before_and_during_admission_returns_no_scene() {
    let tile = tile(b"cancel-replay");
    let original = serde_json::to_vec(&tile).unwrap();
    for stop_at in [1, 3] {
        let mut polls = 0;
        let result = SceneDocument::from_tile_with_cancellation(&tile, || {
            polls += 1;
            polls >= stop_at
        });
        assert_eq!(result.unwrap_err(), SceneError::Cancelled);
        assert_eq!(serde_json::to_vec(&tile).unwrap(), original);
    }
}

#[test]
fn serialized_scene_replays_samples_instead_of_trusting_derived_cells() {
    let scene = SceneDocument::from_tile(&tile(b"serialized-replay")).unwrap();
    let original: serde_json::Value = serde_json::from_slice(scene.canonical_bytes()).unwrap();
    for mutation in 0..3 {
        let mut value = original.clone();
        match mutation {
            0 => {
                value["cells"][0]["class"] = serde_json::json!("interpolated");
                value["cells"][0]["value"] =
                    serde_json::to_value(Evidence::Known(Dbm::new(-12.0).unwrap())).unwrap();
            }
            1 => value["grid"]["origin"]["x"] = serde_json::json!(100.0),
            _ => {
                value["samples"][0]["value"] =
                    serde_json::to_value(Evidence::Known(Dbm::new(-12.0).unwrap())).unwrap()
            }
        }
        assert_eq!(
            SceneDocument::from_canonical_bytes(&serde_json::to_vec(&value).unwrap()).unwrap_err(),
            SceneError::InvalidDocument("scene differs from canonical computation"),
            "mutation {mutation} must fail numerical replay before encoding-order checks"
        );
    }
}

#[test]
fn serialized_import_cancellation_never_returns_a_partial_scene() {
    let scene = SceneDocument::from_tile(&tile(b"cancel-import")).unwrap();
    for stop_at in [1, 2, 3, 4, 5] {
        let mut polls = 0;
        assert_eq!(
            SceneDocument::from_canonical_bytes_with_cancellation(scene.canonical_bytes(), || {
                polls += 1;
                polls >= stop_at
            })
            .unwrap_err(),
            SceneError::Cancelled
        );
        assert_eq!(polls, stop_at);
    }
    assert_eq!(
        SceneDocument::from_canonical_bytes_with_cancellation(scene.canonical_bytes(), || false)
            .unwrap(),
        scene
    );
}

#[test]
fn public_admission_rejects_excessive_replay_work_before_validation_clones() {
    let mut oversized = tile(b"replay-work-budget");
    oversized.grid.width = MAX_CELLS as u32;
    oversized.grid.height = 1;
    let group = oversized.location_groups[0].clone();
    oversized.location_groups.resize(1_001, group);
    assert_eq!(
        SceneDocument::from_tile(&oversized),
        Err(SceneError::ResourceLimit("scene replay work"))
    );
}

#[test]
fn representative_large_tile_is_admitted_and_cancellation_stays_bounded() {
    let source = b"representative-large-scene";
    let samples = (0..512_u32)
        .map(|index| {
            let column = index % 32;
            let row = index / 32;
            indexed_sample(
                index,
                point(f64::from(column) + 0.5, f64::from(row) + 0.5),
                Evidence::Known(Dbm::new(-42.0 - f64::from(index % 32)).unwrap()),
            )
        })
        .collect();
    let model = model(samples, source);
    let (floor_id, frame_id) = ids();
    let large_tile = model
        .tile(
            Grid {
                floor_id,
                frame_id,
                origin: point(0.0, 0.0),
                resolution: Meters::new(1.0).unwrap(),
                column_offset: 0,
                row_offset: 0,
                width: 32,
                height: 32,
            },
            || false,
        )
        .unwrap();

    let scene = SceneDocument::from_tile(&large_tile).unwrap();
    assert_eq!(scene.cells().len(), 1024);
    assert!(scene.canonical_bytes().len() < MAX_SCENE_BYTES);

    let mut polls = 0;
    let cancelled = SceneDocument::from_tile_with_cancellation(&large_tile, || {
        polls += 1;
        polls >= 32
    });
    assert_eq!(cancelled, Err(SceneError::Cancelled));
    assert_eq!(polls, 32);
}

#[test]
fn resource_measurement_harness_covers_direct_import_and_cancelled_paths() {
    let source = b"resource-measurement-scene";
    let samples = (0..512_u32)
        .map(|index| {
            let column = index % 32;
            let row = index / 32;
            indexed_sample(
                index,
                point(f64::from(column) + 0.5, f64::from(row) + 0.5),
                Evidence::Known(Dbm::new(-42.0 - f64::from(index % 32)).unwrap()),
            )
        })
        .collect();
    let model = model(samples, source);
    let (floor_id, frame_id) = ids();
    let large_tile = model
        .tile(
            Grid {
                floor_id,
                frame_id,
                origin: point(0.0, 0.0),
                resolution: Meters::new(1.0).unwrap(),
                column_offset: 0,
                row_offset: 0,
                width: 32,
                height: 32,
            },
            || false,
        )
        .unwrap();

    let direct_started = Instant::now();
    let direct = SceneDocument::from_tile(&large_tile).unwrap();
    let direct_elapsed = direct_started.elapsed();
    let canonical = direct.canonical_bytes().to_vec();
    let estimate = SceneDocument::estimate_canonical_resources(&canonical, || false).unwrap();

    let import_started = Instant::now();
    let imported = SceneDocument::from_canonical_bytes(&canonical).unwrap();
    let import_elapsed = import_started.elapsed();
    assert_eq!(imported, direct);
    assert_eq!(estimate.encoded_bytes(), canonical.len());
    assert!(estimate.working_set_bytes() <= kyberia_rendering_scene::MAX_SCENE_WORKING_BYTES);

    let mut cancel_polls = 0;
    let cancel_started = Instant::now();
    let cancelled = SceneDocument::from_canonical_bytes_with_cancellation(&canonical, || {
        cancel_polls += 1;
        cancel_polls == 8
    });
    let cancel_elapsed = cancel_started.elapsed();
    assert_eq!(cancelled, Err(SceneError::Cancelled));
    assert_eq!(cancel_polls, 8);
    assert!(cancel_elapsed < Duration::from_secs(2));
    assert!(direct_elapsed > Duration::ZERO);
    assert!(import_elapsed > Duration::ZERO);
    println!(
        "scene_measurement encoded_bytes={} working_set_estimate={} direct_us={} import_us={} cancel_us={} cancel_polls={}",
        estimate.encoded_bytes(),
        estimate.working_set_bytes(),
        direct_elapsed.as_micros(),
        import_elapsed.as_micros(),
        cancel_elapsed.as_micros(),
        cancel_polls,
    );
}
