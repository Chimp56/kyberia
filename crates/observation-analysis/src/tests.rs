use super::*;
use kyberia_domain::{
    analysis::VersionedArtifact,
    capability::{Capability, CapabilityDocument, CapabilityState, RawPayloadPolicy},
    evidence::*,
    identity::*,
    observation::*,
    spatial::{Point3, PoseReference, PositionCovariance},
    time::*,
    units::*,
};
use kyberia_spatial_analysis::{
    CellClass, Extrapolation, Grid, Method, MetricDefinition, MetricDefinitionBinding, Point2,
    SpatialMethod,
};
use kyberia_survey::{
    CaptureMode, ChannelRequirement, PointConfigData, PointId, PointMetric, Target,
};
use std::{collections::BTreeMap, num::NonZeroU32};

const RSSI: MacAddress = MacAddress([0, 1, 2, 3, 4, 5]);

fn text(value: &str) -> Text {
    Text::new(value).unwrap()
}
fn id<const N: usize>(value: u8) -> [u8; N] {
    [value; N]
}
fn epoch() -> ClockEpochId {
    ClockEpochId::from_bytes(id(1)).unwrap()
}
fn stamp(nanoseconds: u64) -> MonotonicTimestamp {
    MonotonicTimestamp {
        epoch: epoch(),
        nanoseconds,
    }
}
fn unknown<T>() -> Evidence<T> {
    Evidence::Unknown(UnknownReason::SourceDidNotProvide)
}
fn anchor() -> PoseReference {
    PoseReference {
        pose_id: PoseId::from_bytes(id(2)).unwrap(),
        frame_id: FrameId::from_bytes(id(3)).unwrap(),
        assignment_version: text("anchor/v1"),
        position: Point3 {
            x: CoordinateMeters::new(1.).unwrap(),
            y: CoordinateMeters::new(2.).unwrap(),
            z: CoordinateMeters::new(0.).unwrap(),
        },
        covariance: Evidence::Known(
            PositionCovariance::new([0.04, 0., 0., 0.04, 0., 0.04]).unwrap(),
        ),
        orientation: unknown(),
        method_version: text("manual/v1"),
    }
}
fn channel() -> ChannelContext {
    ChannelContext {
        band: Evidence::Known(Band::Ghz2_4),
        primary_channel: Evidence::Known(std::num::NonZeroU16::new(1).unwrap()),
        primary_frequency: Evidence::Known(Hertz::new(2_412_000_000.).unwrap()),
        center_frequency: unknown(),
        second_center_frequency: unknown(),
        width: unknown(),
        puncturing: unknown(),
    }
}
fn config() -> PointConfig {
    let collector_id = CollectorId::from_bytes(id(4)).unwrap();
    PointConfig::new(PointConfigData {
        schema_version: SchemaVersion::V1,
        point_id: PointId::from_bytes(id(5)).unwrap(),
        session_id: SessionId::from_bytes(id(6)).unwrap(),
        anchor: anchor(),
        map_calibration: Evidence::Unknown(UnknownReason::NotApplicable),
        source_id: SourceId::from_bytes(id(7)).unwrap(),
        collector_id,
        adapter_version: text("adapter/1"),
        epoch: epoch(),
        capabilities: CapabilityDocument {
            schema_version: SchemaVersion::V1,
            collector_id,
            collector_version: text("adapter/1"),
            probed_at: CaptureTime {
                wall: unknown(),
                monotonic: Evidence::Known(stamp(0)),
                synchronization: unknown(),
            },
            entries: BTreeMap::from([(
                Capability::NearbyScan,
                CapabilityState::Available {
                    evidence: text("fixture"),
                },
            )]),
            raw_payload_policy: RawPayloadPolicy::Discard,
        },
        mode: CaptureMode::Scan,
        required_capabilities: vec![],
        metrics: BTreeMap::from([(PointMetric::Rssi, NonZeroU32::new(1).unwrap())]),
        channels: vec![ChannelRequirement {
            frequency: Hertz::new(2_412_000_000.).unwrap(),
            minimum_dwell: Seconds::new(0.1).unwrap(),
        }],
        minimum_active_time: Seconds::new(0.1).unwrap(),
        maximum_scan_age: Seconds::new(1.).unwrap(),
        target: Target::AnyBssid,
        pose_policy: PosePolicy::ManualAnchor {
            maximum_reported_offset: Meters::new(1.).unwrap(),
        },
        allow_synthetic: false,
        method_version: text("point/v1"),
    })
    .unwrap()
}
fn source() -> SourceDescriptor {
    let config = config();
    SourceDescriptor {
        source_id: config.data().source_id,
        collector_id: config.data().collector_id,
        sensor_id: unknown(),
        adapter_id: unknown(),
        kind: SourceKind::NativeApi,
        source_name: text("fixture"),
        source_version: Evidence::Known(text("1")),
        source_schema_version: text("fixture/1"),
        adapter_name: text("fixture-adapter"),
        adapter_version: config.data().adapter_version.clone(),
        parser_version: text("parser/1"),
        driver_version: unknown(),
        os_version: unknown(),
    }
}
fn observation(
    value: u8,
    rssi: Evidence<Dbm>,
    pose: Evidence<PoseReference>,
    capture: Evidence<MonotonicTimestamp>,
    result_age: Evidence<Seconds>,
    bssid: Evidence<MacAddress>,
) -> ObservationEnvelope {
    ObservationEnvelope::new(EnvelopeData {
        schema_version: ObservationSchemaVersion::V2,
        id: ObservationId::from_bytes(id(value)).unwrap(),
        session_id: config().data().session_id,
        source: source(),
        time: CaptureTime {
            wall: unknown(),
            monotonic: capture,
            synchronization: unknown(),
        },
        pose,
        channel: Evidence::Known(channel()),
        dwell: Evidence::Unknown(UnknownReason::NotObservable),
        privacy: PrivacyState {
            policy_version: text("privacy/v1"),
            identifiers: IdentifierPolicy::ExplicitResearchConsent,
            payload: PayloadRetention::Discarded,
        },
        quality: vec![],
        raw_source: Evidence::Unknown(UnknownReason::NotRetained),
        payload: ObservationPayload::Scan(ScanObservation {
            identity: RadioIdentityEvidence {
                physical_device: unknown(),
                radio: unknown(),
                bss: unknown(),
                bssid,
                ess: unknown(),
                mld: unknown(),
                link_id: unknown(),
                client: unknown(),
                grouping_evidence: unknown(),
            },
            ssid: unknown(),
            signal: SignalReading {
                rssi_dbm: rssi,
                noise_dbm: Evidence::Unknown(UnknownReason::NotObservable),
                chains: vec![],
                calibration: Evidence::Known(CalibrationState::Uncalibrated),
                measurement_method: text("native fixture"),
            },
            information_elements: unknown(),
            result_age,
        }),
    })
    .unwrap()
}
fn strict_observation(value: u8, rssi: f64) -> ObservationEnvelope {
    observation(
        value,
        Evidence::Known(Dbm::new(rssi).unwrap()),
        Evidence::Unknown(UnknownReason::NotMeasured),
        Evidence::Known(stamp(500_000_000)),
        Evidence::Known(Seconds::new(0.).unwrap()),
        Evidence::Known(RSSI),
    )
}
fn metric_for(method: SpatialMethod) -> kyberia_spatial_analysis::MetricDefinitionBinding {
    let definition = MetricDefinition::observed_rssi(method).unwrap();
    let bytes = definition.canonical_bytes().unwrap();
    let artifact = kyberia_domain::analysis::VersionedArtifact {
        version: definition.version().clone(),
        sha256: ContentHash::from_sha256(Sha256::digest(&bytes).into()),
        byte_length: ExactU64::new(bytes.len() as u64),
        media_type: text(kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE),
    };
    definition.bind(artifact).unwrap()
}
fn metric() -> kyberia_spatial_analysis::MetricDefinitionBinding {
    metric_for(SpatialMethod::PointValue)
}
fn spatial_config() -> SpatialConfig {
    SpatialConfig {
        method: Method::PointValue,
        support_radius: Meters::new(1.).unwrap(),
        minimum_locations: 1,
        maximum_neighbors: 1,
        extrapolation: Extrapolation::Disabled,
    }
}
fn spatial_config_for(method: Method) -> SpatialConfig {
    SpatialConfig {
        method,
        support_radius: Meters::new(2.).unwrap(),
        minimum_locations: 1,
        maximum_neighbors: 2,
        extrapolation: Extrapolation::Disabled,
    }
}
fn request(ids: Vec<ObservationId>) -> SelectionRequest {
    SelectionRequest {
        project_id: ProjectId::from_bytes(id(8)).unwrap(),
        project_revision: 3,
        floor_id: FloorId::from_bytes(id(9)).unwrap(),
        frame_id: anchor().frame_id,
        target_bssid: RSSI,
        observation_ids: ids,
        session_scope: Some(config().data().session_id),
        source_scope: Some(config().data().source_id),
        adapter_scope: None,
        allow_uncalibrated: true,
        source: source_binding(),
    }
}
fn source_binding() -> SelectionSourceBinding {
    SelectionSourceBinding::from_verified_query(
        3,
        vec![
            ContentHash::from_sha256([0; 32]),
            ContentHash::from_sha256([1; 32]),
        ],
    )
    .unwrap()
}
fn survey_with_strict(observation: &ObservationEnvelope) -> PointSurvey {
    PointSurvey::start(config(), stamp(0))
        .unwrap()
        .admit(observation, stamp(500_000_000))
        .unwrap()
}
fn survey_at(point_byte: u8, x: f64, y: f64, observation: &ObservationEnvelope) -> PointSurvey {
    let mut data = config().data().clone();
    data.point_id = PointId::from_bytes(id(point_byte)).unwrap();
    data.anchor.position.x = CoordinateMeters::new(x).unwrap();
    data.anchor.position.y = CoordinateMeters::new(y).unwrap();
    let point_config = PointConfig::new(data).unwrap();
    PointSurvey::start(point_config, stamp(0))
        .unwrap()
        .admit(observation, stamp(500_000_000))
        .unwrap()
}
fn receipt_observation(value: u8) -> (PointSurvey, ObservationEnvelope) {
    let observation = observation(
        value,
        Evidence::Known(Dbm::new(-64.).unwrap()),
        Evidence::Unknown(UnknownReason::NotMeasured),
        Evidence::Unknown(UnknownReason::NotMeasured),
        Evidence::Unknown(UnknownReason::SourceDidNotProvide),
        Evidence::Known(RSSI),
    );
    let returned = CaptureTime {
        wall: unknown(),
        monotonic: Evidence::Known(stamp(100)),
        synchronization: unknown(),
    };
    let response = SourceResponseTiming::new(
        returned,
        Evidence::Known(MonotonicWindow::new(stamp(10), stamp(90)).unwrap()),
    )
    .unwrap();
    let received =
        ReceivedObservation::new(observation.clone(), Evidence::Known(response)).unwrap();
    let survey = PointSurvey::start(config(), stamp(0))
        .unwrap()
        .associate_received(&received)
        .unwrap()
        .0;
    (survey, observation)
}

