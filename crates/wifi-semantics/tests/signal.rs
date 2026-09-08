use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{ClockEpochId, ObservationId},
    time::MonotonicTimestamp,
    units::{Db, Dbm, Dimensionless, Milliwatts, Probability},
};
use kyberia_wifi_semantics::*;
use proptest::prelude::*;

fn id(value: u8) -> ObservationId {
    ObservationId::from_bytes([value; 16]).unwrap()
}
fn epoch(value: u8) -> ClockEpochId {
    ClockEpochId::from_bytes([value; 16]).unwrap()
}
fn sample(value: u8, time: u64, rssi: f64) -> SignalSample {
    SignalSample {
        observation_id: id(value),
        captured_at: MonotonicTimestamp {
            epoch: epoch(1),
            nanoseconds: time,
        },
        rssi: Dbm::new(rssi).unwrap(),
    }
}
fn static_sample(value: u8, rssi: f64) -> StaticSignalSample {
    StaticSignalSample {
        observation_id: id(value),
        rssi: Dbm::new(rssi).unwrap(),
    }
}
fn known(result: &SignalAggregate) -> f64 {
    result.estimate.as_known().unwrap().get()
}

#[test]
fn canonical_power_conversions_and_sums_use_linear_power() {
    assert_eq!(
        dbm_to_milliwatts(Dbm::new(0.0).unwrap()).unwrap(),
        Milliwatts::new(1.0).unwrap()
    );
    assert!((dbm_to_milliwatts(Dbm::new(10.0).unwrap()).unwrap().get() - 10.0).abs() < 1e-12);
    assert!(
        (milliwatts_to_dbm(Milliwatts::new(0.001).unwrap())
            .unwrap()
            .get()
            + 30.0)
            .abs()
            < 1e-12
    );
    let result = sum_dbm(&[Dbm::new(0.0).unwrap(), Dbm::new(0.0).unwrap()]).unwrap();
    assert!((result.as_known().unwrap().get() - 3.010_299_956_639_812).abs() < 1e-12);
    assert_eq!(
        sum_dbm(&[]).unwrap(),
        Evidence::Unknown(UnknownReason::NotMeasured)
    );
    assert_eq!(
        dbm_to_milliwatts(Dbm::new(-1e308).unwrap()),
        Err(Error::NumericalFailure)
    );
    assert!(serde_json::from_str::<Milliwatts>("0").is_err());
    assert!(serde_json::from_str::<Milliwatts>("-1").is_err());
}

#[test]
fn snr_sir_and_sinr_require_evidence_and_use_linear_denominators() {
    let signal = Dbm::new(-50.0).unwrap();
    assert_eq!(
        snr_db(signal, Evidence::Known(Dbm::new(-90.0).unwrap())).unwrap(),
        Evidence::Known(Db::new(40.0).unwrap())
    );
    let interference = sum_dbm(&[Dbm::new(-80.0).unwrap(), Dbm::new(-80.0).unwrap()]).unwrap();
    let sir = sir_db(signal, interference.clone())
        .unwrap()
        .as_known()
        .unwrap()
        .get();
    assert!((sir - 26.989_700_043_360_188).abs() < 1e-12);
    let sinr = sinr_db(
        signal,
        interference,
        Evidence::Known(Dbm::new(-90.0).unwrap()),
    )
    .unwrap()
    .as_known()
    .unwrap()
    .get();
    assert!((sinr - 26.777_807_052_660_79).abs() < 1e-12);

    assert_eq!(
        snr_db(signal, Evidence::Unknown(UnknownReason::NotMeasured)).unwrap(),
        Evidence::Unknown(UnknownReason::NotMeasured)
    );
    assert_eq!(
        sir_db(
            signal,
            Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        )
        .unwrap(),
        Evidence::Unknown(UnknownReason::SourceDidNotProvide)
    );
    assert_eq!(
        sinr_db(
            signal,
            Evidence::Known(Dbm::new(-80.0).unwrap()),
            Evidence::Unknown(UnknownReason::NotObservable),
        )
        .unwrap(),
        Evidence::Unknown(UnknownReason::NotObservable)
    );
}

