use kyberia_active_measurement::{
    ActiveSchedule, Cancellation, ConnectResult, MonotonicClock, NeverCancelled, SCHEDULE_VERSION,
    ScheduleError, SleepResult, StdMonotonicClock, StdTcpConnector, TcpConnector, build_schedule,
    execute,
};
use kyberia_domain::{
    active::{
        ActiveAuthorization, ActiveEndpoint, ActiveEndpointTier, ActiveIpAddress,
        ActiveMeasurementMethod, ActiveMeasurementProvenance, ActiveSample, ActiveSampleOutcome,
        ActiveSocketAddr, ActiveTarget, ActiveTestLimits, ActiveTestRun, ActiveTransportProtocol,
        EndpointAttribution,
    },
    evidence::{Evidence, UnknownReason},
    identity::{
        ActiveEndpointId, ActiveIntervalId, ActiveResultId, ActiveSampleId, ActiveTestRunId,
        AdapterId, ClockEpochId, SensorId, Text,
    },
    time::MonotonicTimestamp,
    units::{Milliseconds, Seconds},
};
use proptest::prelude::*;
use serde_json::json;
use std::{
    cell::Cell, collections::VecDeque, net::TcpListener, rc::Rc, sync::mpsc, thread, time::Duration,
};

fn id<T>(value: u8) -> T
where
    T: FromBytes,
{
    T::from_bytes([value; 16])
}

trait FromBytes {
    fn from_bytes(bytes: [u8; 16]) -> Self;
}

macro_rules! ids {
    ($($name:ty),+ $(,)?) => {$ (
        impl FromBytes for $name {
            fn from_bytes(bytes: [u8; 16]) -> Self {
                <$name>::from_bytes(bytes).unwrap()
            }
        }
    )+};
}
ids!(
    ActiveEndpointId,
    ActiveIntervalId,
    ActiveResultId,
    ActiveSampleId,
    ActiveTestRunId,
    AdapterId,
    ClockEpochId,
    SensorId
);

fn text(value: &str) -> Text {
    Text::new(value).unwrap()
}

fn unknown_attribution() -> EndpointAttribution {
    EndpointAttribution::new(
        Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        Evidence::Unknown(UnknownReason::NotObservable),
        Evidence::Unknown(UnknownReason::NotAdvertised),
    )
}

fn provenance(epoch: ClockEpochId) -> ActiveMeasurementProvenance {
    ActiveMeasurementProvenance::new(
        text("test-adapter"),
        text("test/1"),
        text("rf-atlas-active-tcp-connect-timing/v1"),
        epoch,
        Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        Evidence::Unknown(UnknownReason::SourceDidNotProvide),
    )
    .unwrap()
}

fn endpoint(value: u8, port: u16, tier: ActiveEndpointTier) -> ActiveEndpoint {
    let target = ActiveTarget::new(
        text("loopback-test"),
        tier,
        ActiveSocketAddr::new(ActiveIpAddress::v4([127, 0, 0, 1]), port).unwrap(),
    )
    .unwrap();
    ActiveEndpoint::new(
        id::<ActiveEndpointId>(value),
        tier,
        target,
        ActiveTransportProtocol::Tcp,
        ActiveMeasurementMethod::TcpConnectTiming,
        unknown_attribution(),
    )
    .unwrap()
}

fn authorization(tiers: Vec<ActiveEndpointTier>, loopback: bool) -> ActiveAuthorization {
    ActiveAuthorization::new(
        true,
        tiers,
        loopback,
        false,
        text("local test authorization"),
    )
    .unwrap()
}

fn limits(max_samples: u32, timeout: f64, duration: f64, spacing: f64) -> ActiveTestLimits {
    ActiveTestLimits::new(
        max_samples,
        2,
        Seconds::new(timeout).unwrap(),
        Seconds::new(duration).unwrap(),
        Seconds::new(spacing).unwrap(),
    )
    .unwrap()
}

fn make_run(
    endpoints: Vec<ActiveEndpoint>,
    run_duration: f64,
    per_endpoint: u32,
) -> (ActiveTestRun, kyberia_domain::active::ActiveInterval) {
    make_run_with_spacing(endpoints, run_duration, per_endpoint, 0.0)
}

fn make_run_with_spacing(
    endpoints: Vec<ActiveEndpoint>,
    run_duration: f64,
    per_endpoint: u32,
    spacing: f64,
) -> (ActiveTestRun, kyberia_domain::active::ActiveInterval) {
    let epoch = id(200);
    let started = MonotonicTimestamp {
        epoch,
        nanoseconds: 10_000,
    };
    let deadline = MonotonicTimestamp {
        epoch,
        nanoseconds: 10_000 + (run_duration * 1e9) as u64,
    };
    let mut run = ActiveTestRun::new(
        id(201),
        started,
        deadline,
        endpoints,
        authorization(vec![ActiveEndpointTier::LanReference], true),
        limits(64, run_duration.min(0.5), run_duration, spacing),
        provenance(epoch),
    )
    .unwrap();
    let interval = run
        .create_interval(id(202), Seconds::new(run_duration).unwrap(), per_endpoint)
        .unwrap();
    (run, interval)
}