fn mutate_observation(
    observation: ObservationEnvelope,
    mutate: impl FnOnce(&mut EnvelopeData),
) -> ObservationEnvelope {
    let mut data = observation.into_data();
    mutate(&mut data);
    ObservationEnvelope::new(data).unwrap()
}

fn assert_receipt_mutation_rejected(label: &str, mutate: impl FnOnce(&mut EnvelopeData)) {
    let (survey, original) = receipt_observation(20);
    let altered = mutate_observation(original.clone(), mutate);
    let set = ValidatedObservedRssiSet::build(
        request(vec![original.data().id]),
        metric(),
        spatial_config(),
        &[SurveyInput {
            survey: &survey,
            floor_id: FloorId::from_bytes(id(9)).unwrap(),
        }],
        std::slice::from_ref(&altered),
    )
    .unwrap_or_else(|error| panic!("{label}: unexpected build error: {error:?}"));
    assert!(
        matches!(set.rejected()[0].reason, RejectionReason::ScopeMismatch),
        "{label}: {:?}",
        set.rejected()[0].reason
    );
}

type ObservationMutation = Box<dyn FnOnce(&mut EnvelopeData)>;

#[test]
fn strict_manual_anchor_builds_observed_tile_and_binds_manifest() {
    let envelope = strict_observation(1, -58.);
    let survey = survey_with_strict(&envelope);
    let set = ValidatedObservedRssiSet::build(
        request(vec![envelope.data().id]),
        metric(),
        spatial_config(),
        &[SurveyInput {
            survey: &survey,
            floor_id: FloorId::from_bytes(id(9)).unwrap(),
        }],
        std::slice::from_ref(&envelope),
    )
    .unwrap();

    assert_eq!(set.samples().count(), 1);
    let record = set.samples().next().unwrap().record();
    assert_eq!(record.bssid, RSSI);
    assert_eq!(record.position_basis, PositionBasis::SelectedPointAnchor);
    assert!(record.association.is_none());
    assert_eq!(
        set.artifact().byte_length as usize,
        set.canonical_manifest().len()
    );
    assert_eq!(
        set.artifact().sha256,
        set.manifest().content_hash().unwrap()
    );

    let tile = set
        .tile(
            Grid {
                floor_id: record.floor_id,
                frame_id: record.frame_id,
                origin: kyberia_spatial_analysis::Point2 {
                    x: CoordinateMeters::new(0.5).unwrap(),
                    y: CoordinateMeters::new(1.5).unwrap(),
                },
                resolution: Meters::new(1.).unwrap(),
                column_offset: 0,
                row_offset: 0,
                width: 1,
                height: 1,
            },
            || false,
        )
        .unwrap();
    assert_eq!(
        tile.cells[0].class,
        kyberia_spatial_analysis::CellClass::Observed
    );
    assert_eq!(tile.inputs.source_artifact, *set.artifact());
}

