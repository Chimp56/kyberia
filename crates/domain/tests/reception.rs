use kyberia_domain::{
    ValidationError,
    evidence::{Evidence, UnknownReason},
    identity::{ClockEpochId, Text},
    observation::{ObservationEnvelope, ReceivedObservation, SourceResponseTiming},
    time::{CaptureTime, ClockModel, MonotonicTimestamp, MonotonicWindow, UtcTimestamp},
};
use proptest::prelude::*;

fn epoch(n: u8) -> ClockEpochId {
    ClockEpochId::from_bytes([n; 16]).unwrap()
}
fn tick(n: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: epoch(1),
        nanoseconds: n,
    }
}
fn time(monotonic: Evidence<MonotonicTimestamp>) -> CaptureTime {
    CaptureTime {
        wall: Evidence::Unknown(UnknownReason::ClockUnavailable),
        monotonic,
        synchronization: Evidence::Unknown(UnknownReason::SourceDidNotProvide),
    }
}
fn envelope(monotonic: Evidence<MonotonicTimestamp>) -> ObservationEnvelope {
    let mut d: ObservationEnvelope =
        serde_json::from_str(include_str!("fixtures/observation-v1.json")).unwrap();
    let mut data = d.into_data();
    data.time = time(monotonic);
    data.dwell = Evidence::Unknown(UnknownReason::NotObservable);
    d = ObservationEnvelope::new(data).unwrap();
    d
}
fn response(n: u64) -> SourceResponseTiming {
    SourceResponseTiming::new(
        time(Evidence::Known(tick(n))),
        Evidence::Known(MonotonicWindow::new(tick(10), tick(20)).unwrap()),
    )
    .unwrap()
}

#[test]
fn response_timing_never_becomes_capture_time_or_channel_dwell() {
    let input = envelope(Evidence::Unknown(UnknownReason::SourceDidNotProvide));
    let received = ReceivedObservation::new(input.clone(), Evidence::Known(response(21))).unwrap();
    assert_eq!(received.envelope(), &input);
    let json = serde_json::to_value(&received).unwrap();
    assert_eq!(json["schema_version"], "1");
    assert_eq!(json["envelope"]["schema_version"], "2");
    let decoded: ReceivedObservation = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, received);
    assert!(matches!(
        decoded.envelope().data().time.monotonic,
        Evidence::Unknown(_)
    ));
    assert!(matches!(
        decoded.envelope().data().dwell,
        Evidence::Unknown(_)
    ));
}

#[test]
fn cached_capture_can_precede_api_window_and_foreign_clocks_are_not_compared() {
    ReceivedObservation::new(
        envelope(Evidence::Known(tick(1))),
        Evidence::Known(response(21)),
    )
    .unwrap();
    let foreign = MonotonicTimestamp {
        epoch: epoch(2),
        nanoseconds: u64::MAX,
    };
    ReceivedObservation::new(
        envelope(Evidence::Known(foreign)),
        Evidence::Known(response(21)),
    )
    .unwrap();
    assert_eq!(
        ReceivedObservation::new(
            envelope(Evidence::Known(tick(22))),
            Evidence::Known(response(21))
        )
        .unwrap_err(),
        ValidationError::ReversedTime,
    );
}

#[test]
fn reversed_response_window_and_inconsistent_clock_models_are_rejected() {
    let window = Evidence::Known(MonotonicWindow::new(tick(10), tick(20)).unwrap());
    assert_eq!(
        SourceResponseTiming::new(time(Evidence::Known(tick(19))), window.clone()).unwrap_err(),
        ValidationError::ReversedTime
    );
    let foreign = MonotonicTimestamp {
        epoch: epoch(2),
        nanoseconds: 30,
    };
    assert_eq!(
        SourceResponseTiming::new(time(Evidence::Known(foreign)), window.clone()).unwrap_err(),
        ValidationError::ClockEpochMismatch
    );
    let mut returned = time(Evidence::Unknown(UnknownReason::ClockUnavailable));
    returned.synchronization = Evidence::Known(ClockModel {
        epoch: epoch(2),
        reference_monotonic_nanoseconds: 0,
        reference_utc: UtcTimestamp(0),
        offset_to_reference: Evidence::Unknown(UnknownReason::NotMeasured),
        drift: Evidence::Unknown(UnknownReason::NotMeasured),
        error: Evidence::Unknown(UnknownReason::NotMeasured),
        method_version: Text::new("test/1").unwrap(),
    });
    assert_eq!(
        SourceResponseTiming::new(returned, window).unwrap_err(),
        ValidationError::ClockEpochMismatch
    );
}

#[test]
fn missing_response_information_is_explicit_and_not_fabricated_from_capture() {
    let received = ReceivedObservation::new(
        envelope(Evidence::Known(tick(1))),
        Evidence::Unknown(UnknownReason::NotRetained),
    )
    .unwrap();
    assert_eq!(
        received.source_response(),
        &Evidence::Unknown(UnknownReason::NotRetained)
    );
    let unknown = time(Evidence::Unknown(UnknownReason::ClockUnavailable));
    let timing = SourceResponseTiming::new(
        unknown.clone(),
        Evidence::Unknown(UnknownReason::SourceDidNotProvide),
    )
    .unwrap();
    assert_eq!(timing.returned_at(), &unknown);
    assert!(matches!(timing.api_window(), Evidence::Unknown(_)));
}

#[test]
fn reception_wire_revalidates_version_fields_and_temporal_relations() {
    let baseline = ReceivedObservation::new(
        envelope(Evidence::Known(tick(15))),
        Evidence::Known(response(21)),
    )
    .unwrap();
    let value = serde_json::to_value(baseline).unwrap();
    for version in [
        serde_json::json!("2"),
        serde_json::json!(1),
        serde_json::Value::Null,
    ] {
        let mut bad = value.clone();
        bad["schema_version"] = version;
        assert!(serde_json::from_value::<ReceivedObservation>(bad).is_err());
    }
    for field in ["schema_version", "envelope", "source_response"] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<ReceivedObservation>(bad).is_err());
    }
    let mut bad = value.clone();
    bad["source_response"]["detail"]["returned_at"]["monotonic"]["detail"]["nanoseconds"] =
        serde_json::json!(19);
    assert!(serde_json::from_value::<ReceivedObservation>(bad).is_err());
    let json = serde_json::to_string(&value).unwrap();
    let duplicate = format!("{{\"schema_version\":\"1\",{}", &json[1..]);
    assert!(serde_json::from_str::<ReceivedObservation>(&duplicate).is_err());
}

proptest! {
    #[test]
    fn source_response_order_is_exact_for_large_monotonic_values(start in any::<u64>(), width in 0u64..10000, delay in 0u64..10000) {
        if let Some(end) = start.checked_add(width)
            && let Some(returned) = end.checked_add(delay)
        {
            let timing = SourceResponseTiming::new(time(Evidence::Known(tick(returned))), Evidence::Known(MonotonicWindow::new(tick(start),tick(end)).unwrap())).unwrap();
            let wire = serde_json::to_vec(&timing).unwrap();
            prop_assert_eq!(serde_json::from_slice::<SourceResponseTiming>(&wire).unwrap(), timing);
        }
    }
}
