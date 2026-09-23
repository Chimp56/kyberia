use kyberia_antenna_model::{
    AntennaError, Direction3, JSON_SCHEMA_V1, MAX_JSON_BYTES, MAX_JSON_DEPTH, PolarizationBasis,
    PolarizationComponent, QuaternionWxyz, ValidatedAntenna, WorkBudget, WorkLimits,
};
use serde_json::{Value, json};

fn document() -> Value {
    serde_json::from_str(include_str!("fixtures/antenna-pattern-v1-valid.json")).unwrap()
}

fn parse(value: &Value) -> ValidatedAntenna {
    ValidatedAntenna::parse_json(&serde_json::to_vec(value).unwrap()).unwrap()
}

fn parse_error(value: &Value) -> AntennaError {
    ValidatedAntenna::parse_json(&serde_json::to_vec(value).unwrap()).unwrap_err()
}

fn direction(azimuth_degrees: f64, elevation_degrees: f64) -> Direction3 {
    let azimuth = azimuth_degrees.to_radians();
    let elevation = elevation_degrees.to_radians();
    Direction3 {
        x: elevation.cos() * azimuth.cos(),
        y: elevation.cos() * azimuth.sin(),
        z: elevation.sin(),
    }
}

#[test]
fn versioned_json_import_exposes_units_provenance_and_canonical_identity() {
    let antenna = parse(&document());
    assert_eq!(antenna.model_id(), "example/asymmetric-v1");
    assert_eq!(
        antenna.source_uri(),
        "urn:rf-atlas:test-pattern/asymmetric-v1"
    );
    assert_eq!(antenna.license_spdx(), "CC0-1.0");
    assert_eq!(antenna.source_checksum_sha256().len(), 64);
    assert_eq!(
        antenna.polarization_basis(),
        PolarizationBasis::LinearHorizontalVertical
    );
    assert_eq!(
        antenna.supported_frequency_hz().collect::<Vec<_>>(),
        [2_400_000_000, 5_000_000_000]
    );
    let metadata = antenna.frequency_metadata().next().unwrap();
    assert_eq!(metadata.frequency_hz, 2_400_000_000);
    assert_eq!(metadata.nominal_gain_dbi, 6.0);
    assert_eq!(metadata.nominal_gain_uncertainty_db, 0.0);
    assert_eq!(metadata.efficiency_fraction, 0.8);
    assert_eq!(metadata.efficiency_uncertainty, 0.1);
    let reparsed = ValidatedAntenna::parse_json(&antenna.canonical_json()).unwrap();
    assert_eq!(antenna.identity(), reparsed.identity());
    assert_eq!(antenna.identity().to_hex().len(), 64);
}