#[test]
fn canonical_observed_rssi_variants_drive_real_interpolated_tiles() {
    let first = strict_observation(30, -40.);
    let second = strict_observation(31, -60.);
    let first_survey = survey_at(40, 1., 2., &first);
    let second_survey = survey_at(41, 3., 2., &second);
    let floor_id = FloorId::from_bytes(id(9)).unwrap();
    let frame_id = anchor().frame_id;
    let ids = vec![first.data().id, second.data().id];
    let grid = Grid {
        floor_id,
        frame_id,
        origin: Point2 {
            x: CoordinateMeters::new(1.5).unwrap(),
            y: CoordinateMeters::new(1.5).unwrap(),
        },
        resolution: Meters::new(1.).unwrap(),
        column_offset: 0,
        row_offset: 0,
        width: 1,
        height: 1,
    };

    let nearest = ValidatedObservedRssiSet::build(
        request(ids.clone()),
        metric_for(SpatialMethod::Nearest),
        spatial_config_for(Method::Nearest),
        &[
            SurveyInput {
                survey: &first_survey,
                floor_id,
            },
            SurveyInput {
                survey: &second_survey,
                floor_id,
            },
        ],
        &[first.clone(), second.clone()],
    )
    .unwrap();
    let nearest_tile = nearest.tile(grid, || false).unwrap();
    assert_eq!(nearest_tile.cells[0].class, CellClass::Interpolated);
    assert_eq!(
        nearest_tile.cells[0].value,
        Evidence::Known(Dbm::new(-40.).unwrap())
    );
    assert_eq!(nearest_tile.cells[0].support_locations, 2);
    assert_eq!(
        nearest.manifest().metric.spatial_method,
        SpatialMethod::Nearest
    );

    let idw = ValidatedObservedRssiSet::build(
        request(ids.clone()),
        metric_for(SpatialMethod::InverseDistanceWeighted),
        spatial_config_for(Method::Idw { power: 2. }),
        &[
            SurveyInput {
                survey: &first_survey,
                floor_id,
            },
            SurveyInput {
                survey: &second_survey,
                floor_id,
            },
        ],
        &[first.clone(), second.clone()],
    )
    .unwrap();
    let idw_tile = idw.tile(grid, || false).unwrap();
    assert_eq!(idw_tile.cells[0].class, CellClass::Interpolated);
    let value = idw_tile.cells[0].value.as_known().unwrap().get();
    assert!((value + 50.).abs() < 1e-12, "unexpected IDW value {value}");
    assert_eq!(idw_tile.cells[0].support_locations, 2);
    assert_eq!(
        idw.manifest().metric.spatial_method,
        SpatialMethod::InverseDistanceWeighted
    );

    let reordered = ValidatedObservedRssiSet::build(
        request(vec![ids[1], ids[0]]),
        metric_for(SpatialMethod::InverseDistanceWeighted),
        spatial_config_for(Method::Idw { power: 2. }),
        &[
            SurveyInput {
                survey: &second_survey,
                floor_id,
            },
            SurveyInput {
                survey: &first_survey,
                floor_id,
            },
        ],
        &[second, first],
    )
    .unwrap();
    assert_eq!(idw.canonical_manifest(), reordered.canonical_manifest());
    assert_eq!(
        serde_json::to_vec(&idw.tile(grid, || false).unwrap()).unwrap(),
        serde_json::to_vec(&reordered.tile(grid, || false).unwrap()).unwrap()
    );
}