#[test]
fn all_static_aggregators_have_independent_known_oracles() {
    let samples = [
        sample(1, 1, -80.0),
        sample(2, 2, -70.0),
        sample(3, 3, -60.0),
        sample(4, 4, -20.0),
    ];
    let median_result = aggregate(&samples, AggregateMethod::MedianDbm).unwrap();
    assert_eq!(known(&median_result), -65.0);
    assert!(matches!(
        median_result.percentile_interval,
        Evidence::Unknown(UnknownReason::NotApplicable)
    ));
    let trimmed = aggregate(
        &samples,
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.25).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(known(&trimmed), -65.0);
    let linear = aggregate(&samples[..2], AggregateMethod::LinearPowerMean).unwrap();
    assert!((known(&linear) - -72.596_373_105_057_56).abs() < 1e-12);
    let range = aggregate(
        &samples,
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.25).unwrap(),
            upper: Probability::new(0.75).unwrap(),
        },
    )
    .unwrap();
    let interval = range.percentile_interval.as_known().unwrap();
    assert_eq!((interval.lower.get(), interval.upper.get()), (-72.5, -50.0));
    assert_eq!(known(&range), -65.0);
}

#[test]
fn ordered_live_aggregators_retain_order_and_reject_bad_time() {
    let samples = [
        sample(1, 1, -80.0),
        sample(2, 2, -60.0),
        sample(3, 3, -40.0),
    ];
    let ewma = aggregate(
        &samples,
        AggregateMethod::EwmaDbm {
            alpha: Probability::new(0.5).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(known(&ewma), -55.0);
    assert_eq!(ewma.observation_order, vec![id(1), id(2), id(3)]);
    let robust = aggregate(
        &samples[..2],
        AggregateMethod::RobustStateSpaceDbm {
            process_stddev: Db::new(1.0).unwrap(),
            measurement_stddev: Db::new(2.0).unwrap(),
            huber_threshold_stddevs: Dimensionless::new(1.5).unwrap(),
        },
    )
    .unwrap();
    // Independent one-step oracle: prior variance 4, predicted variance 5,
    // innovation variance 9, Huber limit 4.5, gain 5/9.
    assert!((known(&robust) - -77.5).abs() < 1e-12);
    let reversed = [samples[1], samples[0]];
    assert_eq!(
        aggregate(
            &reversed,
            AggregateMethod::EwmaDbm {
                alpha: Probability::new(0.5).unwrap()
            }
        ),
        Err(Error::NonMonotonicSequence)
    );
    let mut other_epoch = samples[1];
    other_epoch.captured_at.epoch = epoch(2);
    assert_eq!(
        aggregate(
            &[samples[0], other_epoch],
            AggregateMethod::EwmaDbm {
                alpha: Probability::new(0.5).unwrap()
            }
        ),
        Err(Error::ClockEpochMismatch)
    );
}

#[test]
fn static_aggregator_rejects_temporal_methods_without_monotonic_evidence() {
    let samples = [static_sample(1, -70.0), static_sample(2, -60.0)];
    assert_eq!(
        aggregate_static(
            &samples,
            AggregateMethod::EwmaDbm {
                alpha: Probability::new(0.5).unwrap(),
            }
        ),
        Err(Error::TemporalMethodRequiresMonotonicTime)
    );
    assert_eq!(
        aggregate_static(
            &samples,
            AggregateMethod::RobustStateSpaceDbm {
                process_stddev: Db::new(1.0).unwrap(),
                measurement_stddev: Db::new(2.0).unwrap(),
                huber_threshold_stddevs: Dimensionless::new(1.5).unwrap(),
            }
        ),
        Err(Error::TemporalMethodRequiresMonotonicTime)
    );
}

#[test]
fn aggregate_method_wire_rejects_unknown_parameters() {
    assert!(
        serde_json::from_str::<AggregateMethod>(
            r#"{"method":"trimmed_mean_dbm","trim_each_tail":0.25,"unexpected":true}"#
        )
        .is_err()
    );
}

#[test]
fn unknown_duplicate_and_invalid_configuration_are_explicit() {
    let empty = aggregate(&[], AggregateMethod::MedianDbm).unwrap();
    assert_eq!(
        empty.estimate,
        Evidence::Unknown(UnknownReason::NotMeasured)
    );
    assert_eq!(empty.sample_count, 0);
    let empty_range = aggregate(
        &[],
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.1).unwrap(),
            upper: Probability::new(0.9).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(
        empty_range.percentile_interval,
        Evidence::Unknown(UnknownReason::NotMeasured)
    );
    let duplicate = [sample(1, 1, -70.0), sample(1, 2, -60.0)];
    assert_eq!(
        aggregate(&duplicate, AggregateMethod::MedianDbm),
        Err(Error::DuplicateObservation(id(1)))
    );
    for method in [
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.5).unwrap(),
        },
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.9).unwrap(),
            upper: Probability::new(0.1).unwrap(),
        },
        AggregateMethod::EwmaDbm {
            alpha: Probability::new(0.0).unwrap(),
        },
        AggregateMethod::RobustStateSpaceDbm {
            process_stddev: Db::new(-1.0).unwrap(),
            measurement_stddev: Db::new(1.0).unwrap(),
            huber_threshold_stddevs: Dimensionless::new(1.0).unwrap(),
        },
    ] {
        assert!(matches!(
            aggregate(&[], method),
            Err(Error::InvalidConfiguration(_))
        ));
    }
}

