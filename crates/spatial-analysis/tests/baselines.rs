use kyberia_domain::{
    analysis::{ExactU64, VersionedArtifact},
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{ContentHash, FloorId, FrameId, ObservationId, Text},
    units::{CoordinateMeters, Db, Dbm, Dimensionless, Meters, Probability},
};
use kyberia_spatial_analysis::*;
use proptest::prelude::*;
use sha2::{Digest, Sha256};

fn point(x: f64, y: f64) -> Point2 {
    Point2 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
    }
}
fn sample(id: u128, x: f64, y: f64, value: f64) -> Sample {
    Sample {
        observation_id: ObservationId::from_bytes(id.to_be_bytes()).unwrap(),
        floor_id: FloorId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([2; 16]).unwrap(),
        position: point(x, y),
        value: Evidence::Known(Dbm::new(value).unwrap()),
        position_covariance: Evidence::Unknown(UnknownReason::NotMeasured),
    }
}
fn config() -> Config {
    Config {
        method: Method::Idw { power: 2.0 },
        support_radius: Meters::new(5.0).unwrap(),
        minimum_locations: 1,
        maximum_neighbors: 8,
        extrapolation: Extrapolation::Disabled,
    }
}
fn artifact_for(version: &Text, bytes: &[u8]) -> VersionedArtifact {
    VersionedArtifact {
        version: version.clone(),
        sha256: ContentHash::from_sha256(Sha256::digest(bytes).into()),
        byte_length: ExactU64::new(bytes.len() as u64),
        media_type: Text::new(SIGNAL_METRIC_DEFINITION_MEDIA_TYPE).unwrap(),
    }
}
fn metric_binding(method: AggregateMethod) -> MetricDefinitionBinding {
    let selection = SignalAggregationSelection::new(method);
    let definition = SignalMetricDefinition::new(
        Text::new("test/synthetic-single-transmitter-rssi/1").unwrap(),
        selection,
    )
    .unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let artifact = artifact_for(definition.version(), &bytes);
    definition.bind(artifact).unwrap()
}
fn inputs(samples: Vec<Sample>) -> Inputs {
    Inputs {
        floor_id: FloorId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([2; 16]).unwrap(),
        metric_definition: metric_binding(AggregateMethod::MedianDbm),
        evidence_plane: InputEvidencePlane::Synthetic,
        source_artifact: ArtifactReference {
            sha256: ContentHash::from_sha256([1; 32]),
            media_type: Text::new("application/kyberia-test-fixture").unwrap(),
            byte_length: 0,
        },
        samples,
    }
}
fn inputs_with_aggregation(samples: Vec<Sample>, method: AggregateMethod) -> Inputs {
    let mut inputs = inputs(samples);
    inputs.metric_definition = metric_binding(method);
    inputs
}
fn model(samples: Vec<Sample>) -> Model {
    Model::new(inputs(samples), config()).unwrap()
}
fn model_with_aggregation(samples: Vec<Sample>, method: AggregateMethod) -> Model {
    Model::new(inputs_with_aggregation(samples, method), config()).unwrap()
}
fn estimate(model: &Model, x: f64, y: f64) -> Cell {
    model.estimate(point(x, y), &mut || false).unwrap()
}
fn value(cell: &Cell) -> f64 {
    cell.value.as_known().unwrap().get()
}
fn material(loss_db: f64, traversal_m: f64, policy: BarrierPolicy) -> BarrierMaterial {
    BarrierMaterial::new(
        Meters::new(traversal_m).unwrap(),
        Db::new(loss_db).unwrap(),
        policy,
    )
    .unwrap()
}
fn barrier(
    id: u32,
    start: (f64, f64),
    end: (f64, f64),
    material: BarrierMaterial,
) -> BarrierSegment {
    BarrierSegment::new(
        BarrierId::new(id).unwrap(),
        point(start.0, start.1),
        point(end.0, end.1),
        material,
    )
    .unwrap()
}
fn barrier_model(samples: Vec<Sample>, barriers: Vec<BarrierSegment>) -> Model {
    Model::new_with_barriers(
        inputs(samples),
        config(),
        BarrierSet::new(barriers).unwrap(),
    )
    .unwrap()
}
fn grid(width: u32, height: u32) -> Grid {
    Grid {
        floor_id: FloorId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([2; 16]).unwrap(),
        origin: point(0.0, 0.0),
        resolution: Meters::new(1.0).unwrap(),
        column_offset: 0,
        row_offset: 0,
        width,
        height,
    }
}

#[test]
fn one_wall_changes_neighbor_ranking_and_value() {
    let plain = model(vec![
        sample(1, 1.0, 0.0, -40.0),
        sample(2, -3.0, 0.0, -80.0),
    ]);
    let barrier_model = barrier_model(
        vec![sample(1, 1.0, 0.0, -40.0), sample(2, -3.0, 0.0, -80.0)],
        vec![barrier(
            1,
            (0.0, -10.0),
            (0.0, 10.0),
            material(40.0, 0.0, BarrierPolicy::Passable),
        )],
    );
    let plain_cell = estimate(&plain, -0.1, 0.0);
    let wall_cell = estimate(&barrier_model, -0.1, 0.0);
    // Location groups are canonically ordered by coordinates (the left
    // sample is group 0), while neighbor order is by effective distance.
    assert_eq!(plain_cell.contributors[0].location_group, 1);
    assert_eq!(wall_cell.contributors[0].location_group, 0);
    assert!(value(&wall_cell) < -79.0);
    assert!(wall_cell.nearest_distance.as_known().unwrap().get() > 0.0);
}