#[test]
fn target_and_units_fail_closed() {
    assert!(ActiveSocketAddr::new(ActiveIpAddress::v4([127, 0, 0, 1]), 0).is_err());
    for address in [
        ActiveIpAddress::v4([0, 0, 0, 0]),
        ActiveIpAddress::v4([224, 0, 0, 1]),
        ActiveIpAddress::v4([255, 255, 255, 255]),
    ] {
        assert!(ActiveSocketAddr::new(address, 80).is_err());
    }
    for octets in [
        [127, 0, 0, 1],
        [10, 0, 0, 1],
        [192, 0, 2, 1],
        [224, 0, 0, 1],
        [255, 255, 255, 255],
    ] {
        let mut mapped = [0_u8; 16];
        mapped[10] = 0xff;
        mapped[11] = 0xff;
        mapped[12..].copy_from_slice(&octets);
        assert!(ActiveIpAddress::v6(mapped).is_ipv4_mapped());
        assert!(ActiveSocketAddr::new(ActiveIpAddress::v6(mapped), 80).is_err());
    }
    let mut ipv6_link_local = [0_u8; 16];
    ipv6_link_local[0] = 0xfe;
    ipv6_link_local[1] = 0x80;
    assert!(ActiveIpAddress::v6(ipv6_link_local).is_link_local());
    assert!(ActiveSocketAddr::new(ActiveIpAddress::v6(ipv6_link_local), 80).is_err());
    assert!(Seconds::new(f64::NAN).is_err());
    assert!(Milliseconds::new(-1.0).is_err());
    let endpoint = endpoint(1, 80, ActiveEndpointTier::LanReference);
    assert!(
        ActiveTestRun::new(
            id::<ActiveTestRunId>(1),
            MonotonicTimestamp {
                epoch: id(2),
                nanoseconds: 0
            },
            MonotonicTimestamp {
                epoch: id(2),
                nanoseconds: 1_000_000
            },
            vec![endpoint],
            ActiveAuthorization::new(
                true,
                vec![ActiveEndpointTier::LanReference],
                false,
                false,
                text("denied loopback")
            )
            .unwrap(),
            limits(4, 0.0005, 0.001, 0.0),
            provenance(id(2)),
        )
        .is_err()
    );
}