#[test]
fn selection_requires_exact_spatial_rssi_definition_and_manifest_identity() {
    let envelope = strict_observation(32, -52.);
    let survey = survey_with_strict(&envelope);
    let floor_id = FloorId::from_bytes(id(9)).unwrap();
    let input = [SurveyInput {
        survey: &survey,
        floor_id,
    }];
    let observations = [envelope.clone()];

    assert!(matches!(
        ValidatedObservedRssiSet::build(
            request(vec![envelope.data().id]),
            metric_for(SpatialMethod::Nearest),
            spatial_config(),
            &input,
            &observations,
        ),
        Err(SelectionError::InvalidRequest(
            "metric is not canonical observed RSSI"
        ))
    ));
    assert!(matches!(
        ValidatedObservedRssiSet::build(
            request(vec![envelope.data().id]),
            metric(),
            spatial_config_for(Method::Nearest),
            &input,
            &observations,
        ),
        Err(SelectionError::InvalidRequest(
            "metric is not canonical observed RSSI"
        ))
    ));

    let nearest = MetricDefinition::signal_rssi_nearest().unwrap();
    let nearest_bytes = nearest.canonical_bytes().unwrap();
    let mut impostor_bytes = nearest_bytes.clone();
    let original_id = b"wifi.rssi.nearest";
    let replacement_id = b"wifi.test.nearest";
    let offset = impostor_bytes
        .windows(original_id.len())
        .position(|window| window == original_id)
        .unwrap();
    impostor_bytes[offset..offset + original_id.len()].copy_from_slice(replacement_id);
    let impostor = MetricDefinitionBinding::from_artifact_bytes(
        VersionedArtifact {
            version: text("wifi.test.nearest/1"),
            sha256: ContentHash::from_sha256(Sha256::digest(&impostor_bytes).into()),
            byte_length: ExactU64::new(impostor_bytes.len() as u64),
            media_type: text(kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE),
        },
        &impostor_bytes,
        nearest.signal_aggregation(),
    )
    .unwrap();
    assert!(matches!(
        ValidatedObservedRssiSet::build(
            request(vec![envelope.data().id]),
            impostor,
            spatial_config_for(Method::Nearest),
            &input,
            &observations,
        ),
        Err(SelectionError::InvalidRequest(
            "metric is not canonical observed RSSI"
        ))
    ));

    let valid = ValidatedObservedRssiSet::build(
        request(vec![envelope.data().id]),
        metric_for(SpatialMethod::Nearest),
        spatial_config_for(Method::Nearest),
        &input,
        &observations,
    )
    .unwrap();
    let mut forged = valid.manifest().clone();
    let forged_hash = ContentHash::from_sha256([0xabu8; 32]);
    forged.metric.artifact.sha256 = forged_hash;
    forged.metric.definition_hash = forged_hash;
    assert!(matches!(
        SelectionManifest::from_canonical_bytes(&serde_json::to_vec(&forged).unwrap()),
        Err(SelectionError::InvalidManifest("metric identity"))
    ));
}

