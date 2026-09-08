use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{BssId, MldId, ObservationId, PhysicalDeviceId, RadioId},
    units::{Dbm, Megahertz, Milliwatts, Probability},
};
use kyberia_wifi_semantics::*;
use proptest::prelude::*;

fn id(value: u8) -> ObservationId {
    ObservationId::from_bytes([value; 16]).unwrap()
}
fn radio(value: u8) -> RadioId {
    RadioId::from_bytes([value; 16]).unwrap()
}
fn physical(value: u8) -> PhysicalDeviceId {
    PhysicalDeviceId::from_bytes([value; 16]).unwrap()
}
fn bss(value: u8) -> BssId {
    BssId::from_bytes([value; 16]).unwrap()
}
fn mld(value: u8) -> MldId {
    MldId::from_bytes([value; 16]).unwrap()
}
fn geometry(
    band: WifiBand,
    primary: u16,
    center: f64,
    width: ChannelWidth,
    puncturing: u16,
) -> ChannelGeometry {
    ChannelGeometry::new(
        band,
        primary,
        Megahertz::new(center).unwrap(),
        None,
        width,
        PuncturingMask::new(puncturing),
    )
    .unwrap()
}
fn identity(value: u8, bss_id: Option<BssId>, mld_id: Option<MldId>) -> RadioIdentity {
    RadioIdentity {
        radio_id: radio(value),
        physical_device_id: Some(physical(value)),
        bss_id,
        mld_id,
    }
}
fn input(
    input_id: u8,
    radio_id: u8,
    channel: ChannelGeometry,
    power_mw: f64,
    utilization: f64,
) -> InterfererInput {
    InterfererInput {
        input_id: id(input_id),
        identity: identity(radio_id, None, None),
        geometry: GeometryEvidence::Known(channel),
        received_power: Evidence::Known(if power_mw == 0.0 {
            LinearPower::Zero
        } else {
            LinearPower::Positive(Milliwatts::new(power_mw).unwrap())
        }),
        utilization: Evidence::Known(UtilizationEvidence::ScenarioAssumption(
            Probability::new(utilization).unwrap(),
        )),
    }
}
fn desired(channel: ChannelGeometry) -> DesiredChannel {
    DesiredChannel {
        identity: identity(250, Some(bss(1)), Some(mld(1))),
        geometry: GeometryEvidence::Known(channel),
    }
}

#[test]
fn stable_center_frequency_mappings_cover_24_5_and_6_ghz() {
    assert_eq!(
        channel_center_frequency(WifiBand::Ghz2_4, 1).unwrap().get(),
        2412.0
    );
    assert_eq!(
        channel_center_frequency(WifiBand::Ghz2_4, 14)
            .unwrap()
            .get(),
        2484.0
    );
    assert_eq!(
        channel_center_frequency(WifiBand::Ghz5, 36).unwrap().get(),
        5180.0
    );
    assert_eq!(
        channel_center_frequency(WifiBand::Ghz6, 1).unwrap().get(),
        5955.0
    );
    let channel14 = ChannelGeometry::from_primary(WifiBand::Ghz2_4, 14).unwrap();
    assert_eq!(channel14.occupied_subchannels()[0].channel, 14);
    assert!(matches!(
        channel_center_frequency(WifiBand::Ghz2_4, 200),
        Err(GeometryError::Unsupported(_))
    ));
    assert!(matches!(
        channel_center_frequency(WifiBand::Ghz5, 1),
        Err(GeometryError::Unsupported(_))
    ));
    assert!(matches!(
        channel_center_frequency(WifiBand::Ghz5, 68),
        Err(GeometryError::Unsupported(_))
    ));
    assert!(matches!(
        channel_center_frequency(WifiBand::Ghz6, 2),
        Err(GeometryError::Unsupported(_))
    ));
}