#[test]
fn canonical_active_wire_revalidates_schema_and_socket_limits() {
    let value = endpoint(1, 80, ActiveEndpointTier::LanReference);
    let encoded = serde_json::to_value(&value).unwrap();
    let decoded: ActiveEndpoint = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, value);

    let mut bad_schema = encoded.clone();
    bad_schema["schema_version"] = json!("2");
    assert!(serde_json::from_value::<ActiveEndpoint>(bad_schema).is_err());

    let mut bad_port = encoded;
    bad_port["target"]["address"]["port"] = json!(0);
    assert!(serde_json::from_value::<ActiveEndpoint>(bad_port).is_err());

    let valid_limits = limits(4, 0.1, 1.0, 0.0);
    let mut bad_limits = serde_json::to_value(valid_limits).unwrap();
    bad_limits["max_samples_total"] = json!(0);
    assert!(serde_json::from_value::<ActiveTestLimits>(bad_limits).is_err());

    let (run, interval) = make_run(vec![value], 1.0, 1);
    let mut legacy_run = serde_json::to_value(&run).unwrap();
    legacy_run.as_object_mut().unwrap().remove("intervals");
    let legacy_run: ActiveTestRun = serde_json::from_value(legacy_run).unwrap();
    assert!(legacy_run.intervals().is_empty());
    let mut bad_interval = serde_json::to_value(&interval).unwrap();
    bad_interval["samples_per_endpoint"] = json!(0);
    assert!(
        serde_json::from_value::<kyberia_domain::active::ActiveInterval>(bad_interval).is_err()
    );
    let mut unknown_window = serde_json::to_value(&interval).unwrap();
    unknown_window["window"]["start"]["extra"] = json!(true);
    assert!(
        serde_json::from_value::<kyberia_domain::active::ActiveInterval>(unknown_window).is_err()
    );

    let sample = ActiveSample::new(
        id::<ActiveSampleId>(205),
        run.id(),
        interval.id(),
        id(1),
        ActiveEndpointTier::LanReference,
        unknown_attribution(),
        0,
        run.started(),
        MonotonicTimestamp {
            epoch: run.started().epoch,
            nanoseconds: run.started().nanoseconds + 1_000_000,
        },
        ActiveSampleOutcome::Success,
        Evidence::Known(Milliseconds::new(1.0).unwrap()),
        run.provenance().clone(),
    )
    .unwrap();
    let mut unknown_sample_time = serde_json::to_value(&sample).unwrap();
    unknown_sample_time["started"]["extra"] = json!(true);
    assert!(serde_json::from_value::<ActiveSample>(unknown_sample_time).is_err());
    let mut inconsistent_duration = serde_json::to_value(&sample).unwrap();
    inconsistent_duration["tcp_connect_duration"]["detail"] = json!(2.0);
    assert!(serde_json::from_value::<ActiveSample>(inconsistent_duration).is_err());
    let mut legacy_method_version = serde_json::to_value(&sample).unwrap();
    legacy_method_version["provenance"]["method_version"] = json!("rf-atlas-active-tcp-connect/v1");
    assert!(serde_json::from_value::<ActiveSample>(legacy_method_version).is_err());
    let duplicate_id_sample = ActiveSample::new(
        sample.id(),
        run.id(),
        interval.id(),
        id(1),
        ActiveEndpointTier::LanReference,
        unknown_attribution(),
        1,
        run.started(),
        MonotonicTimestamp {
            epoch: run.started().epoch,
            nanoseconds: run.started().nanoseconds + 1_000_000,
        },
        ActiveSampleOutcome::Success,
        Evidence::Known(Milliseconds::new(1.0).unwrap()),
        run.provenance().clone(),
    )
    .unwrap();
    assert!(
        kyberia_domain::active::ActiveResult::from_samples(
            id(206),
            run.id(),
            interval.id(),
            &run.endpoints()[0],
            vec![sample.clone(), duplicate_id_sample.clone()],
        )
        .is_err()
    );
    let stats =
        kyberia_domain::active::ActiveStatistics::from_samples(std::slice::from_ref(&sample))
            .unwrap();
    assert_eq!(
        stats.packet_loss_percent(),
        &Evidence::Unknown(UnknownReason::NotMeasured)
    );
    assert_eq!(
        stats.tcp_attempt_failure_bursts().median(),
        &Evidence::Unknown(UnknownReason::NotApplicable)
    );
    let two_sample_stats =
        kyberia_domain::active::ActiveStatistics::from_samples(&[sample, duplicate_id_sample])
            .unwrap();
    let mut impossible_two_sample_timing =
        serde_json::to_value(two_sample_stats.connect_timing()).unwrap();
    impossible_two_sample_timing["max"] = json!({"state": "known", "detail": 2.0});
    assert!(
        serde_json::from_value::<kyberia_domain::active::ConnectTimingDistribution>(
            impossible_two_sample_timing
        )
        .is_err()
    );
    let mut bad_timing = serde_json::to_value(stats.connect_timing()).unwrap();
    bad_timing["successful_samples"] = json!(0);
    assert!(
        serde_json::from_value::<kyberia_domain::active::ConnectTimingDistribution>(bad_timing)
            .is_err()
    );
    let mut excessive_timing_count = serde_json::to_value(stats.connect_timing()).unwrap();
    excessive_timing_count["successful_samples"] = json!(4_097);
    assert!(
        serde_json::from_value::<kyberia_domain::active::ConnectTimingDistribution>(
            excessive_timing_count
        )
        .is_err()
    );
    let mut zero_success_wrong_reason = serde_json::to_value(stats.connect_timing()).unwrap();
    zero_success_wrong_reason["successful_samples"] = json!(0);
    for field in ["median", "p90", "p95", "p99", "max"] {
        zero_success_wrong_reason[field] = json!({"state": "unknown", "detail": "not_applicable"});
    }
    assert!(
        serde_json::from_value::<kyberia_domain::active::ConnectTimingDistribution>(
            zero_success_wrong_reason
        )
        .is_err()
    );
    let mut impossible_one_success = serde_json::to_value(stats.connect_timing()).unwrap();
    impossible_one_success["p90"] = json!({"state": "known", "detail": 2.0});
    assert!(
        serde_json::from_value::<kyberia_domain::active::ConnectTimingDistribution>(
            impossible_one_success
        )
        .is_err()
    );
    let mut bad_bursts = serde_json::to_value(stats.tcp_attempt_failure_bursts()).unwrap();
    bad_bursts["failed_samples"] = json!(1);
    assert!(
        serde_json::from_value::<kyberia_domain::active::TcpAttemptFailureBurstDistribution>(
            bad_bursts
        )
        .is_err()
    );
    let mut wrong_no_failure_reason =
        serde_json::to_value(stats.tcp_attempt_failure_bursts()).unwrap();
    wrong_no_failure_reason["median"]["detail"] = json!("failed_test");
    assert!(
        serde_json::from_value::<kyberia_domain::active::TcpAttemptFailureBurstDistribution>(
            wrong_no_failure_reason
        )
        .is_err()
    );
    let failed_sample = ActiveSample::new(
        id::<ActiveSampleId>(209),
        run.id(),
        interval.id(),
        id(1),
        ActiveEndpointTier::LanReference,
        unknown_attribution(),
        0,
        run.started(),
        run.started(),
        ActiveSampleOutcome::ConnectionRefused,
        Evidence::Unknown(UnknownReason::FailedTest),
        run.provenance().clone(),
    )
    .unwrap();
    let failure_stats =
        kyberia_domain::active::ActiveStatistics::from_samples(&[failed_sample]).unwrap();
    let mut impossible_failure_burst =
        serde_json::to_value(failure_stats.tcp_attempt_failure_bursts()).unwrap();
    impossible_failure_burst["p90"] = json!({"state": "known", "detail": 2});
    assert!(
        serde_json::from_value::<kyberia_domain::active::TcpAttemptFailureBurstDistribution>(
            impossible_failure_burst
        )
        .is_err()
    );
    let make_failed_sample = |id_value: u8, ordinal: u32| {
        ActiveSample::new(
            id::<ActiveSampleId>(id_value),
            run.id(),
            interval.id(),
            id(1),
            ActiveEndpointTier::LanReference,
            unknown_attribution(),
            ordinal,
            run.started(),
            run.started(),
            ActiveSampleOutcome::ConnectionRefused,
            Evidence::Unknown(UnknownReason::FailedTest),
            run.provenance().clone(),
        )
        .unwrap()
    };
    let successful_break = ActiveSample::new(
        id::<ActiveSampleId>(212),
        run.id(),
        interval.id(),
        id(1),
        ActiveEndpointTier::LanReference,
        unknown_attribution(),
        1,
        run.started(),
        MonotonicTimestamp {
            epoch: run.started().epoch,
            nanoseconds: run.started().nanoseconds + 1_000_000,
        },
        ActiveSampleOutcome::Success,
        Evidence::Known(Milliseconds::new(1.0).unwrap()),
        run.provenance().clone(),
    )
    .unwrap();
    let failure_sequence_stats = kyberia_domain::active::ActiveStatistics::from_samples(&[
        make_failed_sample(210, 0),
        successful_break,
        make_failed_sample(213, 2),
        make_failed_sample(214, 3),
    ])
    .unwrap();
    assert_eq!(
        failure_sequence_stats
            .tcp_attempt_failure_bursts()
            .burst_count(),
        2
    );
    assert_eq!(
        failure_sequence_stats
            .tcp_attempt_failure_bursts()
            .failed_samples(),
        3
    );
    let mut impossible_failure_max =
        serde_json::to_value(failure_sequence_stats.tcp_attempt_failure_bursts()).unwrap();
    impossible_failure_max["max"] = json!({"state": "known", "detail": 3});
    assert!(
        serde_json::from_value::<kyberia_domain::active::TcpAttemptFailureBurstDistribution>(
            impossible_failure_max
        )
        .is_err()
    );
    let mut impossible_failure_quantiles =
        serde_json::to_value(failure_sequence_stats.tcp_attempt_failure_bursts()).unwrap();
    for field in ["median", "p90", "p95", "p99"] {
        impossible_failure_quantiles[field] = json!({"state": "known", "detail": 1});
    }
    impossible_failure_quantiles["max"] = json!({"state": "known", "detail": 2});
    assert!(
        serde_json::from_value::<kyberia_domain::active::TcpAttemptFailureBurstDistribution>(
            impossible_failure_quantiles
        )
        .is_err()
    );
    let mut wrong_zero_success_reason = serde_json::to_value(&failure_stats).unwrap();
    wrong_zero_success_reason["connect_timing"]["median"]["detail"] = json!("not_measured");
    assert!(
        serde_json::from_value::<kyberia_domain::active::ActiveStatistics>(
            wrong_zero_success_reason
        )
        .is_err()
    );
    let mut bad_packet_loss = serde_json::to_value(&stats).unwrap();
    bad_packet_loss["packet_loss_percent"] = json!({"state": "known", "detail": 0.0});
    assert!(
        serde_json::from_value::<kyberia_domain::active::ActiveStatistics>(bad_packet_loss)
            .is_err()
    );
    let mut bad_stats = serde_json::to_value(&stats).unwrap();
    bad_stats["eligible_samples"] = json!(0);
    assert!(serde_json::from_value::<kyberia_domain::active::ActiveStatistics>(bad_stats).is_err());
    let mut excessive_statistics = serde_json::to_value(&stats).unwrap();
    excessive_statistics["scheduled_samples"] = json!(4_097);
    excessive_statistics["eligible_samples"] = json!(4_097);
    excessive_statistics["connect_timing"]["successful_samples"] = json!(4_097);
    assert!(
        serde_json::from_value::<kyberia_domain::active::ActiveStatistics>(excessive_statistics)
            .is_err()
    );
    let mut measured_without_eligible = serde_json::to_value(&stats).unwrap();
    measured_without_eligible["cancelled_samples"] = json!(1);
    measured_without_eligible["eligible_samples"] = json!(0);
    assert!(
        serde_json::from_value::<kyberia_domain::active::ActiveStatistics>(
            measured_without_eligible
        )
        .is_err()
    );
    assert!(
        ActiveSample::new(
            id::<ActiveSampleId>(207),
            run.id(),
            interval.id(),
            id(1),
            ActiveEndpointTier::LanReference,
            unknown_attribution(),
            0,
            run.started(),
            run.started(),
            ActiveSampleOutcome::ConnectionRefused,
            Evidence::Unknown(UnknownReason::NotMeasured),
            run.provenance().clone(),
        )
        .is_err()
    );
    assert!(
        ActiveSample::new(
            id::<ActiveSampleId>(208),
            run.id(),
            interval.id(),
            id(1),
            ActiveEndpointTier::LanReference,
            unknown_attribution(),
            0,
            run.started(),
            run.started(),
            ActiveSampleOutcome::Cancelled,
            Evidence::Unknown(UnknownReason::FailedTest),
            run.provenance().clone(),
        )
        .is_err()
    );
    assert_eq!(run.endpoints().len(), 1);
}

