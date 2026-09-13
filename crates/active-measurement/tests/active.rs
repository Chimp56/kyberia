use kyberia_active_measurement::{
    ActiveSchedule, Cancellation, ConnectResult, MonotonicClock, NeverCancelled, SCHEDULE_VERSION,
    ScheduleError, StdMonotonicClock, StdTcpConnector, TcpConnector, build_schedule, execute,
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
        ActiveEndpointId, ActiveIntervalId, ActiveSampleId, ActiveTestRunId, AdapterId,
        ClockEpochId, SensorId, Text,
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
        text("rf-atlas-active-tcp-connect/v1"),
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
        ActiveMeasurementMethod::TcpConnectRtt,
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
    let epoch = id(200);
    let started = MonotonicTimestamp {
        epoch,
        nanoseconds: 10_000,
    };
    let deadline = MonotonicTimestamp {
        epoch,
        nanoseconds: 10_000 + (run_duration * 1e9) as u64,
    };
    let run = ActiveTestRun::new(
        id(201),
        started,
        deadline,
        endpoints,
        authorization(vec![ActiveEndpointTier::LanReference], true),
        limits(64, run_duration.min(0.5), run_duration, 0.0),
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
    let mut bad_interval = serde_json::to_value(interval).unwrap();
    bad_interval["samples_per_endpoint"] = json!(0);
    assert!(
        serde_json::from_value::<kyberia_domain::active::ActiveInterval>(bad_interval).is_err()
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
            ActiveMeasurementMethod::TcpConnectRtt,
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
        ActiveMeasurementMethod::TcpConnectRtt,
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
    let (limited_run, limited_interval) = make_run(
        vec![endpoint(1, 9, ActiveEndpointTier::LanReference)],
        1.0,
        2,
    );
    let too_many = ActiveTestRun::new(
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

#[derive(Clone)]
struct FakeClock {
    now: Rc<Cell<u64>>,
}

impl MonotonicClock for FakeClock {
    fn now_nanos(&mut self) -> u64 {
        self.now.get()
    }

    fn sleep_for(&mut self, duration: Duration) {
        self.now
            .set(self.now.get().saturating_add(duration.as_nanos() as u64));
    }
}

struct FakeConnector {
    now: Rc<Cell<u64>>,
    results: VecDeque<(ConnectResult, u64)>,
    seen: Vec<ActiveSocketAddr>,
}

impl TcpConnector for FakeConnector {
    fn connect(&mut self, target: ActiveSocketAddr, _timeout: Duration) -> ConnectResult {
        self.seen.push(target);
        let (result, elapsed) = self
            .results
            .pop_front()
            .unwrap_or((ConnectResult::Error, 0));
        self.now.set(self.now.get().saturating_add(elapsed));
        result
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
        result.samples()[1].tcp_connect_rtt(),
        Evidence::Unknown(_)
    ));
    assert_eq!(result.statistics().rtt().successful_samples(), 2);
    assert_eq!(result.statistics().loss_bursts().lost_samples(), 3);
    assert_eq!(result.statistics().loss_bursts().burst_count(), 1);
    assert_eq!(
        result.endpoint_attribution(),
        result.samples()[0].endpoint_attribution()
    );
    assert!(matches!(
        result.statistics().rtt().p95(),
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
            .all(|sample| matches!(sample.tcp_connect_rtt(), Evidence::Unknown(_)))
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
        for value in [stats.rtt().median(), stats.rtt().p90(), stats.rtt().p95(), stats.rtt().p99(), stats.rtt().max()] {
            let Evidence::Known(value) = value else { prop_assert!(false); unreachable!() };
            prop_assert!(value.get() >= min && value.get() <= max);
        }
        prop_assert_eq!(stats.loss_bursts().lost_samples(), 0);
        prop_assert_eq!(stats.loss_bursts().burst_count(), 0);
    }

    #[test]
    fn loss_burst_statistics_match_each_failure_run(pattern in prop::collection::vec(any::<bool>(), 1..20)) {
        let (run, interval) = make_run(vec![endpoint(5, 9, ActiveEndpointTier::LanReference)], 2.0, pattern.len() as u32);
        let mut samples = Vec::new();
        for (ordinal, connected) in pattern.iter().copied().enumerate() {
            let start = MonotonicTimestamp { epoch: run.started().epoch, nanoseconds: ordinal as u64 * 2_000_000 };
            let (outcome, rtt, elapsed) = if connected {
                (ActiveSampleOutcome::Success, Evidence::Known(Milliseconds::new(1.0).unwrap()), 1_000_000)
            } else {
                (ActiveSampleOutcome::Timeout, Evidence::Unknown(UnknownReason::FailedTest), 0)
            };
            samples.push(ActiveSample::new(
                id((ordinal as u8).saturating_add(40)), run.id(), interval.id(), id(5),
                ActiveEndpointTier::LanReference, unknown_attribution(), ordinal as u32,
                start, MonotonicTimestamp { epoch: start.epoch, nanoseconds: start.nanoseconds + elapsed },
                outcome, rtt, run.provenance().clone()
            ).unwrap());
        }
        let stats = kyberia_domain::active::ActiveStatistics::from_samples(&samples).unwrap();
        let expected_lost = pattern.iter().filter(|connected| !**connected).count() as u32;
        let mut expected_bursts = Vec::new();
        let mut current = 0u32;
        for connected in pattern {
            if connected {
                if current > 0 { expected_bursts.push(current); current = 0; }
            } else {
                current += 1;
            }
        }
        if current > 0 { expected_bursts.push(current); }
        prop_assert_eq!(stats.loss_bursts().lost_samples(), expected_lost);
        prop_assert_eq!(stats.loss_bursts().burst_count(), expected_bursts.len() as u32);
        prop_assert_eq!(stats.eligible_samples(), samples.len() as u32);
        if expected_bursts.is_empty() {
            prop_assert!(matches!(stats.loss_bursts().max(), Evidence::Unknown(_)));
        } else {
            prop_assert_eq!(stats.loss_bursts().max(), &Evidence::Known(*expected_bursts.iter().max().unwrap()));
        }
    }
}

#[test]
fn real_loopback_adapter_records_success_and_refusal_without_external_network() {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
        // Some CI sandboxes prohibit listener creation. The fake connector
        // tests above still cover every lifecycle and outcome branch.
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