#[test]
fn bonded_widths_expose_segments_and_validate_primary_and_puncturing() {
    for (width, center, expected) in [
        (ChannelWidth::Mhz20, 5180.0, 1),
        (ChannelWidth::Mhz40, 5190.0, 2),
        (ChannelWidth::Mhz80, 5210.0, 4),
        (ChannelWidth::Mhz160, 5250.0, 8),
    ] {
        let value = geometry(WifiBand::Ghz5, 36, center, width, 0);
        assert_eq!(value.occupied_subchannels().len(), expected);
        assert_eq!(
            value.occupied_subchannels()[value.primary_segment_index()].channel,
            36
        );
    }
    let six = geometry(WifiBand::Ghz6, 1, 6105.0, ChannelWidth::Mhz320, 0b1100);
    assert_eq!(six.occupied_subchannels().len(), 16);
    assert!(six.occupied_subchannels()[2].punctured);
    assert!(six.occupied_subchannels()[3].punctured);
    assert!(!six.occupied_subchannels()[six.primary_segment_index()].punctured);

    let eighty_plus_eighty = ChannelGeometry::new(
        WifiBand::Ghz5,
        36,
        Megahertz::new(5210.0).unwrap(),
        Some(Megahertz::new(5530.0).unwrap()),
        ChannelWidth::Mhz80Plus80,
        PuncturingMask::NONE,
    )
    .unwrap();
    assert_eq!(eighty_plus_eighty.occupied_subchannels().len(), 8);
    assert_eq!(
        eighty_plus_eighty
            .occupied_subchannels()
            .iter()
            .filter(|segment| segment.punctured)
            .count(),
        0
    );

    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz5,
            52,
            Megahertz::new(5210.0).unwrap(),
            None,
            ChannelWidth::Mhz80,
            PuncturingMask::NONE,
        ),
        Err(GeometryError::Invalid(_))
    ));
    let valid_80_mask = geometry(WifiBand::Ghz5, 36, 5210.0, ChannelWidth::Mhz80, 0b0010);
    assert!(valid_80_mask.occupied_subchannels()[1].punctured);
    let valid_160_mask = geometry(WifiBand::Ghz5, 36, 5250.0, ChannelWidth::Mhz160, 0b1100);
    assert!(valid_160_mask.occupied_subchannels()[2].punctured);
    assert!(valid_160_mask.occupied_subchannels()[3].punctured);
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz5,
            36,
            Megahertz::new(5210.0).unwrap(),
            None,
            ChannelWidth::Mhz80,
            PuncturingMask::new(0b0011),
        ),
        Err(GeometryError::UnsupportedPuncturingPattern {
            width: ChannelWidth::Mhz80,
            mask: 0b0011,
        })
    ));
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz5,
            36,
            Megahertz::new(5250.0).unwrap(),
            None,
            ChannelWidth::Mhz160,
            PuncturingMask::new(0x88),
        ),
        Err(GeometryError::UnsupportedPuncturingPattern {
            width: ChannelWidth::Mhz160,
            mask: 0x88,
        })
    ));
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz6,
            1,
            Megahertz::new(6105.0).unwrap(),
            None,
            ChannelWidth::Mhz320,
            PuncturingMask::new(0b10),
        ),
        Err(GeometryError::UnsupportedPuncturingPattern {
            width: ChannelWidth::Mhz320,
            mask: 0b10,
        })
    ));
    for (width, center) in [(ChannelWidth::Mhz20, 5180.0), (ChannelWidth::Mhz40, 5190.0)] {
        assert!(matches!(
            ChannelGeometry::new(
                WifiBand::Ghz5,
                36,
                Megahertz::new(center).unwrap(),
                None,
                width,
                PuncturingMask::new(1),
            ),
            Err(GeometryError::Invalid(_))
        ));
    }
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz5,
            36,
            Megahertz::new(5210.0).unwrap(),
            Some(Megahertz::new(5530.0).unwrap()),
            ChannelWidth::Mhz80Plus80,
            PuncturingMask::new(1),
        ),
        Err(GeometryError::UnsupportedPuncturingPattern {
            width: ChannelWidth::Mhz80Plus80,
            mask: 1,
        })
    ));
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz5,
            36,
            Megahertz::new(5210.0).unwrap(),
            None,
            ChannelWidth::Mhz80,
            PuncturingMask::new(1),
        ),
        Err(GeometryError::Invalid(_))
    ));
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz2_4,
            14,
            Megahertz::new(2484.0).unwrap(),
            None,
            ChannelWidth::Mhz40,
            PuncturingMask::NONE,
        ),
        Err(GeometryError::Invalid(_) | GeometryError::Unsupported(_))
    ));
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz2_4,
            1,
            Megahertz::new(2412.0).unwrap(),
            None,
            ChannelWidth::Mhz80,
            PuncturingMask::NONE,
        ),
        Err(GeometryError::Unsupported(_))
    ));
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz5,
            36,
            Megahertz::new(5180.0).unwrap(),
            None,
            ChannelWidth::Mhz320,
            PuncturingMask::NONE,
        ),
        Err(GeometryError::Unsupported(_))
    ));
}