#[test]
fn topology_and_authorization_mismatch_is_rejected() {
    let target = ActiveTarget::new(
        text("gateway"),
        ActiveEndpointTier::Gateway,
        ActiveSocketAddr::new(ActiveIpAddress::v4([192, 168, 1, 1]), 80).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        ActiveEndpoint::new(
            id::<ActiveEndpointId>(1),
            ActiveEndpointTier::InternetControl,
            target,
            ActiveTransportProtocol::Tcp,
            ActiveMeasurementMethod::TcpConnectTiming,
            unknown_attribution(),
        ),
        Err(kyberia_domain::active::ActiveValidationError::TargetTierMismatch)
    ));
    let target = ActiveTarget::new(
        text("public"),
        ActiveEndpointTier::InternetControl,
        ActiveSocketAddr::new(ActiveIpAddress::v4([192, 0, 2, 1]), 443).unwrap(),
    )
    .unwrap();
    let endpoint = ActiveEndpoint::new(
        id::<ActiveEndpointId>(2),
        ActiveEndpointTier::InternetControl,
        target,
        ActiveTransportProtocol::Tcp,
        ActiveMeasurementMethod::TcpConnectTiming,
        unknown_attribution(),
    )
    .unwrap();
    assert!(
        endpoint
            .validate_for_authorization(&authorization(
                vec![ActiveEndpointTier::LanReference],
                false
            ))
            .is_err()
    );
}

#[test]
fn schedule_is_deterministic_and_resource_bounded() {
    let (run, interval) = make_run(
        vec![
            endpoint(9, 9, ActiveEndpointTier::LanReference),
            endpoint(3, 9, ActiveEndpointTier::LanReference),
        ],
        1.0,
        2,
    );
    let first = build_schedule(&run, &interval).unwrap();
    let second = build_schedule(&run, &interval).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.schedule_version().as_str(), SCHEDULE_VERSION);
    assert_eq!(first.samples()[0].endpoint().id(), id(3));
    assert_eq!(first.samples()[2].endpoint().id(), id(9));
    assert_eq!(first.samples()[0].ordinal(), 0);
    assert_eq!(first.samples()[1].ordinal(), 1);
    assert_eq!(first.max_concurrency(), 2);
    let sample_ids: std::collections::BTreeSet<_> =
        first.samples().iter().map(|sample| sample.id()).collect();
    assert_eq!(sample_ids.len(), first.samples().len());
    let mut forged_interval = serde_json::to_value(&interval).unwrap();
    forged_interval["samples_per_endpoint"] = json!(4_096);
    let forged_interval = serde_json::from_value(forged_interval).unwrap();
    assert!(matches!(
        build_schedule(&run, &forged_interval),
        Err(ScheduleError::Invalid(
            kyberia_domain::active::ActiveValidationError::Domain(
                kyberia_domain::ValidationError::ResourceLimit(_)
            )
        ))
    ));
    let mut forged_rate_interval = serde_json::to_value(&interval).unwrap();
    forged_rate_interval["samples_per_endpoint"] = json!(2);
    forged_rate_interval["window"]["end"]["nanoseconds"] =
        json!(interval.window().start().nanoseconds + 499_999_999);
    let forged_rate_interval = serde_json::from_value(forged_rate_interval).unwrap();
    assert!(matches!(
        build_schedule(&run, &forged_rate_interval),
        Err(ScheduleError::Invalid(
            kyberia_domain::active::ActiveValidationError::Domain(
                kyberia_domain::ValidationError::OutOfRange("active interval rate")
            )
        ))
    ));
    let mut mismatched_interval = serde_json::to_value(&interval).unwrap();
    mismatched_interval["provenance"]["source"] = json!("other-source");
    let mismatched_interval =
        serde_json::from_value::<kyberia_domain::active::ActiveInterval>(mismatched_interval)
            .unwrap();
    assert!(matches!(
        build_schedule(&run, &mismatched_interval),
        Err(ScheduleError::Invalid(
            kyberia_domain::active::ActiveValidationError::Domain(
                kyberia_domain::ValidationError::Inconsistent(_)
            )
        ))
    ));
    let (mut limited_run, limited_interval) = make_run(
        vec![endpoint(1, 9, ActiveEndpointTier::LanReference)],
        1.0,
        2,
    );
    let mut too_many = ActiveTestRun::new(
        limited_run.id(),
        limited_run.started(),
        limited_run.deadline(),
        limited_run.endpoints().to_vec(),
        limited_run.authorization().clone(),
        limits(1, 0.5, 1.0, 0.0),
        limited_run.provenance().clone(),
    )
    .unwrap();
    assert!(matches!(
        too_many.create_interval(limited_interval.id(), Seconds::new(1.0).unwrap(), 2),
        Err(kyberia_domain::active::ActiveValidationError::Domain(
            kyberia_domain::ValidationError::ResourceLimit(_)
        ))
    ));
    assert!(
        limited_run
            .create_interval(id(203), Seconds::new(0.49).unwrap(), 2)
            .is_err()
    );
    assert!(
        ActiveTestRun::new(
            id::<ActiveTestRunId>(204),
            limited_run.started(),
            limited_run.deadline(),
            limited_run.endpoints().to_vec(),
            limited_run.authorization().clone(),
            limits(4, 0.1, 0.5, 0.0),
            limited_run.provenance().clone(),
        )
        .is_err()
    );
}