#[test]
fn coordinate_axes_mount_rotation_poles_and_asymmetric_sample_order_are_explicit() {
    let antenna = parse(&document());
    let x = antenna
        .evaluate_gain(
            2_400_000_000,
            direction(0.0, 0.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    assert!((x.gain_dbi - 6.0).abs() < 1.0e-12);
    assert!(x.local_azimuth_degrees.abs() < 1.0e-12);

    let y = antenna
        .evaluate_gain(
            2_400_000_000,
            direction(90.0, 0.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    assert!(y.gain_dbi.abs() < 1.0e-12);
    assert!((y.local_azimuth_degrees - 90.0).abs() < 1.0e-12);

    for elevation in [-90.0, 90.0] {
        let pole = antenna
            .evaluate_gain(
                2_400_000_000,
                direction(127.0, elevation),
                PolarizationComponent::CoPolar,
            )
            .unwrap();
        assert!((pole.gain_dbi - -10.0).abs() < 1.0e-12);
    }

    let half_turn = std::f64::consts::FRAC_PI_4;
    let mut rotated = document();
    rotated["mount_orientation_local_to_world"] = json!({
        "w": half_turn.cos(), "x": 0.0, "y": 0.0, "z": half_turn.sin()
    });
    let rotated = parse(&rotated);
    let world_y = rotated
        .evaluate_gain(
            2_400_000_000,
            direction(90.0, 0.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    assert!((world_y.gain_dbi - 6.0).abs() < 1.0e-12);
    assert!(world_y.local_azimuth_degrees.abs() < 1.0e-12);
}

#[test]
fn direction_elevation_and_frequency_interpolation_are_in_linear_power() {
    let antenna = parse(&document());
    let halfway_power = |left_dbi: f64, right_dbi: f64| {
        10.0 * ((10.0_f64.powf(left_dbi / 10.0) + 10.0_f64.powf(right_dbi / 10.0)) / 2.0).log10()
    };

    let azimuth_midpoint = antenna
        .evaluate_gain(
            2_400_000_000,
            direction(45.0, 0.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    assert!((azimuth_midpoint.gain_dbi - halfway_power(6.0, 0.0)).abs() < 1.0e-12);

    // Local +X is azimuth zero, and positive elevation points toward +Z.
    // At +45 degrees the lookup is halfway from the 0-degree +6 dBi sample
    // to the +90-degree pole's -10 dBi sample, in linear power.
    let elevation_midpoint = antenna
        .evaluate_gain(
            2_400_000_000,
            direction(0.0, 45.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    assert!(elevation_midpoint.local_azimuth_degrees.abs() < 1.0e-12);
    assert!((elevation_midpoint.local_elevation_degrees - 45.0).abs() < 1.0e-12);
    assert!((elevation_midpoint.gain_dbi - 3.0974422984797583).abs() < 1.0e-12);

    let seam_midpoint = antenna
        .evaluate_gain(
            2_400_000_000,
            direction(315.0, 0.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    assert!((seam_midpoint.gain_dbi - halfway_power(0.0, 6.0)).abs() < 1.0e-12);

    let frequency_midpoint = antenna
        .evaluate_gain(
            3_700_000_000,
            direction(0.0, 0.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    assert!((frequency_midpoint.gain_dbi - halfway_power(6.0, 0.0)).abs() < 1.0e-12);
    assert_eq!(frequency_midpoint.lower_frequency_hz, 2_400_000_000);
    assert_eq!(frequency_midpoint.upper_frequency_hz, 5_000_000_000);

    assert!(matches!(
        antenna.evaluate_gain(
            2_399_999_999,
            direction(0.0, 0.0),
            PolarizationComponent::CoPolar
        ),
        Err(AntennaError::Unsupported(_))
    ));
    assert!(matches!(
        antenna.evaluate_gain(
            5_000_000_001,
            direction(0.0, 0.0),
            PolarizationComponent::CoPolar
        ),
        Err(AntennaError::Unsupported(_))
    ));
}

#[test]
fn polarization_planes_are_separate_and_missing_cross_plane_is_not_invented() {
    let antenna = parse(&document());
    let co = antenna
        .evaluate_gain(
            2_400_000_000,
            direction(0.0, 0.0),
            PolarizationComponent::CoPolar,
        )
        .unwrap();
    let cross = antenna
        .evaluate_gain(
            2_400_000_000,
            direction(0.0, 0.0),
            PolarizationComponent::CrossPolar,
        )
        .unwrap();
    assert!((co.gain_dbi - 6.0).abs() < 1.0e-12);
    assert!((cross.gain_dbi - 3.0).abs() < 1.0e-12);

    let mut absent = document();
    for frequency in absent["frequencies"].as_array_mut().unwrap() {
        for sample in frequency["samples"].as_array_mut().unwrap() {
            sample
                .as_object_mut()
                .unwrap()
                .remove("cross_polar_gain_dbi");
        }
    }
    let no_cross = parse(&absent);
    let mut explicit_null = absent.clone();
    for frequency in explicit_null["frequencies"].as_array_mut().unwrap() {
        for sample in frequency["samples"].as_array_mut().unwrap() {
            sample["cross_polar_gain_dbi"] = Value::Null;
        }
    }
    assert_eq!(no_cross.identity(), parse(&explicit_null).identity());
    assert!(matches!(
        no_cross.evaluate_gain(
            2_400_000_000,
            direction(0.0, 0.0),
            PolarizationComponent::CrossPolar
        ),
        Err(AntennaError::Unsupported(_))
    ));

    let mut partial = document();
    partial["frequencies"][0]["samples"][4]
        .as_object_mut()
        .unwrap()
        .remove("cross_polar_gain_dbi");
    assert!(matches!(parse_error(&partial), AntennaError::Invalid(_)));
}

#[test]
fn signed_zero_and_antipodal_quaternions_have_canonical_identity() {
    let original = document();
    let identity = parse(&original).identity();
    let zero_fields = [
        "/azimuth_degrees/0",
        "/elevation_degrees/1",
        "/mount_orientation_local_to_world/x",
        "/mount_orientation_local_to_world/y",
        "/mount_orientation_local_to_world/z",
        "/normalization_tolerance_db",
        "/frequencies/1/nominal_gain/value_dbi",
        "/frequencies/1/nominal_gain/uncertainty_db",
        "/frequencies/1/efficiency/uncertainty",
        "/frequencies/0/samples/5/co_polar_gain_dbi",
    ];
    for pointer in zero_fields {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = json!(-0.0);
        assert_eq!(identity, parse(&changed).identity(), "{pointer}");
    }

    let mut antipodal = original;
    antipodal["mount_orientation_local_to_world"] = json!({
        "w": -1.0, "x": -0.0, "y": -0.0, "z": -0.0
    });
    assert_eq!(identity, parse(&antipodal).identity());
}

#[test]
fn invalid_axes_grid_metadata_and_unknown_fields_fail_closed() {
    let mut unknown = document();
    unknown["unversioned_extra"] = json!(true);
    assert!(matches!(
        parse_error(&unknown),
        AntennaError::InvalidJson(_)
    ));

    let mut wrong_order = document();
    wrong_order["sample_order"] = json!("azimuth_major_elevation_minor");
    assert!(matches!(
        parse_error(&wrong_order),
        AntennaError::InvalidJson(_)
    ));

    let mut unordered_axis = document();
    unordered_axis["azimuth_degrees"] = json!([0.0, 180.0, 90.0, 270.0]);
    assert!(matches!(
        parse_error(&unordered_axis),
        AntennaError::Invalid(_)
    ));

    let mut missing_pole = document();
    missing_pole["elevation_degrees"] = json!([-80.0, 0.0, 90.0]);
    assert!(matches!(
        parse_error(&missing_pole),
        AntennaError::Invalid(_)
    ));

    let mut bad_grid = document();
    bad_grid["frequencies"][0]["samples"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(matches!(parse_error(&bad_grid), AntennaError::Invalid(_)));

    let mut bad_pole = document();
    bad_pole["frequencies"][0]["samples"][1]["co_polar_gain_dbi"] = json!(-9.0);
    assert!(matches!(parse_error(&bad_pole), AntennaError::Invalid(_)));

    let mut bad_normalization = document();
    bad_normalization["frequencies"][0]["nominal_gain"]["value_dbi"] = json!(20.0);
    assert!(matches!(
        parse_error(&bad_normalization),
        AntennaError::Invalid(_)
    ));

    let mut duplicate_frequency = document();
    duplicate_frequency["frequencies"][1]["frequency_hz"] = json!(2_400_000_000_u64);
    assert!(matches!(
        parse_error(&duplicate_frequency),
        AntennaError::Invalid(_)
    ));

    let mut bad_efficiency = document();
    bad_efficiency["frequencies"][0]["efficiency"] = json!({
        "fraction": 0.95, "uncertainty": 0.1
    });
    assert!(matches!(
        parse_error(&bad_efficiency),
        AntennaError::Invalid(_)
    ));

    let mut non_unit = document();
    non_unit["mount_orientation_local_to_world"]["w"] = json!(0.5);
    assert!(matches!(parse_error(&non_unit), AntennaError::Invalid(_)));
}

#[test]
fn source_uri_checksum_and_spdx_expression_are_bounded_and_validated() {
    let mut bad_uri = document();
    bad_uri["source"]["source_uri"] = json!("https://example.invalid/a b");
    assert!(matches!(parse_error(&bad_uri), AntennaError::Invalid(_)));

    let mut bad_license = document();
    bad_license["source"]["license_spdx"] = json!("MIT OR");
    assert!(matches!(
        parse_error(&bad_license),
        AntennaError::Invalid(_)
    ));

    let mut valid_expression = document();
    valid_expression["source"]["license_spdx"] = json!("(MIT OR Apache-2.0) AND BSD-3-Clause");
    assert!(ValidatedAntenna::parse_json(&serde_json::to_vec(&valid_expression).unwrap()).is_ok());

    let mut bad_checksum = document();
    bad_checksum["source"]["source_checksum_sha256"] = json!("A".repeat(64));
    assert!(matches!(
        parse_error(&bad_checksum),
        AntennaError::Invalid(_)
    ));
}

#[test]
fn patterned_text_fields_reject_trailing_line_feeds() {
    for pointer in [
        "/model_id",
        "/source/license_spdx",
        "/source/source_uri",
        "/source/source_checksum_sha256",
    ] {
        let mut with_line_feed = document();
        let original = with_line_feed.pointer(pointer).unwrap().as_str().unwrap();
        *with_line_feed.pointer_mut(pointer).unwrap() = json!(format!("{original}\n"));
        assert!(
            matches!(
                parse_error(&with_line_feed),
                AntennaError::Invalid(_) | AntennaError::InvalidJson(_)
            ),
            "{pointer}"
        );
    }
}

#[test]
fn schema_is_valid_json_and_marks_cross_polar_data_optional() {
    let schema: Value = serde_json::from_str(JSON_SCHEMA_V1).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let sample = &schema["$defs"]["sample"];
    assert!(
        !sample["required"]
            .as_array()
            .unwrap()
            .contains(&json!("cross_polar_gain_dbi"))
    );
    assert_eq!(
        sample["properties"]["cross_polar_gain_dbi"]["oneOf"][1]["type"],
        "null"
    );
    assert_eq!(
        schema["properties"]["frequency_interpolation"]["const"],
        "linear_power_reject_outside"
    );
}

#[test]
fn input_depth_work_and_direction_limits_are_enforced() {
    let bytes = serde_json::to_vec(&document()).unwrap();
    let mut too_small = WorkBudget::new(WorkLimits { max_steps: 1 });
    assert!(matches!(
        ValidatedAntenna::parse_json_with_budget(&bytes, &mut too_small),
        Err(AntennaError::ResourceLimit(_))
    ));
    assert_eq!(too_small.usage().steps, 0);

    let oversized = vec![b' '; MAX_JSON_BYTES + 1];
    assert_eq!(
        ValidatedAntenna::parse_json(&oversized).unwrap_err(),
        AntennaError::InputTooLarge
    );
    let deeply_nested = format!(
        "{}0{}",
        "[".repeat(MAX_JSON_DEPTH + 1),
        "]".repeat(MAX_JSON_DEPTH + 1)
    );
    assert_eq!(
        ValidatedAntenna::parse_json(deeply_nested.as_bytes()).unwrap_err(),
        AntennaError::JsonDepthExceeded
    );

    let antenna = parse(&document());
    for magnitude in [f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1)] {
        let result = antenna
            .evaluate_gain(
                2_400_000_000,
                Direction3 {
                    x: magnitude,
                    y: 0.0,
                    z: 0.0,
                },
                PolarizationComponent::CoPolar,
            )
            .unwrap();
        assert!((result.gain_dbi - 6.0).abs() < 1.0e-12);
    }
    assert!(matches!(
        antenna.evaluate_gain(
            2_400_000_000,
            Direction3 {
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            PolarizationComponent::CoPolar
        ),
        Err(AntennaError::Invalid("zero direction"))
    ));
}

#[test]
fn quaternion_public_default_is_the_identity_mount() {
    assert_eq!(
        QuaternionWxyz::IDENTITY,
        QuaternionWxyz {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0
        }
    );
}