#[test]
fn eighty_plus_eighty_requires_canonical_separated_centers() {
    let valid = ChannelGeometry::new(
        WifiBand::Ghz5,
        36,
        Megahertz::new(5210.0).unwrap(),
        Some(Megahertz::new(5530.0).unwrap()),
        ChannelWidth::Mhz80Plus80,
        PuncturingMask::NONE,
    )
    .unwrap();
    let channels: Vec<_> = valid
        .occupied_subchannels()
        .iter()
        .map(|segment| segment.channel)
        .collect();
    let mut unique = channels.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(channels.len(), unique.len());

    for (first, second) in [(5530.0, 5210.0), (5210.0, 5290.0), (5210.0, 5270.0)] {
        assert!(matches!(
            ChannelGeometry::new(
                WifiBand::Ghz5,
                36,
                Megahertz::new(first).unwrap(),
                Some(Megahertz::new(second).unwrap()),
                ChannelWidth::Mhz80Plus80,
                PuncturingMask::NONE,
            ),
            Err(GeometryError::Invalid(_)) | Err(GeometryError::Unsupported(_))
        ));
    }
}

#[test]
fn bonded_centers_are_standardized_and_serialized_canonically() {
    let almost_80 = ChannelGeometry::new(
        WifiBand::Ghz5,
        36,
        Megahertz::new(5210.000000005).unwrap(),
        None,
        ChannelWidth::Mhz80,
        PuncturingMask::NONE,
    )
    .unwrap();
    assert_eq!(almost_80.center_frequency().get(), 5210.0);
    let almost_80_plus_80 = ChannelGeometry::new(
        WifiBand::Ghz5,
        36,
        Megahertz::new(5210.000000005).unwrap(),
        Some(Megahertz::new(5530.000000005).unwrap()),
        ChannelWidth::Mhz80Plus80,
        PuncturingMask::NONE,
    )
    .unwrap();
    assert_eq!(almost_80_plus_80.center_frequency().get(), 5210.0);
    assert_eq!(
        almost_80_plus_80.second_center_frequency().unwrap().get(),
        5530.0
    );
    let almost_6_320 = ChannelGeometry::new(
        WifiBand::Ghz6,
        1,
        Megahertz::new(6105.000000005).unwrap(),
        None,
        ChannelWidth::Mhz320,
        PuncturingMask::NONE,
    )
    .unwrap();
    assert_eq!(almost_6_320.center_frequency().get(), 6105.0);
    let encoded = serde_json::to_value(&almost_80_plus_80).unwrap();
    assert_eq!(encoded["center_frequency"], 5210.0);
    assert_eq!(encoded["second_center_frequency"], 5530.0);

    for (width, center) in [(ChannelWidth::Mhz20, 5180.1), (ChannelWidth::Mhz80, 5210.1)] {
        assert!(matches!(
            ChannelGeometry::new(
                WifiBand::Ghz5,
                36,
                Megahertz::new(center).unwrap(),
                None,
                width,
                PuncturingMask::NONE,
            ),
            Err(GeometryError::Invalid(_)) | Err(GeometryError::Unsupported(_))
        ));
    }
    assert!(matches!(
        ChannelGeometry::new(
            WifiBand::Ghz5,
            36,
            Megahertz::new(5210.0).unwrap(),
            Some(Megahertz::new(5530.1).unwrap()),
            ChannelWidth::Mhz80Plus80,
            PuncturingMask::NONE,
        ),
        Err(GeometryError::Invalid(_)) | Err(GeometryError::Unsupported(_))
    ));
}

#[test]
fn malformed_and_future_geometry_wire_is_rejected() {
    let value = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let encoded = serde_json::to_value(&value).unwrap();
    assert_eq!(encoded["version"], "kyberia-wifi-channel/1");
    assert!(
        serde_json::from_value::<ChannelGeometry>({
            let mut object = encoded.as_object().unwrap().clone();
            object.insert("version".to_owned(), serde_json::json!("future"));
            serde_json::Value::Object(object)
        })
        .is_err()
    );
    assert!(
        serde_json::from_value::<ChannelGeometry>({
            let mut object = encoded.as_object().unwrap().clone();
            object.insert("future_field".to_owned(), serde_json::json!(true));
            serde_json::Value::Object(object)
        })
        .is_err()
    );
    assert!(serde_json::from_value::<ChannelWidth>(serde_json::json!("mhz_640")).is_err());
}