#[test]
fn robust_barrier_intersections_retain_shallow_touch_and_overlap_cases() {
    let shallow = barrier_model(
        vec![sample(1, 1.0e9, 0.0, -40.0)],
        vec![barrier(
            1,
            (5.0e8, -1.0e-5),
            (5.0e8, 1.0e-5),
            material(0.0, 1.0, BarrierPolicy::Passable),
        )],
    );
    let shallow_path = shallow
        .path_to_group(point(0.0, 0.0), 0, &mut || false)
        .unwrap();
    assert_eq!(
        shallow_path.crossed_barriers,
        vec![BarrierId::new(1).unwrap()]
    );

    let shifted = barrier_model(
        vec![sample(2, 1.2e9, 3.0e8, -40.0)],
        vec![barrier(
            2,
            (7.0e8, 3.0e8 - 1.0e-5),
            (7.0e8, 3.0e8 + 1.0e-5),
            material(0.0, 1.0, BarrierPolicy::Passable),
        )],
    );
    let shifted_path = shifted
        .path_to_group(point(2.0e8, 3.0e8), 0, &mut || false)
        .unwrap();
    assert_eq!(
        shifted_path.crossed_barriers,
        vec![BarrierId::new(2).unwrap()]
    );

    let touch_and_collinear = barrier_model(
        vec![sample(3, 2.0, 0.0, -40.0)],
        vec![
            barrier(
                3,
                (2.0, 0.0),
                (2.0, 1.0),
                material(0.0, 1.0, BarrierPolicy::Passable),
            ),
            barrier(
                4,
                (1.0, 0.0),
                (3.0, 0.0),
                material(0.0, 1.0, BarrierPolicy::Passable),
            ),
            barrier(
                5,
                (3.0, 0.0),
                (4.0, 0.0),
                material(0.0, 1.0, BarrierPolicy::Passable),
            ),
        ],
    );
    let touch_path = touch_and_collinear
        .path_to_group(point(0.0, 0.0), 0, &mut || false)
        .unwrap();
    assert_eq!(
        touch_path.crossed_barriers,
        vec![BarrierId::new(3).unwrap(), BarrierId::new(4).unwrap()]
    );

    let diagonal = barrier_model(
        vec![sample(7, 2.0, 2.0, -40.0)],
        vec![
            barrier(
                7,
                (1.0, 1.0),
                (3.0, 3.0),
                material(0.0, 1.0, BarrierPolicy::Passable),
            ),
            barrier(
                8,
                (3.0, 3.0),
                (4.0, 4.0),
                material(0.0, 1.0, BarrierPolicy::Passable),
            ),
        ],
    );
    let diagonal_path = diagonal
        .path_to_group(point(0.0, 0.0), 0, &mut || false)
        .unwrap();
    assert_eq!(
        diagonal_path.crossed_barriers,
        vec![BarrierId::new(7).unwrap()]
    );

    let unresolved = barrier_model(
        vec![sample(6, 1.0e9, 1.0e9, -40.0)],
        vec![barrier(
            6,
            (5.0e8, 1.0e-315),
            (5.0e8, 2.0e-315),
            material(0.0, 1.0, BarrierPolicy::Passable),
        )],
    );
    assert_eq!(
        unresolved.path_to_group(point(0.0, 0.0), 0, &mut || false),
        Err(Error::NumericalFailure("barrier intersection unresolved"))
    );
}

#[test]
fn extreme_admitted_attenuation_uses_stable_log_weights() {
    let barriers = (0..11)
        .map(|index| {
            barrier(
                index + 1,
                (0.05 * f64::from(index), -1.0),
                (0.05 * f64::from(index), 1.0),
                material(300.0, 0.0, BarrierPolicy::Passable),
            )
        })
        .collect();
    let m = barrier_model(
        vec![sample(1, 1.0, 0.0, -40.0), sample(2, 1.2, 0.0, -80.0)],
        barriers,
    );
    let path = m.path_to_group(point(0.0, 0.0), 0, &mut || false).unwrap();
    assert_eq!(
        path.path_cost.as_known().unwrap().attenuation_db.get(),
        3300.0
    );

    let cell = estimate(&m, 0.0, 0.0);
    let ratio = (1.0_f64 / 1.2).powi(2);
    let oracle = (-40.0 + ratio * -80.0) / (1.0 + ratio);
    assert!((value(&cell) - oracle).abs() < 1.0e-12);
    assert_eq!(cell.contributors.len(), 2);
}

#[test]
fn identical_geometry_with_distinct_ids_is_additive_layered_material() {
    let same_geometry = barrier(
        1,
        (1.0, -2.0),
        (1.0, 2.0),
        material(4.0, 2.0, BarrierPolicy::Passable),
    );
    let second_layer = BarrierSegment::new(
        BarrierId::new(2).unwrap(),
        same_geometry.start,
        same_geometry.end,
        material(6.0, 3.0, BarrierPolicy::Passable),
    )
    .unwrap();
    let m = barrier_model(
        vec![sample(1, 2.0, 0.0, -40.0)],
        vec![same_geometry, second_layer],
    );
    let path = m.path_to_group(point(0.0, 0.0), 0, &mut || false).unwrap();
    let cost = path.path_cost.as_known().unwrap();
    assert_eq!(path.crossed_barriers.len(), 2);
    assert_eq!(cost.crossed_barriers.len(), 2);
    assert_eq!(cost.traversal_cost.get(), 5.0);
    assert_eq!(cost.attenuation_db.get(), 10.0);
}