#[test]
fn measured_selection_rejects_synthetic_source_and_quality_evidence() {
    let original = strict_observation(35, -54.);
    let survey = survey_with_strict(&original);
    let floor_id = FloorId::from_bytes(id(9)).unwrap();
    let input = [SurveyInput {
        survey: &survey,
        floor_id,
    }];
    let synthetic_source = mutate_observation(original.clone(), |data| {
        data.source.kind = SourceKind::SyntheticFixture;
        data.quality.push(QualityFlag::SyntheticFixture);
    });
    let synthetic_source_result = ValidatedObservedRssiSet::build(
        request(vec![original.data().id]),
        metric(),
        spatial_config(),
        &input,
        std::slice::from_ref(&synthetic_source),
    )
    .unwrap();
    assert!(matches!(
        synthetic_source_result.rejected()[0].reason,
        RejectionReason::UnsupportedPayload
    ));

    let synthetic_quality = mutate_observation(original.clone(), |data| {
        data.quality.push(QualityFlag::SyntheticFixture);
    });
    let synthetic_quality_result = ValidatedObservedRssiSet::build(
        request(vec![original.data().id]),
        metric(),
        spatial_config(),
        &input,
        std::slice::from_ref(&synthetic_quality),
    )
    .unwrap();
    assert!(matches!(
        synthetic_quality_result.rejected()[0].reason,
        RejectionReason::UnusableQuality
    ));
    assert!(synthetic_quality_result.samples().next().is_none());
    assert_eq!(
        synthetic_quality_result.manifest().evidence_plane,
        SelectionEvidencePlane::Measured
    );
}

#[test]
fn interpolated_rssi_gap_remains_unknown_without_support() {
    let first = strict_observation(33, -40.);
    let second = strict_observation(34, -60.);
    let first_survey = survey_at(42, 1., 2., &first);
    let second_survey = survey_at(43, 3., 2., &second);
    let floor_id = FloorId::from_bytes(id(9)).unwrap();
    let frame_id = anchor().frame_id;
    let mut config = spatial_config_for(Method::Idw { power: 2. });
    config.support_radius = Meters::new(0.5).unwrap();
    config.maximum_neighbors = 2;
    let set = ValidatedObservedRssiSet::build(
        request(vec![first.data().id, second.data().id]),
        metric_for(SpatialMethod::InverseDistanceWeighted),
        config,
        &[
            SurveyInput {
                survey: &first_survey,
                floor_id,
            },
            SurveyInput {
                survey: &second_survey,
                floor_id,
            },
        ],
        &[first, second],
    )
    .unwrap();
    let tile = set
        .tile(
            Grid {
                floor_id,
                frame_id,
                origin: Point2 {
                    x: CoordinateMeters::new(5.5).unwrap(),
                    y: CoordinateMeters::new(1.5).unwrap(),
                },
                resolution: Meters::new(1.).unwrap(),
                column_offset: 0,
                row_offset: 0,
                width: 1,
                height: 1,
            },
            || false,
        )
        .unwrap();
    assert_eq!(tile.cells[0].class, CellClass::Unknown);
    assert_eq!(
        tile.cells[0].value,
        Evidence::Unknown(UnknownReason::OutsideEvidenceSupport)
    );
}

#[test]
fn receipt_selection_keeps_receipt_timing_and_unknown_capture() {
    let (survey, envelope) = receipt_observation(2);
    let set = ValidatedObservedRssiSet::build(
        request(vec![envelope.data().id]),
        metric(),
        spatial_config(),
        &[SurveyInput {
            survey: &survey,
            floor_id: FloorId::from_bytes(id(9)).unwrap(),
        }],
        std::slice::from_ref(&envelope),
    )
    .unwrap();
    let record = set.samples().next().unwrap().record();
    assert_eq!(record.position_basis, PositionBasis::SelectedPointAnchor);
    assert!(record.association.is_some());
    assert!(matches!(
        record.capture_time.monotonic,
        Evidence::Unknown(UnknownReason::NotMeasured)
    ));
    assert!(matches!(
        record.association_time.as_ref().unwrap().basis,
        AssociationTimeKind::Receipt
    ));
    assert!(matches!(
        record.result_age,
        Evidence::Unknown(UnknownReason::SourceDidNotProvide)
    ));
    assert!(
        !record
            .association
            .as_ref()
            .unwrap()
            .association_hash
            .bytes()
            .iter()
            .all(|byte| *byte == 0)
    );
}