#[test]
fn run_registry_enforces_cumulative_interval_sample_budget() {
    let epoch = id(210);
    let started = MonotonicTimestamp {
        epoch,
        nanoseconds: 5_000,
    };
    let deadline = MonotonicTimestamp {
        epoch,
        nanoseconds: 2_000_005_000,
    };
    let mut run = ActiveTestRun::new(
        id(211),
        started,
        deadline,
        vec![endpoint(1, 9, ActiveEndpointTier::LanReference)],
        authorization(vec![ActiveEndpointTier::LanReference], true),
        limits(2, 0.1, 2.0, 0.0),
        provenance(epoch),
    )
    .unwrap();
    let first = run
        .create_interval(id(212), Seconds::new(1.0).unwrap(), 1)
        .unwrap();
    let second = run
        .create_interval(id(213), Seconds::new(1.0).unwrap(), 1)
        .unwrap();
    assert_eq!(run.intervals(), &[first.clone(), second]);
    assert!(matches!(
        run.create_interval(id(214), Seconds::new(1.0).unwrap(), 1),
        Err(kyberia_domain::active::ActiveValidationError::Domain(
            kyberia_domain::ValidationError::ResourceLimit("active run sample count")
        ))
    ));
    assert!(matches!(
        run.create_interval(first.id(), Seconds::new(1.0).unwrap(), 1),
        Err(kyberia_domain::active::ActiveValidationError::Domain(
            kyberia_domain::ValidationError::Inconsistent("duplicate active interval")
        ))
    ));

    let mut forged_run = serde_json::to_value(&run).unwrap();
    let mut forged_interval = serde_json::to_value(&first).unwrap();
    forged_interval["id"] = serde_json::to_value(id::<ActiveIntervalId>(215)).unwrap();
    forged_run["intervals"]
        .as_array_mut()
        .unwrap()
        .push(forged_interval.clone());
    assert!(serde_json::from_value::<ActiveTestRun>(forged_run).is_err());

    let detached: kyberia_domain::active::ActiveInterval =
        serde_json::from_value(forged_interval).unwrap();
    assert!(matches!(
        build_schedule(&run, &detached),
        Err(ScheduleError::RunIntervalMismatch)
    ));
}

#[derive(Clone)]
struct FakeClock {
    now: Rc<Cell<u64>>,
}

impl MonotonicClock for FakeClock {
    fn now_nanos(&mut self) -> u64 {
        self.now.get()
    }

    fn sleep_for(&mut self, duration: Duration, cancellation: &dyn Cancellation) -> SleepResult {
        let mut remaining = duration;
        while remaining > Duration::ZERO {
            if cancellation.is_cancelled() {
                return SleepResult::Cancelled;
            }
            let step = remaining.min(Duration::from_millis(25));
            self.now
                .set(self.now.get().saturating_add(step.as_nanos() as u64));
            remaining = remaining.saturating_sub(step);
        }
        if cancellation.is_cancelled() {
            SleepResult::Cancelled
        } else {
            SleepResult::Complete
        }
    }
}

struct FakeConnector {
    now: Rc<Cell<u64>>,
    results: VecDeque<(ConnectResult, u64)>,
    seen: Vec<ActiveSocketAddr>,
}

impl TcpConnector for FakeConnector {
    fn connect(
        &mut self,
        target: ActiveSocketAddr,
        _timeout: Duration,
        cancellation: &dyn Cancellation,
    ) -> ConnectResult {
        if cancellation.is_cancelled() {
            return ConnectResult::Cancelled;
        }
        self.seen.push(target);
        let (result, elapsed) = self
            .results
            .pop_front()
            .unwrap_or((ConnectResult::Error, 0));
        self.now.set(self.now.get().saturating_add(elapsed));
        if cancellation.is_cancelled() {
            ConnectResult::Cancelled
        } else {
            result
        }
    }
}

struct CancelAfter {
    now: Rc<Cell<u64>>,
    at: u64,
}

impl Cancellation for CancelAfter {
    fn is_cancelled(&self) -> bool {
        self.now.get() >= self.at
    }
}