#[test]
fn two_wall_path_cost_is_retained_and_affects_support() {
    let m = barrier_model(
        vec![sample(1, 4.0, 0.0, -40.0)],
        vec![
            barrier(
                10,
                (1.0, -2.0),
                (1.0, 2.0),
                material(0.0, 2.0, BarrierPolicy::Passable),
            ),
            barrier(
                20,
                (2.0, -2.0),
                (2.0, 2.0),
                material(0.0, 3.0, BarrierPolicy::Passable),
            ),
        ],
    );
    let path = m.path_to_group(point(0.0, 0.0), 0, &mut || false).unwrap();
    let cost = path.path_cost.as_known().unwrap();
    assert_eq!(cost.geometric_distance.get(), 4.0);
    assert_eq!(cost.traversal_cost.get(), 5.0);
    assert_eq!(cost.total_cost.get(), 9.0);
    assert_eq!(
        cost.crossed_barriers,
        vec![BarrierId::new(10).unwrap(), BarrierId::new(20).unwrap()]
    );
    assert_eq!(
        estimate(&m, 0.0, 0.0)
            .nearest_distance
            .as_known()
            .unwrap()
            .get(),
        9.0
    );
    assert_eq!(
        m.path_assessments(point(0.0, 0.0), &mut || false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn equal_distance_material_loss_changes_contributor_weight() {
    let m = barrier_model(
        vec![sample(1, 2.0, 0.0, -40.0), sample(2, 0.0, 2.0, -80.0)],
        vec![
            barrier(
                1,
                (1.0, -2.0),
                (1.0, 2.0),
                material(0.0, 0.0, BarrierPolicy::Passable),
            ),
            barrier(
                2,
                (-2.0, 1.0),
                (2.0, 1.0),
                material(30.0, 0.0, BarrierPolicy::Passable),
            ),
        ],
    );
    let cell = estimate(&m, 0.0, 0.0);
    assert_eq!(cell.contributors.len(), 2);
    assert!(cell.contributors[0].weight.get() > cell.contributors[1].weight.get() * 100.0);
    assert!(value(&cell) < -39.0 && value(&cell) > -81.0);
}

#[test]
fn impassable_barrier_preserves_unknown_and_path_diagnosis() {
    let m = barrier_model(
        vec![sample(1, 1.0, 0.0, -40.0)],
        vec![barrier(
            7,
            (0.5, -10.0),
            (0.5, 10.0),
            material(0.0, 0.0, BarrierPolicy::Impassable),
        )],
    );
    let cell = estimate(&m, 0.0, 0.0);
    assert_eq!(cell.class, CellClass::Unknown);
    assert_eq!(
        cell.value,
        Evidence::Unknown(UnknownReason::OutsideEvidenceSupport)
    );
    assert!(cell.contributors.is_empty());
    let path = m.path_to_group(point(0.0, 0.0), 0, &mut || false).unwrap();
    assert_eq!(
        path.path_cost,
        Evidence::Unknown(UnknownReason::OutsideEvidenceSupport)
    );
    assert_eq!(path.blocked_by, vec![BarrierId::new(7).unwrap()]);
    assert_eq!(path.crossed_barriers, path.blocked_by);
    let all = m.path_assessments(point(0.0, 0.0), &mut || false).unwrap();
    assert_eq!(all[0].blocked_by, path.blocked_by);
}

#[test]
fn barrier_extrapolation_is_explicit_and_uses_path_cost_radius() {
    let mut cfg = config();
    cfg.extrapolation = Extrapolation::WithinRadius(Meters::new(10.0).unwrap());
    let m = Model::new_with_barriers(
        inputs(vec![sample(1, 4.0, 0.0, -40.0)]),
        cfg,
        BarrierSet::new(vec![barrier(
            1,
            (2.0, -2.0),
            (2.0, 2.0),
            material(0.0, 2.0, BarrierPolicy::Passable),
        )])
        .unwrap(),
    )
    .unwrap();
    let cell = estimate(&m, 0.0, 0.0);
    assert_eq!(cell.class, CellClass::Extrapolated);
    assert_eq!(cell.support_locations, 0);
    assert_eq!(cell.nearest_distance.as_known().unwrap().get(), 6.0);
}

#[test]
fn barrier_contract_rejects_malformed_and_duplicate_segments() {
    let id = BarrierId::new(1).unwrap();
    assert!(BarrierId::new(0).is_err());
    assert!(
        BarrierSegment::new(
            id,
            point(0.0, 0.0),
            point(0.0, 0.0),
            material(0.0, 0.0, BarrierPolicy::Passable),
        )
        .is_err()
    );
    assert!(
        BarrierMaterial::new(
            Meters::new(0.0).unwrap(),
            Db::new(-1.0).unwrap(),
            BarrierPolicy::Passable,
        )
        .is_err()
    );
    assert!(
        BarrierSet::new(vec![
            barrier(
                1,
                (0.0, 0.0),
                (0.0, 1.0),
                material(0.0, 0.0, BarrierPolicy::Passable)
            ),
            barrier(
                1,
                (1.0, 0.0),
                (1.0, 1.0),
                material(0.0, 0.0, BarrierPolicy::Passable)
            ),
        ])
        .is_err()
    );
    let huge = BarrierSegment::new(
        BarrierId::new(2).unwrap(),
        point(MAX_BARRIER_COORDINATE + 1.0, 0.0),
        point(MAX_BARRIER_COORDINATE + 2.0, 0.0),
        material(0.0, 0.0, BarrierPolicy::Passable),
    );
    assert!(huge.is_err());
    let malformed = serde_json::json!({
        "barriers": [{
            "id": 3,
            "start": {"x": 0.0, "y": 0.0},
            "end": {"x": 0.0, "y": 0.0},
            "material": {"traversal_cost": 0.0, "attenuation_db": 0.0, "policy": "passable"}
        }]
    });
    assert!(serde_json::from_value::<BarrierSet>(malformed).is_err());
    let unknown_point_field = serde_json::json!({
        "barriers": [{
            "id": 3,
            "start": {"x": 0.0, "y": 0.0, "z": 1.0},
            "end": {"x": 0.0, "y": 1.0},
            "material": {"traversal_cost": 0.0, "attenuation_db": 0.0, "policy": "passable"}
        }]
    });
    assert!(serde_json::from_value::<BarrierSet>(unknown_point_field).is_err());
}

#[test]
fn barrier_order_and_translation_are_deterministic() {
    let first = vec![
        barrier(
            2,
            (1.0, -4.0),
            (1.0, 4.0),
            material(6.0, 1.0, BarrierPolicy::Passable),
        ),
        barrier(
            1,
            (2.0, -4.0),
            (2.0, 4.0),
            material(2.0, 1.0, BarrierPolicy::Passable),
        ),
    ];
    let reverse = first.iter().rev().copied().collect::<Vec<_>>();
    let a = barrier_model(vec![sample(1, 3.0, 0.0, -40.0)], first);
    let b = barrier_model(vec![sample(1, 3.0, 0.0, -40.0)], reverse);
    assert_eq!(
        a.barriers().canonical_bytes().unwrap(),
        b.barriers().canonical_bytes().unwrap()
    );
    let decoded: BarrierSet =
        serde_json::from_slice(&a.barriers().canonical_bytes().unwrap()).unwrap();
    assert_eq!(&decoded, a.barriers());
    assert_eq!(
        decoded.content_hash().unwrap(),
        a.barriers().content_hash().unwrap()
    );
    assert_eq!(estimate(&a, 0.0, 0.0), estimate(&b, 0.0, 0.0));

    let shifted = barrier_model(
        vec![sample(1, 13.0, -7.0, -40.0)],
        vec![
            barrier(
                2,
                (11.0, -11.0),
                (11.0, -3.0),
                material(6.0, 1.0, BarrierPolicy::Passable),
            ),
            barrier(
                1,
                (12.0, -11.0),
                (12.0, -3.0),
                material(2.0, 1.0, BarrierPolicy::Passable),
            ),
        ],
    );
    assert_eq!(estimate(&a, 0.0, 0.0), estimate(&shifted, 10.0, -7.0));
}

#[test]
fn barrier_tile_serialization_retains_canonical_barrier_identity() {
    let m = barrier_model(
        vec![sample(1, 2.0, 0.0, -40.0)],
        vec![barrier(
            3,
            (1.0, -2.0),
            (1.0, 2.0),
            material(10.0, 1.0, BarrierPolicy::Passable),
        )],
    );
    let tile = m.tile(grid(1, 1), || false).unwrap();
    assert_eq!(tile.algorithm_version, BARRIER_ALGORITHM_VERSION);
    let json = serde_json::to_value(&tile).unwrap();
    assert!(json.get("barriers").is_some());
    assert_eq!(json["barriers"]["barriers"][0]["id"], serde_json::json!(3));
}

#[test]
fn barrier_work_and_cancellation_are_bounded() {
    let samples = (0_u32..25_000)
        .map(|id| sample(u128::from(id + 1), 0.0, f64::from(id), -40.0))
        .collect::<Vec<_>>();
    let barriers = (1..=MAX_BARRIERS)
        .map(|id| {
            barrier(
                id as u32,
                (100.0, 0.0),
                (100.0, 1.0),
                material(0.0, 0.0, BarrierPolicy::Passable),
            )
        })
        .collect::<Vec<_>>();
    let m = barrier_model(samples, barriers);
    assert_eq!(
        m.estimate(point(0.0, 0.0), &mut || false),
        Err(Error::ResourceLimit("barrier evaluations"))
    );
    // A single path only charges one group by the 4,096 barriers. It remains
    // admissible even though evaluating every group would exceed the bound.
    assert!(m.path_to_group(point(0.0, -1.0), 0, &mut || false).is_ok());
    let small = barrier_model(
        vec![sample(1, 1.0, 0.0, -40.0)],
        vec![barrier(
            1,
            (0.5, -2.0),
            (0.5, 2.0),
            material(1.0, 1.0, BarrierPolicy::Passable),
        )],
    );
    assert_eq!(
        small.estimate(point(0.0, 0.0), &mut || true),
        Err(Error::Cancelled)
    );
    assert_eq!(
        small.path_to_group(point(1.0, 0.0), 0, &mut || true),
        Err(Error::Cancelled)
    );
}

#[test]
fn exact_points_and_known_two_point_weights() {
    let m = model(vec![sample(1, 0.0, 0.0, -40.0), sample(2, 4.0, 0.0, -80.0)]);
    let observed = estimate(&m, 0.0, 0.0);
    assert_eq!(value(&observed), -40.0);
    assert_eq!(observed.class, CellClass::Observed);
    let a = estimate(&m, 1.0, 0.0);
    assert!((value(&a) + 44.0).abs() < 1e-12);
    assert_eq!(a.class, CellClass::Interpolated);
    assert!((a.contributors[0].weight.get() - 0.9).abs() < 1e-15);
    assert_eq!(
        a.uncertainty_db,
        Evidence::Unknown(UnknownReason::NotMeasured)
    );
}
#[test]
fn empty_and_unknown_values_never_become_zero() {
    let m = model(vec![]);
    let c = estimate(&m, 0.0, 0.0);
    assert_eq!(c.value, Evidence::Unknown(UnknownReason::NotMeasured));
    assert_eq!(c.class, CellClass::Unknown);
    let mut s = sample(1, 0.0, 0.0, -40.0);
    s.value = Evidence::Unknown(UnknownReason::UnsupportedCapability);
    let m = model(vec![s]);
    assert_eq!(m.inputs().samples.len(), 1);
    assert!(m.groups().is_empty());
    assert_eq!(estimate(&m, 0.0, 0.0).support_observations, 0);
}
#[test]
fn one_point_has_explicit_radius_and_two_clusters_leave_gap() {
    let m = model(vec![sample(1, 0.0, 0.0, -40.0)]);
    assert_eq!(estimate(&m, 5.0, 0.0).class, CellClass::Interpolated);
    let c = estimate(&m, 5.001, 0.0);
    assert_eq!(c.class, CellClass::Unknown);
    assert_eq!(c.nearest_distance.as_known().unwrap().get(), 5.001);
    let m = model(vec![
        sample(1, 0.0, 0.0, -40.0),
        sample(2, 20.0, 0.0, -80.0),
    ]);
    let c = estimate(&m, 10.0, 0.0);
    assert_eq!(c.class, CellClass::Unknown);
    assert!(c.value.as_known().is_none());
    assert_eq!(c.support_locations, 0);
}
#[test]
fn explicit_extrapolation_is_labeled_and_bounded() {
    let mut cfg = config();
    cfg.extrapolation = Extrapolation::WithinRadius(Meters::new(10.0).unwrap());
    let m = Model::new(inputs(vec![sample(1, 0.0, 0.0, -40.0)]), cfg).unwrap();
    let c = estimate(&m, 8.0, 0.0);
    assert_eq!(c.class, CellClass::Extrapolated);
    assert_eq!(c.support_locations, 0);
    assert_eq!(value(&c), -40.0);
    assert_eq!(estimate(&m, 11.0, 0.0).class, CellClass::Unknown);
}
#[test]
fn extrapolation_option_does_not_change_supported_result() {
    let samples = vec![sample(1, 0.0, 0.0, -40.0), sample(2, 7.0, 0.0, -80.0)];
    let baseline = model(samples.clone());
    let mut cfg = config();
    cfg.extrapolation = Extrapolation::WithinRadius(Meters::new(10.0).unwrap());
    let extended = Model::new(inputs(samples), cfg).unwrap();
    assert_eq!(estimate(&baseline, 1.0, 0.0), estimate(&extended, 1.0, 0.0));
}
#[test]
fn coincident_mean_is_dbm_and_repeats_do_not_inflate_location_count() {
    let m = model(vec![
        sample(3, 0.0, 0.0, -40.0),
        sample(1, 0.0, 0.0, -80.0),
        sample(2, 4.0, 0.0, -80.0),
    ]);
    let c = estimate(&m, 0.0, 0.0);
    assert_eq!(value(&c), -60.0);
    assert_eq!(c.support_locations, 2);
    assert_eq!(c.support_observations, 3);
    assert_eq!(
        m.groups()[0].observation_ids,
        vec![
            ObservationId::from_bytes(1_u128.to_be_bytes()).unwrap(),
            ObservationId::from_bytes(3_u128.to_be_bytes()).unwrap()
        ]
    );
    assert_eq!(value(&estimate(&m, 2.0, 0.0)), -70.0);
    let mut cfg = config();
    cfg.minimum_locations = 2;
    let m = Model::new(
        inputs(vec![sample(1, 0.0, 0.0, -40.0), sample(2, 0.0, 0.0, -80.0)]),
        cfg,
    )
    .unwrap();
    assert_eq!(estimate(&m, 1.0, 0.0).class, CellClass::Unknown);
    assert_eq!(estimate(&m, 0.0, 0.0).class, CellClass::Observed);
}

#[test]
fn coincident_groups_use_the_metric_selected_static_aggregation() {
    let samples = vec![
        sample(1, 0.0, 0.0, -80.0),
        sample(2, 0.0, 0.0, -70.0),
        sample(3, 0.0, 0.0, -60.0),
        sample(4, 0.0, 0.0, -20.0),
    ];
    let median = model_with_aggregation(samples.clone(), AggregateMethod::MedianDbm);
    assert_eq!(value(&estimate(&median, 0.0, 0.0)), -65.0);
    let trimmed = model_with_aggregation(
        samples.clone(),
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.25).unwrap(),
        },
    );
    assert_eq!(value(&estimate(&trimmed, 0.0, 0.0)), -65.0);
    let linear = model_with_aggregation(
        vec![sample(1, 0.0, 0.0, -80.0), sample(2, 0.0, 0.0, -70.0)],
        AggregateMethod::LinearPowerMean,
    );
    assert!((value(&estimate(&linear, 0.0, 0.0)) + 72.596_373_105_057_56).abs() < 1e-12);
    let range = model_with_aggregation(
        samples,
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.25).unwrap(),
            upper: Probability::new(0.75).unwrap(),
        },
    );
    assert_eq!(value(&estimate(&range, 0.0, 0.0)), -65.0);
    let interval = &range.groups()[0].signal_aggregate.percentile_interval;
    assert_eq!(
        interval
            .as_known()
            .map(|range| (range.lower.get(), range.upper.get())),
        Some((-72.5, -50.0))
    );
}

