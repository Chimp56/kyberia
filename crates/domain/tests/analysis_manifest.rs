use kyberia_domain::{
    analysis::*,
    identity::{ContentHash, FloorId, FrameId, SessionId, SnapshotId, Text},
    units::{CoordinateMeters, Dimensionless, Meters},
};
use proptest::prelude::*;

fn text(s: &str) -> Text {
    Text::new(s).unwrap()
}
fn artifact(n: u8) -> VersionedArtifact {
    VersionedArtifact {
        version: text("1"),
        sha256: ContentHash::from_sha256([n; 32]),
        byte_length: ExactU64::new(100),
        media_type: text("application/json"),
    }
}
fn spec() -> ManifestSpec {
    ManifestSpec {
        schema: ManifestVersion::V1,
        surveys: (1..=2)
            .map(|n| SurveyInput {
                session_id: SessionId::from_bytes([n; 16]).unwrap(),
                snapshot_id: SnapshotId::from_bytes([n + 2; 16]).unwrap(),
                snapshot: artifact(n),
            })
            .collect(),
        identity_graph: artifact(3),
        geometry: artifact(4),
        metric_definition: artifact(5),
        client_profile: ClientProfile::NotApplicable {},
        selection_policy: artifact(6),
        algorithm: Algorithm {
            name: text("idw"),
            version: text("kyberia-spatial/1"),
            implementation: artifact(7),
            execution_profile: artifact(8),
            parameters: vec![
                Parameter {
                    name: text("power"),
                    value: ParameterValue::Dimensionless(Dimensionless::new(2.0).unwrap()),
                },
                Parameter {
                    name: text("neighbors"),
                    value: ParameterValue::Count(ExactU64::new(8)),
                },
            ],
            randomness: Randomness::Seeded {
                seed: ExactU64::new(u64::MAX),
            },
        },
        output_grid: OutputGrid {
            floor_id: FloorId::from_bytes([1; 16]).unwrap(),
            frame_id: FrameId::from_bytes([2; 16]).unwrap(),
            origin_x_m: CoordinateMeters::new(-10.0).unwrap(),
            origin_y_m: CoordinateMeters::new(2.0).unwrap(),
            elevation_m: CoordinateMeters::new(3.0).unwrap(),
            resolution_m: Meters::new(0.5).unwrap(),
            columns: 20,
            rows: 10,
            area_mask: artifact(9),
        },
    }
}

#[test]
fn canonical_v1_matches_independently_encoded_golden() {
    let manifest = AnalysisManifest::new(spec()).unwrap();
    assert_eq!(
        manifest.canonical_json(),
        include_bytes!("fixtures/analysis-manifest-v1.json")
    );
    assert_eq!(
        String::from(manifest.sha256()),
        "ae595b6518430dd7ffc8424aadb16678b0b2b076ed063397bc48d1c2c69539f0"
    );
}

#[test]
fn canonical_sets_and_wire_order_are_independent() {
    let original = AnalysisManifest::new(spec()).unwrap();
    let mut reordered = spec();
    reordered.surveys.reverse();
    reordered.algorithm.parameters.reverse();
    assert_eq!(original, AnalysisManifest::new(reordered).unwrap());
    let value: serde_json::Value = serde_json::from_slice(original.canonical_json()).unwrap();
    let pretty = serde_json::to_vec_pretty(&value).unwrap();
    assert_eq!(original, AnalysisManifest::from_json(&pretty).unwrap());
    assert!(
        String::from_utf8_lossy(original.canonical_json()).contains("\"18446744073709551615\"")
    );
}