#[test]
fn execution_preserves_typed_outcomes_and_statistics() {
    let (run, interval) = make_run(
        vec![endpoint(1, 9, ActiveEndpointTier::LanReference)],
        1.0,
        5,
    );
    let schedule = build_schedule(&run, &interval).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now: now.clone(),
        results: VecDeque::from([
            (ConnectResult::Connected, 10_000_000),
            (ConnectResult::ConnectionRefused, 1_000_000),
            (ConnectResult::Timeout, 2_000_000),
            (ConnectResult::Timeout, 3_000_000),
            (ConnectResult::Connected, 20_000_000),
        ]),
        seen: Vec::new(),
    };
    let report = execute(
        &run,
        &interval,
        &schedule,
        &mut clock,
        &mut connector,
        &NeverCancelled,
    )
    .unwrap();
    let result = &report.results()[0];
    assert_eq!(result.samples().len(), 5);
    assert_eq!(result.samples()[0].outcome(), ActiveSampleOutcome::Success);
    assert_eq!(
        result.samples()[1].outcome(),
        ActiveSampleOutcome::ConnectionRefused
    );
    assert!(matches!(
        result.samples()[1].tcp_connect_duration(),
        Evidence::Unknown(_)
    ));
    assert_eq!(result.statistics().connect_timing().successful_samples(), 2);
    assert_eq!(
        result
            .statistics()
            .tcp_attempt_failure_bursts()
            .failed_samples(),
        3
    );
    assert_eq!(
        result
            .statistics()
            .tcp_attempt_failure_bursts()
            .burst_count(),
        1
    );
    assert_eq!(
        result.endpoint_attribution(),
        result.samples()[0].endpoint_attribution()
    );
    assert!(matches!(
        result.statistics().connect_timing().p95(),
        Evidence::Known(_)
    ));
}

#[test]
fn cancellation_and_deadline_emit_one_outcome_per_scheduled_sample() {
    let (run, interval) = make_run(
        vec![endpoint(1, 9, ActiveEndpointTier::LanReference)],
        0.05,
        4,
    );
    let schedule = build_schedule(&run, &interval).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now: now.clone(),
        results: VecDeque::from([(ConnectResult::Connected, 1_000_000)]),
        seen: Vec::new(),
    };
    let cancelled = CancelAfter {
        now: now.clone(),
        at: 1_000_000,
    };
    let report = execute(
        &run,
        &interval,
        &schedule,
        &mut clock,
        &mut connector,
        &cancelled,
    )
    .unwrap();
    let samples = report.results()[0].samples();
    assert_eq!(samples.len(), 4);
    assert!(
        samples[1..]
            .iter()
            .all(|sample| sample.outcome() == ActiveSampleOutcome::Cancelled)
    );
    assert!(
        samples[1..]
            .iter()
            .all(|sample| matches!(sample.tcp_connect_duration(), Evidence::Unknown(_)))
    );

    let (run, interval) = make_run(
        vec![endpoint(2, 9, ActiveEndpointTier::LanReference)],
        0.01,
        2,
    );
    let schedule = build_schedule(&run, &interval).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now: now.clone(),
        results: VecDeque::from([
            (ConnectResult::Connected, 20_000_000),
            (ConnectResult::Connected, 1),
        ]),
        seen: Vec::new(),
    };
    let report = execute(
        &run,
        &interval,
        &schedule,
        &mut clock,
        &mut connector,
        &NeverCancelled,
    )
    .unwrap();
    assert_eq!(
        report.results()[0].samples()[0].outcome(),
        ActiveSampleOutcome::Timeout
    );
    assert_eq!(
        report.results()[0].samples()[1].outcome(),
        ActiveSampleOutcome::Timeout
    );
}

#[test]
fn completion_at_exact_deadline_is_a_timeout() {
    let (run, interval) = make_run(
        vec![endpoint(7, 9, ActiveEndpointTier::LanReference)],
        0.01,
        1,
    );
    let schedule = build_schedule(&run, &interval).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now,
        results: VecDeque::from([(ConnectResult::Connected, 10_000_000)]),
        seen: Vec::new(),
    };
    let report = execute(
        &run,
        &interval,
        &schedule,
        &mut clock,
        &mut connector,
        &NeverCancelled,
    )
    .unwrap();
    let sample = &report.results()[0].samples()[0];
    assert_eq!(sample.outcome(), ActiveSampleOutcome::Timeout);
    assert_eq!(
        sample.tcp_connect_duration(),
        &Evidence::Unknown(UnknownReason::FailedTest)
    );
}

#[test]
fn cancellation_interrupts_long_spacing_and_connector_ports() {
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let cancelled = CancelAfter {
        now: now.clone(),
        at: 25_000_000,
    };
    assert_eq!(
        clock.sleep_for(Duration::from_secs(3_600), &cancelled),
        SleepResult::Cancelled
    );
    assert!(now.get() < Duration::from_secs(3_600).as_nanos() as u64);

    let (run, interval) = make_run_with_spacing(
        vec![endpoint(1, 9, ActiveEndpointTier::LanReference)],
        2.0,
        2,
        1.0,
    );
    let schedule = build_schedule(&run, &interval).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now: now.clone(),
        results: VecDeque::from([(ConnectResult::Connected, 1_000_000)]),
        seen: Vec::new(),
    };
    let cancelled = CancelAfter {
        now: now.clone(),
        at: 25_000_000,
    };
    let report = execute(
        &run,
        &interval,
        &schedule,
        &mut clock,
        &mut connector,
        &cancelled,
    )
    .unwrap();
    assert_eq!(connector.seen.len(), 1);
    assert_eq!(
        report.results()[0].samples()[0].outcome(),
        ActiveSampleOutcome::Success
    );
    assert_eq!(
        report.results()[0].samples()[1].outcome(),
        ActiveSampleOutcome::Cancelled
    );
    assert!(now.get() < 1_000_000_000);

    let (run, interval) = make_run(
        vec![endpoint(2, 9, ActiveEndpointTier::LanReference)],
        1.0,
        2,
    );
    let schedule = build_schedule(&run, &interval).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now: now.clone(),
        results: VecDeque::from([(ConnectResult::Connected, 1_000_000)]),
        seen: Vec::new(),
    };
    let cancelled = CancelAfter {
        now: now.clone(),
        at: 1,
    };
    let report = execute(
        &run,
        &interval,
        &schedule,
        &mut clock,
        &mut connector,
        &cancelled,
    )
    .unwrap();
    assert_eq!(connector.seen.len(), 1);
    assert!(
        report.results()[0]
            .samples()
            .iter()
            .all(|sample| sample.outcome() == ActiveSampleOutcome::Cancelled)
    );
}