#[test]
fn exact_bssid_filter_preserves_rejection_and_unknown_rssi() {
    let other = observation(
        3,
        Evidence::Known(Dbm::new(-55.).unwrap()),
        Evidence::Unknown(UnknownReason::NotMeasured),
        Evidence::Known(stamp(500_000_000)),
        Evidence::Known(Seconds::new(0.).unwrap()),
        Evidence::Known(MacAddress([9, 9, 9, 9, 9, 9])),
    );
    let survey = survey_with_strict(&other);
    let set = ValidatedObservedRssiSet::build(
        request(vec![other.data().id]),
        metric(),
        spatial_config(),
        &[SurveyInput {
            survey: &survey,
            floor_id: FloorId::from_bytes(id(9)).unwrap(),
        }],
        std::slice::from_ref(&other),
    )
    .unwrap();
    assert!(set.samples().next().is_none());
    assert!(matches!(
        set.rejected()[0].reason,
        RejectionReason::WrongBssid
    ));

    let unknown = observation(
        4,
        Evidence::Unknown(UnknownReason::UnsupportedCapability),
        Evidence::Unknown(UnknownReason::NotMeasured),
        Evidence::Known(stamp(500_000_000)),
        Evidence::Known(Seconds::new(0.).unwrap()),
        Evidence::Known(RSSI),
    );
    let survey = survey_with_strict(&unknown);
    let set = ValidatedObservedRssiSet::build(
        request(vec![unknown.data().id]),
        metric(),
        spatial_config(),
        &[SurveyInput {
            survey: &survey,
            floor_id: FloorId::from_bytes(id(9)).unwrap(),
        }],
        std::slice::from_ref(&unknown),
    )
    .unwrap();
    assert!(matches!(
        set.rejected()[0].reason,
        RejectionReason::UnknownRssi(UnknownReason::UnsupportedCapability)
    ));

    let denied_observation = strict_observation(7, -59.);
    let denied_survey = survey_with_strict(&denied_observation);
    let mut denied_request = request(vec![denied_observation.data().id]);
    denied_request.allow_uncalibrated = false;
    let denied = ValidatedObservedRssiSet::build(
        denied_request,
        metric(),
        spatial_config(),
        &[SurveyInput {
            survey: &denied_survey,
            floor_id: FloorId::from_bytes(id(9)).unwrap(),
        }],
        std::slice::from_ref(&denied_observation),
    )
    .unwrap();
    assert!(matches!(
        denied.rejected()[0].reason,
        RejectionReason::Uncalibrated
    ));
}

#[test]
fn selection_is_canonical_and_manifest_is_strict() {
    let one = strict_observation(5, -61.);
    let two = strict_observation(6, -62.);
    let survey_one = survey_with_strict(&one);
    let survey_two = survey_with_strict(&two);
    let first = ValidatedObservedRssiSet::build(
        request(vec![two.data().id, one.data().id]),
        metric(),
        spatial_config(),
        &[
            SurveyInput {
                survey: &survey_two,
                floor_id: FloorId::from_bytes(id(9)).unwrap(),
            },
            SurveyInput {
                survey: &survey_one,
                floor_id: FloorId::from_bytes(id(9)).unwrap(),
            },
        ],
        &[two.clone(), one.clone()],
    )
    .unwrap();
    let second = ValidatedObservedRssiSet::build(
        request(vec![one.data().id, two.data().id]),
        metric(),
        spatial_config(),
        &[
            SurveyInput {
                survey: &survey_one,
                floor_id: FloorId::from_bytes(id(9)).unwrap(),
            },
            SurveyInput {
                survey: &survey_two,
                floor_id: FloorId::from_bytes(id(9)).unwrap(),
            },
        ],
        &[one, two],
    )
    .unwrap();
    assert_eq!(first.canonical_manifest(), second.canonical_manifest());
    assert_eq!(
        SelectionManifest::from_canonical_bytes(first.canonical_manifest()).unwrap(),
        *first.manifest()
    );

    let mut malformed = first.canonical_manifest().to_vec();
    malformed.insert(malformed.len() - 1, b' ');
    assert!(SelectionManifest::from_canonical_bytes(&malformed).is_err());

    let mut tampered = first.manifest().clone();
    tampered.selected[0].assignment_pose.position.x = CoordinateMeters::new(99.).unwrap();
    assert!(
        SelectionManifest::from_canonical_bytes(&serde_json::to_vec(&tampered).unwrap()).is_err()
    );
    let mut policy_tampered = first.manifest().clone();
    policy_tampered.policy.allow_uncalibrated = false;
    assert!(
        SelectionManifest::from_canonical_bytes(&serde_json::to_vec(&policy_tampered).unwrap())
            .is_err()
    );
    let mut quality_tampered = first.manifest().clone();
    quality_tampered.selected[0]
        .quality
        .push(QualityFlag::Malformed);
    assert!(
        SelectionManifest::from_canonical_bytes(&serde_json::to_vec(&quality_tampered).unwrap())
            .is_err()
    );
    let mut pose_policy_tampered = first.manifest().clone();
    pose_policy_tampered.selected[0].pose_policy = PosePolicy::RequireReported {
        maximum_offset: Meters::new(1.).unwrap(),
        maximum_axis_stddev: Meters::new(1.).unwrap(),
    };
    assert!(
        SelectionManifest::from_canonical_bytes(
            &serde_json::to_vec(&pose_policy_tampered).unwrap()
        )
        .is_err()
    );

    let deep = format!(
        "{}null{}",
        "[".repeat(MAX_MANIFEST_DEPTH + 1),
        "]".repeat(MAX_MANIFEST_DEPTH + 1)
    );
    assert!(matches!(
        SelectionManifest::from_canonical_bytes(deep.as_bytes()),
        Err(SelectionError::ResourceLimit("selection manifest depth"))
    ));
    assert!(matches!(
        ValidatedObservedRssiSet::build(
            request(vec![one_id(), one_id()]),
            metric(),
            spatial_config(),
            &[],
            &[],
        ),
        Err(SelectionError::DuplicateObservation(_))
    ));
}