#[test]
fn every_cache_relevant_input_invalidates_the_hash() {
    let original = AnalysisManifest::new(spec()).unwrap();
    let changes: Vec<fn(&mut ManifestSpec)> = vec![
        |s| s.surveys[0].snapshot = artifact(20),
        |s| s.surveys[0].snapshot_id = SnapshotId::from_bytes([20; 16]).unwrap(),
        |s| s.identity_graph = artifact(20),
        |s| s.geometry = artifact(20),
        |s| s.metric_definition = artifact(20),
        |s| s.selection_policy = artifact(20),
        |s| {
            s.client_profile = ClientProfile::Pinned {
                artifact: artifact(20),
            }
        },
        |s| s.algorithm.version = text("new-algorithm"),
        |s| s.algorithm.implementation = artifact(20),
        |s| s.algorithm.execution_profile = artifact(20),
        |s| {
            s.algorithm.randomness = Randomness::Seeded {
                seed: ExactU64::new(u64::MAX - 1),
            }
        },
        |s| {
            s.algorithm.parameters[0].value =
                ParameterValue::Dimensionless(Dimensionless::new(3.0).unwrap())
        },
        |s| s.output_grid.frame_id = FrameId::from_bytes([20; 16]).unwrap(),
        |s| s.output_grid.floor_id = FloorId::from_bytes([20; 16]).unwrap(),
        |s| s.output_grid.origin_x_m = CoordinateMeters::new(-9.0).unwrap(),
        |s| s.output_grid.elevation_m = CoordinateMeters::new(4.0).unwrap(),
        |s| s.output_grid.resolution_m = Meters::new(0.25).unwrap(),
        |s| s.output_grid.columns = 21,
        |s| s.output_grid.rows = 11,
        |s| s.output_grid.area_mask = artifact(20),
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut changed = spec();
        change(&mut changed);
        assert_ne!(
            original.sha256(),
            AnalysisManifest::new(changed).unwrap().sha256(),
            "mutation {index}"
        );
    }
}

#[test]
fn equivalent_numeric_zero_has_one_encoding() {
    let mut positive = spec();
    positive.output_grid.origin_x_m = CoordinateMeters::new(0.0).unwrap();
    let mut negative = positive.clone();
    negative.output_grid.origin_x_m = CoordinateMeters::new(-0.0).unwrap();
    assert_eq!(
        AnalysisManifest::new(positive).unwrap(),
        AnalysisManifest::new(negative).unwrap()
    );
}

#[test]
fn duplicate_parameters_and_session_snapshots_are_rejected() {
    let mut input = spec();
    input
        .algorithm
        .parameters
        .push(input.algorithm.parameters[0].clone());
    assert!(AnalysisManifest::new(input).is_err());
    let mut input = spec();
    input.surveys[1].session_id = input.surveys[0].session_id;
    assert!(AnalysisManifest::new(input).is_err());
}