#[test]
fn selected_static_aggregation_is_permutation_deterministic() {
    let samples = vec![
        sample(1, 0.0, 0.0, -80.0),
        sample(2, 0.0, 0.0, -70.0),
        sample(3, 0.0, 0.0, -60.0),
        sample(4, 0.0, 0.0, -20.0),
    ];
    let methods = [
        AggregateMethod::MedianDbm,
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.25).unwrap(),
        },
        AggregateMethod::LinearPowerMean,
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.25).unwrap(),
            upper: Probability::new(0.75).unwrap(),
        },
    ];
    for method in methods {
        let forward = model_with_aggregation(samples.clone(), method);
        let reverse = model_with_aggregation(samples.iter().rev().cloned().collect(), method);
        assert_eq!(forward.groups(), reverse.groups());
        assert_eq!(estimate(&forward, 0.0, 0.0), estimate(&reverse, 0.0, 0.0));
    }
}

#[test]
fn temporal_aggregation_is_rejected_without_ordered_sample_evidence() {
    for method in [
        AggregateMethod::EwmaDbm {
            alpha: Probability::new(0.5).unwrap(),
        },
        AggregateMethod::RobustStateSpaceDbm {
            process_stddev: Db::new(1.0).unwrap(),
            measurement_stddev: Db::new(2.0).unwrap(),
            huber_threshold_stddevs: Dimensionless::new(1.5).unwrap(),
        },
    ] {
        assert!(matches!(
            Model::new(
                inputs_with_aggregation(vec![sample(1, 0.0, 0.0, -60.0)], method),
                config()
            ),
            Err(Error::TemporalAggregationRequiresMonotonicEvidence)
        ));
    }
}