#[test]
fn aggregate_enforces_its_request_resource_bound_before_processing() {
    let oversized = vec![sample(1, 1, -70.0); MAX_SAMPLES + 1];
    assert_eq!(
        aggregate(&oversized, AggregateMethod::MedianDbm),
        Err(Error::ResourceLimit)
    );
}

#[test]
fn convex_dbm_statistics_do_not_overflow_on_extreme_finite_inputs() {
    let samples = [sample(1, 1, -1e308), sample(2, 2, 1e308)];
    for method in [
        AggregateMethod::MedianDbm,
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.0).unwrap(),
        },
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.25).unwrap(),
            upper: Probability::new(0.75).unwrap(),
        },
        AggregateMethod::EwmaDbm {
            alpha: Probability::new(0.5).unwrap(),
        },
    ] {
        assert_eq!(known(&aggregate(&samples, method).unwrap()), 0.0);
    }
    let repeated_max = [
        sample(1, 1, f64::MAX),
        sample(2, 2, f64::MAX),
        sample(3, 3, f64::MAX),
    ];
    for method in [
        AggregateMethod::MedianDbm,
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.0).unwrap(),
        },
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.25).unwrap(),
            upper: Probability::new(0.75).unwrap(),
        },
        AggregateMethod::EwmaDbm {
            alpha: Probability::new(0.1).unwrap(),
        },
        AggregateMethod::LinearPowerMean,
    ] {
        assert_eq!(known(&aggregate(&repeated_max, method).unwrap()), f64::MAX);
    }
}

#[test]
fn static_methods_canonicalize_provenance_order_but_do_not_hide_method() {
    let a = [
        sample(3, 1, -50.0),
        sample(1, 2, -70.0),
        sample(2, 3, -60.0),
    ];
    let b = [a[2], a[0], a[1]];
    for method in [
        AggregateMethod::MedianDbm,
        AggregateMethod::TrimmedMeanDbm {
            trim_each_tail: Probability::new(0.1).unwrap(),
        },
        AggregateMethod::LinearPowerMean,
        AggregateMethod::PercentileRange {
            lower: Probability::new(0.1).unwrap(),
            upper: Probability::new(0.9).unwrap(),
        },
    ] {
        assert_eq!(
            aggregate(&a, method).unwrap(),
            aggregate(&b, method).unwrap()
        );
    }
    let left = aggregate(&a, AggregateMethod::MedianDbm).unwrap();
    assert_ne!(
        aggregate(&a, AggregateMethod::LinearPowerMean)
            .unwrap()
            .estimate,
        left.estimate
    );
}

proptest! {
    #[test]
    fn power_sum_is_permutation_invariant(values in prop::collection::vec(-300.0f64..100.0, 1..128)) {
        let forward: Vec<_> = values.iter().map(|v| Dbm::new(*v).unwrap()).collect();
        let mut reverse = forward.clone(); reverse.reverse();
        let a = sum_dbm(&forward).unwrap().as_known().unwrap().get();
        let b = sum_dbm(&reverse).unwrap().as_known().unwrap().get();
        prop_assert_eq!(a.to_bits(), b.to_bits());
        prop_assert!(a + 1e-12 >= values.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    }

    #[test]
    fn dbm_milliwatt_round_trip(value in -300.0f64..300.0) {
        let original=Dbm::new(value).unwrap();
        let round=milliwatts_to_dbm(dbm_to_milliwatts(original).unwrap()).unwrap();
        prop_assert!((round.get()-value).abs() <= 1e-12 * value.abs().max(1.0));
    }
}

#[test]
fn wire_method_names_and_units_are_explicit() {
    let value = serde_json::to_value(AggregateMethod::LinearPowerMean).unwrap();
    assert_eq!(value["method"], "linear_power_mean");
    let value = serde_json::to_value(AggregateMethod::EwmaDbm {
        alpha: Probability::new(0.25).unwrap(),
    })
    .unwrap();
    assert_eq!(value, serde_json::json!({"method":"ewma_dbm","alpha":0.25}));
    let result = aggregate(&[sample(1, 1, -60.0)], AggregateMethod::MedianDbm).unwrap();
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(encoded.contains("kyberia-wifi-signal/1"));
    assert_eq!(
        serde_json::from_str::<SignalAggregate>(&encoded).unwrap(),
        result
    );
    assert!(
        serde_json::from_str::<SignalAggregate>(
            &encoded.replace("kyberia-wifi-signal/1", "kyberia-wifi-signal/2")
        )
        .is_err()
    );
}