#[test]
fn receipt_selection_rejects_every_copied_association_field_mutation() {
    let cases: [(&str, ObservationMutation); 10] = [
        (
            "session",
            Box::new(|data| data.session_id = SessionId::from_bytes(id(40)).unwrap()),
        ),
        (
            "source",
            Box::new(|data| data.source.source_id = SourceId::from_bytes(id(41)).unwrap()),
        ),
        (
            "collector",
            Box::new(|data| data.source.collector_id = CollectorId::from_bytes(id(42)).unwrap()),
        ),
        (
            "adapter version",
            Box::new(|data| data.source.adapter_version = text("adapter/changed")),
        ),
        (
            "capture time",
            Box::new(|data| {
                data.time.monotonic = Evidence::Known(stamp(501));
            }),
        ),
        (
            "raw source",
            Box::new(|data| {
                data.raw_source = Evidence::Unknown(UnknownReason::SourceDidNotProvide);
            }),
        ),
        (
            "pose",
            Box::new(|data| data.pose = Evidence::Known(anchor())),
        ),
        (
            "dwell",
            Box::new(|data| {
                data.dwell = Evidence::Unknown(UnknownReason::SourceDidNotProvide);
            }),
        ),
        (
            "channel",
            Box::new(|data| {
                data.channel = Evidence::Unknown(UnknownReason::SourceDidNotProvide);
            }),
        ),
        (
            "parser version",
            Box::new(|data| data.source.parser_version = text("parser/changed")),
        ),
    ];
    for (label, mutate) in cases {
        assert_receipt_mutation_rejected(label, mutate);
    }

    assert_receipt_mutation_rejected("result age", |data| {
        if let ObservationPayload::Scan(scan) = &mut data.payload {
            scan.result_age = Evidence::Known(Seconds::new(0.).unwrap());
        }
    });
    assert_receipt_mutation_rejected("calibration", |data| {
        if let ObservationPayload::Scan(scan) = &mut data.payload {
            scan.signal.calibration = Evidence::Unknown(UnknownReason::SourceDidNotProvide);
        }
    });
    assert_receipt_mutation_rejected("quality", |data| {
        data.quality.push(QualityFlag::Throttled);
    });
}

#[test]
fn strict_selection_rejects_scientifically_unsafe_inputs() {
    let original = strict_observation(21, -60.);
    let survey = survey_with_strict(&original);

    let wrong_frame = mutate_observation(original.clone(), |data| {
        let mut pose = anchor();
        pose.frame_id = FrameId::from_bytes(id(43)).unwrap();
        data.pose = Evidence::Known(pose);
    });
    let stale_scan = mutate_observation(original.clone(), |data| {
        if let ObservationPayload::Scan(scan) = &mut data.payload {
            scan.result_age = Evidence::Known(Seconds::new(2.).unwrap());
        }
    });
    let outside_calibration = mutate_observation(original.clone(), |data| {
        if let ObservationPayload::Scan(scan) = &mut data.payload {
            scan.signal.calibration = Evidence::Known(CalibrationState::OutsideValidRange {
                id: CalibrationId::from_bytes(id(44)).unwrap(),
                version: text("calibration/1"),
            });
        }
    });
    let unknown_bssid = mutate_observation(original.clone(), |data| {
        if let ObservationPayload::Scan(scan) = &mut data.payload {
            scan.identity.bssid = Evidence::Unknown(UnknownReason::UnsupportedCapability);
        }
    });
    let changed_rssi = mutate_observation(original.clone(), |data| {
        if let ObservationPayload::Scan(scan) = &mut data.payload {
            scan.signal.rssi_dbm = Evidence::Known(Dbm::new(-61.).unwrap());
        }
    });
    let changed_time = mutate_observation(original.clone(), |data| {
        data.time.monotonic = Evidence::Known(stamp(400_000_000));
    });
    let changed_pose = mutate_observation(original.clone(), |data| {
        let mut pose = anchor();
        pose.position.x = CoordinateMeters::new(1.1).unwrap();
        data.pose = Evidence::Known(pose);
    });
    let unsupported_health = mutate_observation(original.clone(), |data| {
        data.payload = ObservationPayload::Health(CaptureHealth {
            dropped_events: unknown(),
            queued_events: unknown(),
            connected: unknown(),
            diagnostic: text("fixture health"),
        });
    });

    let cases = [
        (
            "wrong reported frame",
            wrong_frame,
            RejectionReason::AdmissionMismatch,
        ),
        ("stale scan", stale_scan, RejectionReason::AdmissionMismatch),
        (
            "outside calibration",
            outside_calibration,
            RejectionReason::AdmissionMismatch,
        ),
        (
            "unknown BSSID",
            unknown_bssid,
            RejectionReason::AdmissionMismatch,
        ),
        (
            "health payload",
            unsupported_health,
            RejectionReason::AdmissionMismatch,
        ),
        (
            "changed RSSI",
            changed_rssi,
            RejectionReason::AdmissionMismatch,
        ),
        (
            "changed time",
            changed_time,
            RejectionReason::AdmissionMismatch,
        ),
        (
            "changed pose",
            changed_pose,
            RejectionReason::AdmissionMismatch,
        ),
    ];
    for (label, altered, expected) in cases {
        let set = ValidatedObservedRssiSet::build(
            request(vec![original.data().id]),
            metric(),
            spatial_config(),
            &[SurveyInput {
                survey: &survey,
                floor_id: FloorId::from_bytes(id(9)).unwrap(),
            }],
            std::slice::from_ref(&altered),
        )
        .unwrap_or_else(|error| panic!("{label}: unexpected build error: {error:?}"));
        assert_eq!(set.rejected()[0].reason, expected, "{label}");
    }

    let mut wrong_spatial_method = spatial_config();
    wrong_spatial_method.method = Method::Nearest;
    assert!(matches!(
        ValidatedObservedRssiSet::build(
            request(vec![original.data().id]),
            metric(),
            wrong_spatial_method,
            &[SurveyInput {
                survey: &survey,
                floor_id: FloorId::from_bytes(id(9)).unwrap(),
            }],
            std::slice::from_ref(&original),
        ),
        Err(SelectionError::InvalidRequest(
            "metric is not canonical observed RSSI"
        ))
    ));

    let definition = MetricDefinition::signal_rssi().unwrap();
    let mut altered_bytes = definition.canonical_bytes().unwrap();
    let signal = b"signal";
    let replacement = b"noisex";
    let offset = altered_bytes
        .windows(signal.len())
        .position(|window| window == signal)
        .unwrap();
    altered_bytes[offset..offset + signal.len()].copy_from_slice(replacement);
    let altered_artifact = kyberia_domain::analysis::VersionedArtifact {
        version: definition.version().clone(),
        sha256: ContentHash::from_sha256(Sha256::digest(&altered_bytes).into()),
        byte_length: ExactU64::new(altered_bytes.len() as u64),
        media_type: text(kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE),
    };
    let incompatible_metric =
        kyberia_spatial_analysis::MetricDefinitionBinding::from_artifact_bytes(
            altered_artifact,
            &altered_bytes,
            metric().signal_aggregation(),
        )
        .unwrap();
    assert!(matches!(
        ValidatedObservedRssiSet::build(
            request(vec![original.data().id]),
            incompatible_metric,
            spatial_config(),
            &[SurveyInput {
                survey: &survey,
                floor_id: FloorId::from_bytes(id(9)).unwrap(),
            }],
            std::slice::from_ref(&original),
        ),
        Err(SelectionError::InvalidRequest(
            "metric is not canonical observed RSSI"
        ))
    ));
}