#[test]
fn execute_rejects_same_ids_with_changed_schedule_contract() {
    let (run, interval) = make_run(
        vec![endpoint(1, 9, ActiveEndpointTier::LanReference)],
        1.0,
        1,
    );
    let expected = build_schedule(&run, &interval).unwrap();

    let changed_target = ActiveTarget::new(
        text("loopback-test"),
        ActiveEndpointTier::LanReference,
        ActiveSocketAddr::new(ActiveIpAddress::v4([127, 0, 0, 1]), 10).unwrap(),
    )
    .unwrap();
    let changed_endpoint = ActiveEndpoint::new(
        id::<ActiveEndpointId>(1),
        ActiveEndpointTier::LanReference,
        changed_target,
        ActiveTransportProtocol::Tcp,
        ActiveMeasurementMethod::TcpConnectTiming,
        unknown_attribution(),
    )
    .unwrap();
    let mut changed_run = ActiveTestRun::new(
        run.id(),
        run.started(),
        run.deadline(),
        vec![changed_endpoint],
        run.authorization().clone(),
        run.limits(),
        run.provenance().clone(),
    )
    .unwrap();
    let changed_interval = changed_run
        .create_interval(interval.id(), Seconds::new(1.0).unwrap(), 1)
        .unwrap();
    let changed_target_schedule = build_schedule(&changed_run, &changed_interval).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now,
        results: VecDeque::new(),
        seen: Vec::new(),
    };
    assert!(matches!(
        execute(
            &run,
            &interval,
            &changed_target_schedule,
            &mut clock,
            &mut connector,
            &NeverCancelled,
        ),
        Err(kyberia_active_measurement::ActiveMeasurementError::ScheduleMismatch)
    ));

    let changed_authorization = ActiveAuthorization::new(
        true,
        vec![ActiveEndpointTier::LanReference],
        true,
        false,
        text("different-purpose"),
    )
    .unwrap();
    let mut authorization_run = ActiveTestRun::new(
        run.id(),
        run.started(),
        run.deadline(),
        run.endpoints().to_vec(),
        changed_authorization,
        run.limits(),
        run.provenance().clone(),
    )
    .unwrap();
    let authorization_interval = authorization_run
        .create_interval(interval.id(), Seconds::new(1.0).unwrap(), 1)
        .unwrap();
    let authorization_schedule =
        build_schedule(&authorization_run, &authorization_interval).unwrap();
    assert_ne!(authorization_schedule, expected);
    assert_ne!(authorization_schedule.authorization(), run.authorization());
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now,
        results: VecDeque::new(),
        seen: Vec::new(),
    };
    assert!(matches!(
        execute(
            &run,
            &interval,
            &authorization_schedule,
            &mut clock,
            &mut connector,
            &NeverCancelled,
        ),
        Err(kyberia_active_measurement::ActiveMeasurementError::ScheduleMismatch)
    ));

    let changed_limits = ActiveTestLimits::new(
        32,
        2,
        Seconds::new(0.5).unwrap(),
        Seconds::new(1.0).unwrap(),
        Seconds::new(0.0).unwrap(),
    )
    .unwrap();
    let mut limits_run = ActiveTestRun::new(
        run.id(),
        run.started(),
        run.deadline(),
        run.endpoints().to_vec(),
        run.authorization().clone(),
        changed_limits,
        run.provenance().clone(),
    )
    .unwrap();
    let limits_interval = limits_run
        .create_interval(interval.id(), Seconds::new(1.0).unwrap(), 1)
        .unwrap();
    let limits_schedule = build_schedule(&limits_run, &limits_interval).unwrap();
    assert_ne!(limits_schedule.limits(), expected.limits());
    let now = Rc::new(Cell::new(0));
    let mut clock = FakeClock { now: now.clone() };
    let mut connector = FakeConnector {
        now,
        results: VecDeque::new(),
        seen: Vec::new(),
    };
    assert!(matches!(
        execute(
            &run,
            &interval,
            &limits_schedule,
            &mut clock,
            &mut connector,
            &NeverCancelled,
        ),
        Err(kyberia_active_measurement::ActiveMeasurementError::ScheduleMismatch)
    ));
}