#[test]
fn malformed_typed_aggregation_configuration_is_rejected() {
    let mut json =
        serde_json::to_value(SignalAggregationSelection::new(AggregateMethod::MedianDbm)).unwrap();
    json["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SignalAggregationSelection>(json).is_err());
    assert!(
        serde_json::from_str::<SignalAggregationSelection>(
            r#"{"algorithm_version":"kyberia-wifi-signal/2","method":{"method":"median_dbm"}}"#
        )
        .is_err()
    );
    let mut config_json = serde_json::to_value(config()).unwrap();
    config_json["signal_aggregation"] = serde_json::json!({
        "algorithm_version": "kyberia-wifi-signal/1",
        "method": {"method": "median_dbm"}
    });
    assert!(serde_json::from_value::<Config>(config_json).is_err());
}

#[test]
fn metric_definition_binding_round_trips_only_verified_canonical_bytes() {
    let selection = SignalAggregationSelection::new(AggregateMethod::LinearPowerMean);
    let definition =
        SignalMetricDefinition::new(Text::new("test/linear-power-rssi/1").unwrap(), selection)
            .unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let binding = definition
        .bind(artifact_for(definition.version(), &bytes))
        .unwrap();
    assert_eq!(binding.signal_aggregation(), selection);
    assert_eq!(binding.artifact().version, *definition.version());
    assert_eq!(binding.canonical_bytes().unwrap(), bytes);

    let mut mutated = bytes.clone();
    let mutation = mutated
        .iter_mut()
        .find(|byte| **byte == b'1')
        .expect("canonical schema has a version digit");
    *mutation = b'2';
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(definition.version(), &bytes),
            &mutated,
            selection,
        ),
        Err(MetricDefinitionError::ArtifactHashMismatch)
    );
}