#[test]
fn coupling_has_exact_cochannel_symmetry_range_and_separation_behavior() {
    let same = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let adjacent = geometry(WifiBand::Ghz5, 40, 5200.0, ChannelWidth::Mhz20, 0);
    let separated = geometry(WifiBand::Ghz5, 44, 5220.0, ChannelWidth::Mhz20, 0);
    let one_24 = geometry(WifiBand::Ghz2_4, 1, 2412.0, ChannelWidth::Mhz20, 0);
    let two_24 = geometry(WifiBand::Ghz2_4, 5, 2432.0, ChannelWidth::Mhz20, 0);
    let far_24 = geometry(WifiBand::Ghz2_4, 13, 2472.0, ChannelWidth::Mhz20, 0);

    let exact = coupling(&same, &same, SpectralMaskMethod::Trapezoid20MhzReceiverV1);
    assert_eq!(exact, CouplingValue::Known(Probability::new(1.0).unwrap()));
    let left = coupling(
        &same,
        &adjacent,
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
    );
    let right = coupling(
        &adjacent,
        &same,
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
    );
    assert_eq!(left, right);
    let adjacent_value = match left {
        CouplingValue::Known(value) => value.get(),
        _ => panic!("known geometry must produce known coupling"),
    };
    assert!(adjacent_value > 0.0 && adjacent_value < 1.0);
    assert_eq!(
        coupling(
            &same,
            &separated,
            SpectralMaskMethod::Trapezoid20MhzReceiverV1
        ),
        CouplingValue::Known(Probability::new(0.0).unwrap())
    );
    assert!(matches!(
        coupling(&one_24, &two_24, SpectralMaskMethod::Trapezoid20MhzReceiverV1),
        CouplingValue::Known(value) if value.get() > 0.0
    ));
    assert_eq!(
        coupling(
            &one_24,
            &far_24,
            SpectralMaskMethod::Trapezoid20MhzReceiverV1
        ),
        CouplingValue::Known(Probability::new(0.0).unwrap())
    );

    let punctured = geometry(WifiBand::Ghz5, 36, 5210.0, ChannelWidth::Mhz80, 0b10);
    let unpunctured = geometry(WifiBand::Ghz5, 36, 5210.0, ChannelWidth::Mhz80, 0);
    assert!(matches!(
        coupling(&punctured, &unpunctured, SpectralMaskMethod::Trapezoid20MhzReceiverV1),
        CouplingValue::Known(value) if value.get() < 1.0
    ));
}

fn receiver_response_oracle(distance: f64) -> f64 {
    let distance = distance.abs();
    if distance <= 10.0 {
        1.0
    } else if distance < 15.0 {
        (15.0 - distance) / 5.0
    } else {
        0.0
    }
}

fn wide_to_narrow_oracle() -> f64 {
    let mut accepted = 0.0;
    for center in [5180.0, 5200.0, 5220.0, 5240.0] {
        for index in 0..20 {
            let frequency = center + (index as f64 + 0.5) - 10.0;
            accepted += receiver_response_oracle(frequency - 5180.0);
        }
    }
    accepted / 80.0
}

#[test]
fn coupling_uses_total_interferer_power_and_is_asymmetric_for_widths() {
    let narrow = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let wide = geometry(WifiBand::Ghz5, 36, 5210.0, ChannelWidth::Mhz80, 0);
    let narrow_into_wide = coupling(&wide, &narrow, SpectralMaskMethod::Trapezoid20MhzReceiverV1);
    let wide_into_narrow = coupling(&narrow, &wide, SpectralMaskMethod::Trapezoid20MhzReceiverV1);
    assert_eq!(
        narrow_into_wide,
        CouplingValue::Known(Probability::new(1.0).unwrap())
    );
    let expected = wide_to_narrow_oracle();
    let actual = match wide_into_narrow {
        CouplingValue::Known(value) => value.get(),
        other => panic!("unexpected coupling result: {other:?}"),
    };
    assert!((actual - expected).abs() <= 1e-12);
    assert!(actual < 0.5);
    assert_ne!(narrow_into_wide, wide_into_narrow);
}