proptest! {
    #[test]
    fn percentile_and_burst_statistics_stay_bounded(values in prop::collection::vec(1u32..1000, 1..12)) {
        let (run, interval) = make_run(vec![endpoint(4, 9, ActiveEndpointTier::LanReference)], 2.0, values.len() as u32);
        let mut samples = Vec::new();
        for (ordinal, value) in values.iter().enumerate() {
            let start = MonotonicTimestamp { epoch: run.started().epoch, nanoseconds: ordinal as u64 * 2_000_000 };
            let finish = MonotonicTimestamp { epoch: run.started().epoch, nanoseconds: start.nanoseconds + u64::from(*value) * 1_000_000 };
            samples.push(ActiveSample::new(
                id((ordinal as u8).saturating_add(20)), run.id(), interval.id(), id(4),
                ActiveEndpointTier::LanReference, unknown_attribution(), ordinal as u32,
                start, finish, ActiveSampleOutcome::Success,
                Evidence::Known(Milliseconds::new(f64::from(*value)).unwrap()), run.provenance().clone()
            ).unwrap());
        }
        let stats = kyberia_domain::active::ActiveStatistics::from_samples(&samples).unwrap();
        let min = f64::from(*values.iter().min().unwrap());
        let max = f64::from(*values.iter().max().unwrap());
        for value in [stats.connect_timing().median(), stats.connect_timing().p90(), stats.connect_timing().p95(), stats.connect_timing().p99(), stats.connect_timing().max()] {
            let Evidence::Known(value) = value else { prop_assert!(false); unreachable!() };
            prop_assert!(value.get() >= min && value.get() <= max);
        }
        prop_assert_eq!(stats.tcp_attempt_failure_bursts().failed_samples(), 0);
        prop_assert_eq!(stats.tcp_attempt_failure_bursts().burst_count(), 0);
    }

    #[test]
    fn tcp_attempt_failure_burst_statistics_match_each_failure_run(pattern in prop::collection::vec(any::<bool>(), 1..20)) {
        let (run, interval) = make_run(vec![endpoint(5, 9, ActiveEndpointTier::LanReference)], 2.0, pattern.len() as u32);
        let mut samples = Vec::new();
        for (ordinal, connected) in pattern.iter().copied().enumerate() {
            let start = MonotonicTimestamp { epoch: run.started().epoch, nanoseconds: ordinal as u64 * 2_000_000 };
            let (outcome, timing, elapsed) = if connected {
                (ActiveSampleOutcome::Success, Evidence::Known(Milliseconds::new(1.0).unwrap()), 1_000_000)
            } else {
                (ActiveSampleOutcome::Timeout, Evidence::Unknown(UnknownReason::FailedTest), 0)
            };
            samples.push(ActiveSample::new(
                id((ordinal as u8).saturating_add(40)), run.id(), interval.id(), id(5),
                ActiveEndpointTier::LanReference, unknown_attribution(), ordinal as u32,
                start, MonotonicTimestamp { epoch: start.epoch, nanoseconds: start.nanoseconds + elapsed },
                outcome, timing, run.provenance().clone()
            ).unwrap());
        }
        let stats = kyberia_domain::active::ActiveStatistics::from_samples(&samples).unwrap();
        let expected_failed = pattern.iter().filter(|connected| !**connected).count() as u32;
        let mut expected_failure_bursts = Vec::new();
        let mut current = 0u32;
        for connected in pattern {
            if connected {
                if current > 0 { expected_failure_bursts.push(current); current = 0; }
            } else {
                current += 1;
            }
        }
        if current > 0 { expected_failure_bursts.push(current); }
        prop_assert_eq!(stats.tcp_attempt_failure_bursts().failed_samples(), expected_failed);
        prop_assert_eq!(stats.tcp_attempt_failure_bursts().burst_count(), expected_failure_bursts.len() as u32);
        prop_assert_eq!(stats.eligible_samples(), samples.len() as u32);
        if expected_failure_bursts.is_empty() {
            prop_assert!(matches!(stats.tcp_attempt_failure_bursts().max(), Evidence::Unknown(_)));
        } else {
            prop_assert_eq!(stats.tcp_attempt_failure_bursts().max(), &Evidence::Known(*expected_failure_bursts.iter().max().unwrap()));
        }
    }
}

#[test]
fn real_loopback_adapter_records_success_and_refusal_without_external_network() {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
        eprintln!(
            "SKIP: active loopback integration requires local listener permission; no runtime pass recorded"
        );
        return;
    };
    let success_port = listener.local_addr().unwrap().port();
    let (ready_tx, ready_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        let _ = listener.accept();
    });
    ready_rx.recv().unwrap();
    let refused_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let refused_port = refused_listener.local_addr().unwrap().port();
    drop(refused_listener);
    let endpoints = vec![
        endpoint(2, refused_port, ActiveEndpointTier::LanReference),
        endpoint(1, success_port, ActiveEndpointTier::LanReference),
    ];
    let (run, interval) = make_run(endpoints, 2.0, 1);
    let schedule = build_schedule(&run, &interval).unwrap();
    let mut clock = StdMonotonicClock::new();
    let mut connector = StdTcpConnector::new();
    let report = execute(
        &run,
        &interval,
        &schedule,
        &mut clock,
        &mut connector,
        &NeverCancelled,
    )
    .unwrap();
    assert_eq!(report.results().len(), 2);
    assert_eq!(report.results()[0].endpoint_id(), id(1));
    assert_eq!(
        report.results()[0].samples()[0].outcome(),
        ActiveSampleOutcome::Success
    );
    assert_eq!(report.results()[1].endpoint_id(), id(2));
    assert_eq!(
        report.results()[1].samples()[0].outcome(),
        ActiveSampleOutcome::ConnectionRefused
    );
    server.join().unwrap();
}

#[test]
fn real_loopback_connector_accepts_immediate_peer_close_after_writable_completion() {
    const ATTEMPTS: usize = 32;
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
        eprintln!(
            "SKIP: active loopback integration requires local listener permission; no runtime pass recorded"
        );
        return;
    };
    let success_port = listener.local_addr().unwrap().port();
    let (ready_tx, ready_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        for _ in 0..ATTEMPTS {
            let (stream, _) = listener.accept().unwrap();
            drop(stream);
        }
    });
    ready_rx.recv().unwrap();

    let target = endpoint(1, success_port, ActiveEndpointTier::LanReference)
        .target()
        .address();
    let mut connector = StdTcpConnector::new();
    for _ in 0..ATTEMPTS {
        assert_eq!(
            connector.connect(target, Duration::from_millis(500), &NeverCancelled),
            ConnectResult::Connected
        );
    }
    server.join().unwrap();
}

#[test]
fn pure_module_does_not_import_socket_or_process_apis() {
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/pure.rs")).unwrap();
    assert!(!source.contains("TcpStream"));
    assert!(!source.contains("std::process"));
    assert!(!source.contains("connect_timeout"));
    let domain = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../domain/src/active.rs"
    ))
    .unwrap();
    assert!(!domain.contains("std::net"));
    assert!(!domain.contains("std::process"));
    assert!(!domain.contains("TcpStream"));
    let _ = ScheduleError::RunIntervalMismatch;
    let _ = ActiveSchedule::samples;
}