#[test]
fn metric_definition_binding_rejects_method_mismatch_and_untrusted_wire_forms() {
    let linear = SignalMetricDefinition::new(
        Text::new("test/linear-power-rssi/1").unwrap(),
        SignalAggregationSelection::new(AggregateMethod::LinearPowerMean),
    )
    .unwrap();
    let linear_bytes = linear.canonical_bytes().unwrap();
    let median = SignalAggregationSelection::new(AggregateMethod::MedianDbm);
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(linear.version(), &linear_bytes),
            &linear_bytes,
            median,
        ),
        Err(MetricDefinitionError::SelectionMismatch)
    );

    let mut wrong_length = artifact_for(linear.version(), &linear_bytes);
    wrong_length.byte_length = ExactU64::new(linear_bytes.len() as u64 + 1);
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            wrong_length,
            &linear_bytes,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::ArtifactLengthMismatch)
    );
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(&Text::new("test/other-rssi/1").unwrap(), &linear_bytes),
            &linear_bytes,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::ArtifactVersionMismatch)
    );
    let mut wrong_media_type = artifact_for(linear.version(), &linear_bytes);
    wrong_media_type.media_type = Text::new("application/octet-stream").unwrap();
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            wrong_media_type,
            &linear_bytes,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::ArtifactMediaTypeMismatch)
    );

    let duplicate = String::from_utf8(linear_bytes.clone()).unwrap().replacen(
        ",\"signal_aggregation\":",
        ",\"version\":\"test/linear-power-rssi/1\",\"signal_aggregation\":",
        1,
    );
    let duplicate = duplicate.into_bytes();
    assert!(matches!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(linear.version(), &duplicate),
            &duplicate,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::NonCanonicalBytes | MetricDefinitionError::MalformedBytes)
    ));

    let pretty = serde_json::to_vec_pretty(
        &serde_json::from_slice::<serde_json::Value>(&linear_bytes).unwrap(),
    )
    .unwrap();
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(linear.version(), &pretty),
            &pretty,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::NonCanonicalBytes)
    );

    let unknown = String::from_utf8(linear_bytes.clone()).unwrap().replacen(
        ",\"signal_aggregation\":",
        ",\"unexpected\":true,\"signal_aggregation\":",
        1,
    );
    let unknown = unknown.into_bytes();
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(linear.version(), &unknown),
            &unknown,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::MalformedBytes)
    );

    let future = String::from_utf8(linear_bytes.clone())
        .unwrap()
        .replacen(
            "kyberia.signal-metric-definition/1",
            "kyberia.signal-metric-definition/2",
            1,
        )
        .into_bytes();
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(linear.version(), &future),
            &future,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::MalformedBytes)
    );

    let malformed = linear_bytes[..linear_bytes.len() - 1].to_vec();
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(linear.version(), &malformed),
            &malformed,
            linear.signal_aggregation(),
        ),
        Err(MetricDefinitionError::MalformedBytes)
    );
}

#[test]
fn metric_definition_binding_enforces_byte_and_depth_limits() {
    let version = Text::new("test/bounded-rssi/1").unwrap();
    let selection = SignalAggregationSelection::new(AggregateMethod::MedianDbm);
    let oversized = vec![b' '; MAX_SIGNAL_METRIC_DEFINITION_BYTES + 1];
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(&version, &oversized),
            &oversized,
            selection,
        ),
        Err(MetricDefinitionError::ResourceLimit(
            "signal metric definition bytes"
        ))
    );
    let deeply_nested = format!(
        "{}null{}",
        "[".repeat(MAX_SIGNAL_METRIC_DEFINITION_DEPTH + 1),
        "]".repeat(MAX_SIGNAL_METRIC_DEFINITION_DEPTH + 1)
    )
    .into_bytes();
    assert_eq!(
        MetricDefinitionBinding::from_artifact_bytes(
            artifact_for(&version, &deeply_nested),
            &deeply_nested,
            selection,
        ),
        Err(MetricDefinitionError::ResourceLimit(
            "signal metric definition depth"
        ))
    );
}