#[test]
fn coupling_respects_punctured_receiver_segments() {
    let desired = geometry(WifiBand::Ghz5, 36, 5210.0, ChannelWidth::Mhz80, 0b0010);
    let interferer = geometry(WifiBand::Ghz5, 40, 5200.0, ChannelWidth::Mhz20, 0);
    assert_eq!(
        coupling(
            &desired,
            &interferer,
            SpectralMaskMethod::Trapezoid20MhzReceiverV1
        ),
        CouplingValue::Known(Probability::new(0.25).unwrap())
    );
}

#[test]
fn effective_interference_sums_linear_power_and_preserves_zero_vs_unknown() {
    let channel = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let d = desired(channel.clone());
    let known = input(1, 2, channel.clone(), 2.0, 0.25);
    let zero = input(2, 3, channel.clone(), 0.0, 1.0);
    let result = effective_interference(
        &d,
        &[known, zero],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        result.total.milliwatts,
        Evidence::Known(LinearPower::Positive(Milliwatts::new(0.5).unwrap()))
    );
    assert_eq!(
        result.total.dbm,
        Evidence::Known(Dbm::new(10.0 * 0.5f64.log10()).unwrap())
    );
    assert_eq!(result.contributions.len(), 2);
    assert!(matches!(
        result.contributions[1].effective_power,
        Evidence::Known(LinearPower::Zero)
    ));

    let mut unknown = input(4, 4, channel.clone(), 2.0, 1.0);
    unknown.received_power = Evidence::Unknown(UnknownReason::NotMeasured);
    let unknown_result = effective_interference(
        &d,
        &[unknown],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        unknown_result.total.milliwatts,
        Evidence::Unknown(UnknownReason::NotMeasured)
    );
    let mut unknown_zero_geometry = input(5, 5, channel.clone(), 0.0, 1.0);
    unknown_zero_geometry.geometry = GeometryEvidence::Unknown(UnknownReason::NotObservable);
    let known_zero = effective_interference(
        &d,
        &[unknown_zero_geometry],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        known_zero.total.milliwatts,
        Evidence::Unknown(UnknownReason::NotObservable)
    );
    assert_eq!(
        known_zero.contributions[0].geometry,
        GeometryEvidence::Unknown(UnknownReason::NotObservable)
    );

    let non_overlapping = input(
        6,
        6,
        geometry(WifiBand::Ghz5, 44, 5220.0, ChannelWidth::Mhz20, 0),
        2.0,
        1.0,
    );
    let exact_zero = effective_interference(
        &d,
        &[non_overlapping],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        exact_zero.total.milliwatts,
        Evidence::Known(LinearPower::Zero)
    );
    assert_eq!(
        exact_zero.contributions[0].disposition,
        ContributionDisposition::Excluded(ExclusionReason::NoSpectralCoupling)
    );
    assert!(matches!(
        &exact_zero.contributions[0].geometry,
        GeometryEvidence::Known(_)
    ));
}

#[test]
fn observation_set_completeness_controls_empty_and_partial_totals() {
    let channel = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let d = desired(channel.clone());
    let empty_measured = effective_interference(
        &d,
        &[],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        empty_measured.total.milliwatts,
        Evidence::Known(LinearPower::Zero)
    );
    assert_eq!(
        empty_measured.aggregate_status,
        InterferenceAggregateStatus::Complete
    );

    let empty_scenario = effective_interference(
        &d,
        &[],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteScenario,
    )
    .unwrap();
    assert_eq!(
        empty_scenario.total.milliwatts,
        Evidence::Known(LinearPower::Zero)
    );

    let empty_incomplete = effective_interference(
        &d,
        &[],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::Incomplete(UnknownReason::NotMeasured),
    )
    .unwrap();
    assert_eq!(
        empty_incomplete.total.milliwatts,
        Evidence::Unknown(UnknownReason::NotMeasured)
    );

    let partial = effective_interference(
        &d,
        &[input(1, 2, channel, 0.0, 1.0)],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::Incomplete(UnknownReason::NotMeasured),
    )
    .unwrap();
    assert_eq!(
        partial.total.milliwatts,
        Evidence::Unknown(UnknownReason::NotMeasured)
    );
    assert_eq!(
        partial.aggregate_status,
        InterferenceAggregateStatus::Incomplete(UnknownReason::NotMeasured)
    );
}