#[test]
fn unknown_duplicate_truncated_and_future_json_is_rejected() {
    let manifest = AnalysisManifest::new(spec()).unwrap();
    let json = std::str::from_utf8(manifest.canonical_json()).unwrap();
    for invalid in [
        format!("{{\"extra\":0,{}", &json[1..]),
        json.replacen(
            "\"version\":\"1\"",
            "\"version\":\"1\",\"version\":\"2\"",
            1,
        ),
        json.replacen("\"version\":\"1\"", "\"version\":\"1\",\"extra\":0", 1),
        json.replace("kyberia-spatial-analysis/1", "kyberia-spatial-analysis/2"),
        json[..json.len() - 1].to_owned(),
        format!("{json} true"),
        json.replace(
            "\"seed\":\"18446744073709551615\"",
            "\"seed\":18446744073709551615",
        ),
        json.replace("\"not_applicable\"", "\"not_applicable\",\"extra\":0"),
    ] {
        assert!(
            AnalysisManifest::from_json(invalid.as_bytes()).is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn exact_integer_parser_rejects_noncanonical_or_overflow_values() {
    for s in [
        "",
        "01",
        "+1",
        "-0",
        "1.0",
        "1e1",
        " 1",
        "18446744073709551616",
    ] {
        assert!(ExactU64::try_from(s.to_owned()).is_err(), "{s}");
    }
}

#[test]
fn tiny_float_regression_and_deterministic_variant_remain_exact() {
    let mut s = spec();
    s.algorithm.randomness = Randomness::Deterministic {};
    s.algorithm.parameters[0].value =
        ParameterValue::Dimensionless(Dimensionless::new(-2.5295671164902154e-213).unwrap());
    let manifest = AnalysisManifest::new(s).unwrap();
    assert_eq!(
        manifest,
        AnalysisManifest::from_json(manifest.canonical_json()).unwrap()
    );
    let invalid = std::str::from_utf8(manifest.canonical_json())
        .unwrap()
        .replace("\"deterministic\"", "\"deterministic\",\"seed\":\"42\"");
    assert!(AnalysisManifest::from_json(invalid.as_bytes()).is_err());
}

#[test]
fn grid_bounds_and_floating_point_collapse_are_rejected() {
    let changes: Vec<fn(&mut OutputGrid)> = vec![
        |g| g.columns = 0,
        |g| g.rows = 0,
        |g| g.resolution_m = Meters::new(0.0).unwrap(),
        |g| {
            g.columns = u32::MAX;
            g.rows = u32::MAX;
        },
        |g| g.origin_x_m = CoordinateMeters::new(1e30).unwrap(),
        |g| g.resolution_m = Meters::new(f64::MAX).unwrap(),
        |g| {
            g.origin_x_m = CoordinateMeters::new(1.0).unwrap();
            g.resolution_m = Meters::new(f64::EPSILON).unwrap();
        },
    ];
    for change in changes {
        let mut s = spec();
        change(&mut s.output_grid);
        assert!(AnalysisManifest::new(s).is_err());
    }
}

#[test]
fn manifest_and_collection_limits_are_enforced() {
    assert!(AnalysisManifest::from_json(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
    let mut s = spec();
    s.surveys.clear();
    assert!(AnalysisManifest::new(s).is_err());
    let mut s = spec();
    s.surveys = vec![s.surveys[0].clone(); MAX_INPUTS + 1];
    assert!(AnalysisManifest::new(s).is_err());
    let mut s = spec();
    s.algorithm.parameters = vec![s.algorithm.parameters[0].clone(); MAX_PARAMETERS + 1];
    assert!(AnalysisManifest::new(s).is_err());
}

#[test]
fn artifact_bytes_and_length_must_both_match() {
    let mut reference = artifact(0);
    reference.sha256 = ContentHash::try_from(
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
    )
    .unwrap();
    reference.byte_length = ExactU64::new(3);
    assert!(AnalysisManifest::verify_artifact(&reference, b"abc"));
    assert!(!AnalysisManifest::verify_artifact(&reference, b"abd"));
    reference.byte_length = ExactU64::new(4);
    assert!(!AnalysisManifest::verify_artifact(&reference, b"abc"));
}

proptest! {
    #[test]
    fn accepted_grid_centers_are_strictly_ordered(exponent in -500_i32..500, mantissa in -2.0_f64..2.0, precision in 0_i32..60, columns in 1_u32..100) {
        let mut s=spec();
        s.output_grid.origin_x_m=CoordinateMeters::new(mantissa * 2.0_f64.powi(exponent)).unwrap();
        s.output_grid.origin_y_m=CoordinateMeters::new(0.0).unwrap();
        s.output_grid.resolution_m=Meters::new(2.0_f64.powi(exponent-52+precision)).unwrap();
        s.output_grid.columns=columns;
        if let Ok(manifest)=AnalysisManifest::new(s) {
            let grid=&manifest.spec().output_grid;
            let mut previous=grid.origin_x_m.get();
            for column in 0..columns {
                let center=grid.origin_x_m.get()+(f64::from(column)+0.5)*grid.resolution_m.get();
                prop_assert!(center>previous);
                previous=center;
            }
            prop_assert!(previous<grid.origin_x_m.get()+f64::from(columns)*grid.resolution_m.get());
        }
    }

    #[test]
    fn finite_parameters_roundtrip_without_hash_drift(value in any::<f64>().prop_filter("finite", |x| x.is_finite()), seed in any::<u64>()) {
        let mut s=spec();
        s.algorithm.parameters[0].value=ParameterValue::Dimensionless(Dimensionless::new(value).unwrap());
        s.algorithm.randomness=Randomness::Seeded {seed: ExactU64::new(seed)};
        let manifest=AnalysisManifest::new(s).unwrap();
        let restored=AnalysisManifest::from_json(manifest.canonical_json()).unwrap();
        prop_assert_eq!(manifest,restored);
    }
}

#[test]
#[ignore = "explicit release manifest encoding/hash/decode benchmark"]
fn benchmark_manifest_roundtrip() {
    for count in [1_usize, 100, 1000, 4096] {
        let mut source = spec();
        source.surveys = (1..=count)
            .map(|n| {
                let bytes = (n as u128).to_be_bytes();
                SurveyInput {
                    session_id: SessionId::from_bytes(bytes).unwrap(),
                    snapshot_id: SnapshotId::from_bytes(bytes).unwrap(),
                    snapshot: artifact(1),
                }
            })
            .collect();
        let start = std::time::Instant::now();
        let manifest = AnalysisManifest::new(source).unwrap();
        let encode = start.elapsed();
        let start = std::time::Instant::now();
        let restored = AnalysisManifest::from_json(manifest.canonical_json()).unwrap();
        let decode = start.elapsed();
        assert_eq!(manifest, restored);
        println!(
            "surveys={count} bytes={} encode_hash_us={} decode_validate_hash_us={}",
            manifest.canonical_json().len(),
            encode.as_micros(),
            decode.as_micros()
        );
    }
}