#[test]
fn nearest_ties_and_neighbor_truncation_are_deterministic() {
    let mut cfg = config();
    cfg.method = Method::Nearest;
    cfg.maximum_neighbors = 1;
    let m = Model::new(
        inputs(vec![sample(1, 2.0, 0.0, -80.0), sample(2, 0.0, 0.0, -40.0)]),
        cfg,
    )
    .unwrap();
    assert_eq!(value(&estimate(&m, 1.0, 0.0)), -40.0);
    cfg.method = Method::Idw { power: 2.0 };
    let m = Model::new(m.inputs().clone(), cfg).unwrap();
    assert_eq!(value(&estimate(&m, 1.0, 0.0)), -40.0);
}
#[test]
fn rejection_of_ambiguous_scope_duplicate_ids_and_invalid_policy() {
    let s = sample(1, 0.0, 0.0, -40.0);
    assert!(matches!(
        Model::new(inputs(vec![s.clone(), s.clone()]), config()),
        Err(Error::DuplicateObservation(_))
    ));
    let mut wrong = s.clone();
    wrong.floor_id = FloorId::from_bytes([9; 16]).unwrap();
    assert!(matches!(
        Model::new(inputs(vec![wrong]), config()),
        Err(Error::FloorMismatch)
    ));
    let mut wrong = s;
    wrong.frame_id = FrameId::from_bytes([9; 16]).unwrap();
    assert!(matches!(
        Model::new(inputs(vec![wrong]), config()),
        Err(Error::FrameMismatch)
    ));
    for power in [0.0, -1.0, f64::NAN, f64::INFINITY, 65.0] {
        let mut cfg = config();
        cfg.method = Method::Idw { power };
        assert!(cfg.validate().is_err());
    }
    for (min, max) in [(0, 8), (9, 8), (1, 65)] {
        let mut cfg = config();
        cfg.minimum_locations = min;
        cfg.maximum_neighbors = max;
        assert!(cfg.validate().is_err());
    }
}
#[test]
fn extreme_finite_values_and_distances_remain_honest() {
    let m = model(vec![
        sample(1, 0.0, 0.0, f64::MAX),
        sample(2, 0.0, 0.0, -f64::MAX),
    ]);
    assert_eq!(value(&estimate(&m, 0.0, 0.0)), 0.0);
    let m = model(vec![
        sample(1, -1.0, 0.0, f64::MAX),
        sample(2, 1.0, 0.0, f64::MAX),
    ]);
    assert_eq!(value(&estimate(&m, 0.0, 0.0)), f64::MAX);
    let m = model(vec![sample(1, -f64::MAX, 0.0, -40.0)]);
    let c = estimate(&m, f64::MAX, 0.0);
    assert_eq!(c.class, CellClass::Unknown);
    assert!(c.nearest_distance.as_known().is_none());
    let m = model(vec![sample(1, 0.0, 0.0, -40.0), sample(2, 1.0, 0.0, -80.0)]);
    let c = estimate(&m, f64::from_bits(1), 0.0);
    assert_eq!(value(&c), -40.0);
    assert_eq!(c.class, CellClass::Interpolated);
}
#[test]
fn affine_symmetric_and_radial_independent_truth() {
    // The symmetric corner average reproduces the affine field -60 + 2x - y at its center.
    let m = model(vec![
        sample(1, -1.0, -1.0, -61.0),
        sample(2, -1.0, 1.0, -63.0),
        sample(3, 1.0, -1.0, -57.0),
        sample(4, 1.0, 1.0, -59.0),
    ]);
    assert!((value(&estimate(&m, 0.0, 0.0)) + 60.0).abs() < 1e-12);
    // Radial field -40 - 10r, four r=1 samples; interpolation center is biased by -10 dB.
    let m = model(vec![
        sample(1, -1.0, 0.0, -50.0),
        sample(2, 1.0, 0.0, -50.0),
        sample(3, 0.0, -1.0, -50.0),
        sample(4, 0.0, 1.0, -50.0),
    ]);
    let center = estimate(&m, 0.0, 0.0);
    assert_eq!(value(&center) - (-40.0), -10.0);
    assert!(center.uncertainty_db.as_known().is_none());
}
#[test]
fn tile_serialization_retains_numeric_unknown_and_provenance() {
    let m = model_with_aggregation(
        vec![sample(1, 0.5, 0.5, -40.0)],
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.25).unwrap(),
        },
    );
    let t = m.tile(grid(10, 1), || false).unwrap();
    assert_eq!(t.cells.len(), 10);
    assert_eq!(
        t.signal_aggregation(),
        m.inputs().metric_definition.signal_aggregation()
    );
    assert_eq!(t.cells[0].class, CellClass::Observed);
    assert_eq!(t.cells[9].class, CellClass::Unknown);
    let json = serde_json::to_value(t).unwrap();
    assert_eq!(json["cells"][9]["value"]["state"], "unknown");
    assert_eq!(
        json["signal_aggregation"],
        json["inputs"]["metric_definition"]["signal_aggregation"]
    );
    assert_eq!(
        json["signal_aggregation"]["algorithm_version"],
        "kyberia-wifi-signal/1"
    );
    assert_eq!(
        json["signal_aggregation"]["method"]["method"],
        "trimmed_mean_dbm"
    );
    assert_eq!(json["signal_aggregation"]["method"]["trim_each_tail"], 0.25);
    assert_eq!(
        json["location_groups"][0]["signal_aggregate"]["observation_order"][0],
        "00000000000000000000000000000001"
    );
    assert_eq!(
        json["location_groups"][0]["observation_ids"][0],
        "00000000000000000000000000000001"
    );
}
#[test]
fn independently_tiled_grid_has_identical_boundaries() {
    let m = model(vec![sample(1, 0.0, 0.0, -40.0), sample(2, 4.0, 0.0, -80.0)]);
    let full = m.tile(grid(8, 2), || false).unwrap();
    let left = m.tile(grid(4, 2), || false).unwrap();
    let mut right_grid = grid(4, 2);
    right_grid.column_offset = 4;
    let right = m.tile(right_grid, || false).unwrap();
    for row in 0..2 {
        assert_eq!(
            full.cells[row * 8..row * 8 + 4],
            left.cells[row * 4..row * 4 + 4]
        );
        assert_eq!(
            full.cells[row * 8 + 4..row * 8 + 8],
            right.cells[row * 4..row * 4 + 4]
        );
    }
}
#[test]
fn cancellation_and_resource_limits_are_structured_errors() {
    let m = model((1..=200).map(|i| sample(i, i as f64, 0.0, -40.0)).collect());
    assert_eq!(m.tile(grid(1, 1), || true), Err(Error::Cancelled));
    let mut calls = 0;
    assert_eq!(
        m.estimate(point(0.0, 0.0), &mut || {
            calls += 1;
            calls == 2
        }),
        Err(Error::Cancelled)
    );
    assert_eq!(calls, 2);
    assert!(grid(1001, 100).validate().is_err());
    let m = model(
        (1..=1001)
            .map(|i| sample(i, i as f64, 0.0, -40.0))
            .collect(),
    );
    assert!(matches!(
        m.tile(grid(1000, 100), || false),
        Err(Error::ResourceLimit(_))
    ));
    let mut g = grid(2, 1);
    g.origin = point(f64::MAX, 0.0);
    assert!(g.validate().is_err());
    g.resolution = Meters::new(f64::MAX).unwrap();
    assert!(g.validate().is_err());
}
#[test]
fn grid_rejects_collapsed_interior_centers_even_when_endpoints_differ() {
    let mut g = grid(4, 1);
    g.origin = point(-9007199254740992.0, 0.0);
    assert!(matches!(g.validate(), Err(Error::NumericalFailure(_))));
}
proptest! {
    #[test] fn convex_bound_and_permutation(a in -200.0_f64..100.0,b in -200.0_f64..100.0,x in 0.001_f64..3.999) {
        let samples=vec![sample(1,0.0,0.0,a),sample(2,4.0,0.0,b)];let forward=model(samples.clone());let reverse=model(samples.into_iter().rev().collect());let cell=estimate(&forward,x,0.0);prop_assert!(value(&cell)>=a.min(b)&&value(&cell)<=a.max(b));prop_assert_eq!(cell,estimate(&reverse,x,0.0));
    }
    #[test] fn translation_invariance(dx in -1000_i32..1000,dy in -1000_i32..1000) {
        let base=model(vec![sample(1,0.0,0.0,-40.0),sample(2,4.0,0.0,-80.0)]);let shifted=model(vec![sample(1,dx as f64,dy as f64,-40.0),sample(2,dx as f64+4.0,dy as f64,-80.0)]);prop_assert_eq!(estimate(&base,1.0,0.0),estimate(&shifted,dx as f64+1.0,dy as f64));
    }
}