fn one_id() -> ObservationId {
    ObservationId::from_bytes(id(1)).unwrap()
}

#[test]
fn resource_limits_reject_before_selection() {
    let id = one_id();
    let mut ids = vec![id; MAX_OBSERVATIONS + 1];
    assert!(matches!(
        ValidatedObservedRssiSet::build(
            request(std::mem::take(&mut ids)),
            metric(),
            spatial_config(),
            &[],
            &[],
        ),
        Err(SelectionError::ResourceLimit("observation IDs"))
    ));
}

#[test]
fn maximum_unique_selection_is_bounded_and_preserves_unknowns() {
    let ids = (0..MAX_OBSERVATIONS)
        .map(|value| {
            let mut bytes = [0; 16];
            bytes[..8].copy_from_slice(&((value + 1) as u64).to_be_bytes());
            ObservationId::from_bytes(bytes).unwrap()
        })
        .collect::<Vec<_>>();
    let set = ValidatedObservedRssiSet::build(request(ids), metric(), spatial_config(), &[], &[])
        .unwrap();
    assert_eq!(set.samples().count(), 0);
    assert_eq!(set.rejected().len(), MAX_OBSERVATIONS);
    assert!(set.canonical_manifest().len() < MAX_MANIFEST_BYTES);
    assert!(
        set.rejected()
            .iter()
            .all(|record| matches!(record.reason, RejectionReason::MissingObservation))
    );
}

#[test]
fn supplied_observation_without_a_survey_assignment_is_distinctly_rejected() {
    let envelope = strict_observation(22, -63.);
    let set = ValidatedObservedRssiSet::build(
        request(vec![envelope.data().id]),
        metric(),
        spatial_config(),
        &[],
        std::slice::from_ref(&envelope),
    )
    .unwrap();
    assert_eq!(set.samples().count(), 0);
    assert!(matches!(
        set.rejected()[0].reason,
        RejectionReason::MissingAssociation
    ));
}

#[test]
fn source_binding_is_revision_bound_and_strictly_bounded() {
    let mut request = request(vec![one_id()]);
    request.source.project_revision = ExactU64::new(4);
    assert!(matches!(
        ValidatedObservedRssiSet::build(request, metric(), spatial_config(), &[], &[],),
        Err(SelectionError::InvalidManifest("source revision"))
    ));

    assert!(matches!(
        SelectionSourceBinding::from_verified_query(
            3,
            vec![
                ContentHash::from_sha256([1; 32]),
                ContentHash::from_sha256([0; 32]),
            ],
        ),
        Err(SelectionError::InvalidRequest(
            "source chunk hashes are not strictly sorted"
        ))
    ));
    assert!(matches!(
        SelectionSourceBinding::from_verified_query(3, vec![]),
        Err(SelectionError::InvalidRequest(
            "source receipt has no selected chunks"
        ))
    ));
    let too_many = (0..=MAX_SOURCE_CHUNKS)
        .map(|value| {
            ContentHash::from_sha256((value as u8).to_le_bytes().repeat(32).try_into().unwrap())
        })
        .collect();
    assert!(matches!(
        SelectionSourceBinding::from_verified_query(3, too_many),
        Err(SelectionError::ResourceLimit("source chunks"))
    ));
}