#[test]
fn positive_subnormal_product_is_numerical_failure_not_known_zero() {
    let channel = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let d = desired(channel.clone());
    let subnormal = input(1, 2, channel, f64::MIN_POSITIVE, f64::MIN_POSITIVE);
    let result = effective_interference(
        &d,
        &[subnormal],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        result.contributions[0].effective_power,
        Evidence::Unknown(UnknownReason::SolverFailure)
    );
    assert_eq!(
        result.total.milliwatts,
        Evidence::Unknown(UnknownReason::SolverFailure)
    );
    assert_eq!(
        result.aggregate_status,
        InterferenceAggregateStatus::NumericalFailure
    );
}

#[test]
fn same_bss_policy_and_identity_deduplication_are_explicit() {
    let channel = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let mut same_bss = input(1, 2, channel.clone(), 1.0, 1.0);
    same_bss.identity.bss_id = Some(bss(1));
    same_bss.identity.mld_id = Some(mld(1));
    let d = desired(channel.clone());
    let excluded = effective_interference(
        &d,
        &[same_bss.clone()],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        excluded.total.milliwatts,
        Evidence::Known(LinearPower::Zero)
    );
    assert!(matches!(
        excluded.contributions[0].disposition,
        ContributionDisposition::Excluded(ExclusionReason::SameBssCoordinated)
    ));
    let counted = effective_interference(
        &d,
        &[same_bss.clone()],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::CountIndependently,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(
        counted.total.milliwatts,
        Evidence::Known(LinearPower::Positive(Milliwatts::new(1.0).unwrap()))
    );
    assert_eq!(
        effective_interference(
            &d,
            &[same_bss],
            SpectralMaskMethod::Trapezoid20MhzReceiverV1,
            SameBssPolicy::RejectAmbiguous,
            ObservationSetStatus::CompleteMeasured,
        ),
        Err(InterferenceError::AmbiguousSameBss(radio(2)))
    );

    let mut same_mld = input(6, 6, channel.clone(), 1.0, 1.0);
    same_mld.identity.mld_id = Some(mld(1));
    let mld_excluded = effective_interference(
        &d,
        &[same_mld],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert!(matches!(
        mld_excluded.contributions[0].disposition,
        ContributionDisposition::Excluded(ExclusionReason::SameMldCoordinated)
    ));

    let first = input(1, 7, channel.clone(), 1.0, 0.5);
    let mut alias = first.clone();
    alias.input_id = id(2);
    alias.identity.bss_id = Some(bss(42));
    let deduped = effective_interference(
        &d,
        &[alias, first.clone()],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(deduped.contributions.len(), 1);
    assert_eq!(deduped.contributions[0].input_ids, vec![id(1), id(2)]);
    assert_eq!(deduped.contributions[0].identities.len(), 2);
    assert_eq!(deduped.input_ids, vec![id(1), id(2)]);

    let mut conflict = input(3, 7, channel.clone(), 2.0, 0.5);
    conflict.identity.physical_device_id = Some(physical(8));
    assert_eq!(
        effective_interference(
            &d,
            &[conflict, first.clone()],
            SpectralMaskMethod::Trapezoid20MhzReceiverV1,
            SameBssPolicy::ExcludeCoordinated,
            ObservationSetStatus::CompleteMeasured,
        ),
        Err(InterferenceError::AmbiguousDuplicate(radio(7)))
    );

    let mut bss_alias = input(4, 8, channel.clone(), 1.0, 0.5);
    bss_alias.identity.bss_id = Some(bss(99));
    let mut bss_alias_two = bss_alias.clone();
    bss_alias_two.input_id = id(5);
    bss_alias_two.identity.radio_id = radio(9);
    assert_eq!(
        effective_interference(
            &d,
            &[bss_alias, bss_alias_two],
            SpectralMaskMethod::Trapezoid20MhzReceiverV1,
            SameBssPolicy::CountIndependently,
            ObservationSetStatus::CompleteMeasured,
        ),
        Err(InterferenceError::AmbiguousBssAlias(bss(99)))
    );
}

#[test]
fn order_is_deterministic_and_unknown_geometry_is_retained() {
    let channel = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let one = input(1, 11, channel.clone(), 1.0, 0.5);
    let mut two = input(2, 12, channel.clone(), 2.0, 0.25);
    two.geometry = GeometryEvidence::Unsupported(UnsupportedGeometryReason::FutureWidth);
    let d = desired(channel);
    let a = effective_interference(
        &d,
        &[one.clone(), two.clone()],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    let b = effective_interference(
        &d,
        &[two, one],
        SpectralMaskMethod::Trapezoid20MhzReceiverV1,
        SameBssPolicy::ExcludeCoordinated,
        ObservationSetStatus::CompleteMeasured,
    )
    .unwrap();
    assert_eq!(a.contributions, b.contributions);
    assert_eq!(a.total, b.total);
    assert_eq!(
        a.total.milliwatts,
        Evidence::Unknown(UnknownReason::UnsupportedCapability)
    );
    assert_eq!(
        a.aggregate_status,
        InterferenceAggregateStatus::Unsupported(UnsupportedGeometryReason::FutureWidth)
    );
    assert_eq!(
        a.contributions[1].geometry,
        GeometryEvidence::Unsupported(UnsupportedGeometryReason::FutureWidth)
    );
}

#[test]
fn finite_extreme_power_is_bounded_and_input_count_is_limited() {
    let channel = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let d = desired(channel.clone());
    let huge = input(1, 20, channel.clone(), f64::MAX, 1.0);
    assert!(
        effective_interference(
            &d,
            &[huge.clone(), {
                let mut duplicate = huge.clone();
                duplicate.input_id = id(2);
                duplicate.identity.radio_id = radio(21);
                duplicate
            }],
            SpectralMaskMethod::Trapezoid20MhzReceiverV1,
            SameBssPolicy::ExcludeCoordinated,
            ObservationSetStatus::CompleteMeasured,
        )
        .is_err()
    );
    let oversized = vec![huge; MAX_INTERFERERS + 1];
    assert_eq!(
        effective_interference(
            &d,
            &oversized,
            SpectralMaskMethod::Trapezoid20MhzReceiverV1,
            SameBssPolicy::ExcludeCoordinated,
            ObservationSetStatus::CompleteMeasured,
        ),
        Err(InterferenceError::ResourceLimit)
    );
}

#[test]
fn duplicate_input_identity_is_rejected_and_utilization_wire_is_labeled() {
    let channel = geometry(WifiBand::Ghz5, 36, 5180.0, ChannelWidth::Mhz20, 0);
    let d = desired(channel.clone());
    let one = input(1, 20, channel.clone(), 1.0, 0.5);
    let mut duplicate = one.clone();
    duplicate.identity.radio_id = radio(21);
    assert_eq!(
        effective_interference(
            &d,
            &[one, duplicate],
            SpectralMaskMethod::Trapezoid20MhzReceiverV1,
            SameBssPolicy::CountIndependently,
            ObservationSetStatus::CompleteMeasured,
        ),
        Err(InterferenceError::DuplicateInput(id(1)))
    );
    assert!(serde_json::from_str::<UtilizationEvidence>(r#"0.5"#).is_err());
    assert!(
        serde_json::from_str::<UtilizationEvidence>(
            r#"{"class":"scenario_assumption","utilization":0.5,"extra":true}"#
        )
        .is_err()
    );
}

proptest! {
    #[test]
    fn equal_width_coupling_is_symmetric_and_bounded(primary in 1u16..=13, other in 1u16..=13) {
        let left = geometry(WifiBand::Ghz2_4, primary, 2407.0 + 5.0 * primary as f64, ChannelWidth::Mhz20, 0);
        let right = geometry(WifiBand::Ghz2_4, other, 2407.0 + 5.0 * other as f64, ChannelWidth::Mhz20, 0);
        let a = coupling(&left, &right, SpectralMaskMethod::Trapezoid20MhzReceiverV1);
        let b = coupling(&right, &left, SpectralMaskMethod::Trapezoid20MhzReceiverV1);
        if let (CouplingValue::Known(left), CouplingValue::Known(right)) = (&a, &b) {
            prop_assert!((left.get() - right.get()).abs() <= 1e-12);
        } else {
            prop_assert_eq!(a.clone(), b);
        }
        if let CouplingValue::Known(value) = &a {
            prop_assert!((0.0..=1.0).contains(&value.get()));
        } else {
            prop_assert!(false, "known channel geometry cannot produce unknown coupling");
        }
    }
}