#[test]
#[ignore = "explicit release performance benchmark"]
fn benchmark_tiles() {
    let m = model(
        (1..=100)
            .map(|i| {
                sample(
                    i,
                    ((i - 1) % 10) as f64 * 10.0,
                    ((i - 1) / 10) as f64 * 10.0,
                    -40.0 - i as f64 / 10.0,
                )
            })
            .collect(),
    );
    for (width, height) in [(100, 100), (1000, 100)] {
        let start = std::time::Instant::now();
        let tile = m.tile(grid(width, height), || false).unwrap();
        let elapsed = start.elapsed();
        let known = tile
            .cells
            .iter()
            .filter(|c| c.value.as_known().is_some())
            .count();
        println!(
            "cells={} input_locations=100 neighbor_limit=8 radius_m=5 resolution_m=1 elapsed_ms={:.3} known={} unknown={}",
            tile.cells.len(),
            elapsed.as_secs_f64() * 1000.0,
            known,
            tile.cells.len() - known
        );
        std::hint::black_box(tile);
    }
    let mut dense_config = config();
    dense_config.support_radius = Meters::new(15.0).unwrap();
    let dense = Model::new(m.inputs().clone(), dense_config).unwrap();
    let mut dense_grid = grid(1000, 100);
    dense_grid.resolution = Meters::new(0.1).unwrap();
    let start = std::time::Instant::now();
    let tile = dense.tile(dense_grid, || false).unwrap();
    println!(
        "dense cells=100000 input_locations=100 neighbor_limit=8 radius_m=15 resolution_m=0.1 elapsed_ms={:.3} known={}",
        start.elapsed().as_secs_f64() * 1000.0,
        tile.cells
            .iter()
            .filter(|c| c.value.as_known().is_some())
            .count()
    );
    std::hint::black_box(tile);
    for count in [10_000, 100_000] {
        let start = std::time::Instant::now();
        let m = model(
            (1..=count)
                .map(|i| {
                    sample(
                        i,
                        ((i - 1) % 1000) as f64,
                        ((i - 1) / 1000) as f64,
                        -40.0 - (i % 60) as f64,
                    )
                })
                .collect(),
        );
        let build_ms = start.elapsed().as_secs_f64() * 1000.0;
        let start = std::time::Instant::now();
        let tile = m.tile(grid(10, 10), || false).unwrap();
        println!(
            "cells=100 input_locations={count} neighbor_limit=8 radius_m=5 resolution_m=1 build_ms={build_ms:.3} elapsed_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        std::hint::black_box(tile);
    }
}
