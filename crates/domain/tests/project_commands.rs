use kyberia_domain::{evidence::*, identity::*, project::*, spatial::*, units::*};
use proptest::prelude::*;
use std::num::{NonZeroU32, NonZeroU64};

fn text(s: &str) -> Text {
    Text::new(s).unwrap()
}
fn pixel(x: f64, y: f64) -> PixelPoint {
    PixelPoint {
        x: Pixels::new(x).unwrap(),
        y: Pixels::new(y).unwrap(),
    }
}
fn point(x: f64, y: f64) -> Point2 {
    Point2 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
    }
}
fn frame(n: u8, kind: FrameKind) -> CoordinateFrame {
    CoordinateFrame {
        id: FrameId::from_bytes([n; 16]).unwrap(),
        name: text("frame"),
        kind,
    }
}
fn calibration() -> TwoPointCalibration {
    TwoPointCalibration::new(CalibrationControls {
        source_frame: FrameId::from_bytes([3; 16]).unwrap(),
        target_frame: FrameId::from_bytes([2; 16]).unwrap(),
        image_first: pixel(10., 20.),
        image_second: pixel(110., 20.),
        target_origin: point(2., 3.),
        known_distance: Meters::new(10.).unwrap(),
        target_direction: Radians::new(0.).unwrap(),
        image_y_axis: ImageYAxis::Down,
        distance_uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
        control_point_uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
    })
    .unwrap()
}
fn project() -> Project {
    Project::new(ProjectId::from_bytes([1; 16]).unwrap(), text("Home"))
}
fn request(project: &Project, n: u8, command: ProjectCommand) -> CommandRequest {
    CommandRequest {
        schema_version: SchemaVersion::V1,
        operation_id: OperationId::from_bytes([n; 16]).unwrap(),
        project_id: project.id(),
        actor_id: ActorId::from_bytes([1; 16]).unwrap(),
        device_id: ActorDeviceId::from_bytes([1; 16]).unwrap(),
        logical_time: NonZeroU64::new(project.logical_time() + 1).unwrap(),
        expected_revision: project.revision(),
        wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
        command,
    }
}
fn populated() -> Project {
    let mut state = project();
    for (n, command) in [
        (
            1,
            ProjectCommand::CreateSite(Site {
                id: SiteId::from_bytes([1; 16]).unwrap(),
                name: text("Site"),
            }),
        ),
        (
            2,
            ProjectCommand::CreateBuilding(
                Building::new(BuildingData {
                    id: BuildingId::from_bytes([1; 16]).unwrap(),
                    site_id: SiteId::from_bytes([1; 16]).unwrap(),
                    name: text("Building"),
                    frame: frame(1, FrameKind::BuildingLocalMeters),
                })
                .unwrap(),
            ),
        ),
        (
            3,
            ProjectCommand::CreateFloor(
                Floor::new(FloorData {
                    id: FloorId::from_bytes([1; 16]).unwrap(),
                    building_id: BuildingId::from_bytes([1; 16]).unwrap(),
                    name: text("Ground"),
                    frame: frame(2, FrameKind::FloorLocalMeters),
                    building_frame: FrameId::from_bytes([1; 16]).unwrap(),
                    origin: Point3 {
                        x: CoordinateMeters::new(0.).unwrap(),
                        y: CoordinateMeters::new(0.).unwrap(),
                        z: CoordinateMeters::new(-1.).unwrap(),
                    },
                    yaw: Radians::new(0.).unwrap(),
                    clear_height: Meters::new(2.5).unwrap(),
                })
                .unwrap(),
            ),
        ),
        (
            4,
            ProjectCommand::ImportMap(
                MapAsset::new(MapAssetData {
                    id: MapAssetId::from_bytes([1; 16]).unwrap(),
                    floor_id: FloorId::from_bytes([1; 16]).unwrap(),
                    name: text("Map"),
                    image_frame: frame(3, FrameKind::ImagePixels),
                    width: NonZeroU32::new(200).unwrap(),
                    height: NonZeroU32::new(200).unwrap(),
                    source: ArtifactReference {
                        sha256: ContentHash::from_sha256([1; 32]),
                        media_type: text("image/png"),
                        byte_length: 42,
                    },
                    provenance: text("original test fixture"),
                })
                .unwrap(),
            ),
        ),
    ] {
        state = state.execute(request(&state, n, command)).unwrap().project;
    }
    state
}

#[test]
fn known_scale_origin_image_handedness_and_inverse() {
    let transform = calibration();
    let mapped = transform
        .to_floor(FrameId::from_bytes([3; 16]).unwrap(), pixel(110., 30.))
        .unwrap();
    assert!((mapped.x.get() - 12.).abs() < 1e-12);
    assert!((mapped.y.get() - 2.).abs() < 1e-12);
    let back = transform
        .to_image(FrameId::from_bytes([2; 16]).unwrap(), mapped)
        .unwrap();
    assert!((back.x.get() - 110.).abs() < 1e-12);
    assert!((back.y.get() - 30.).abs() < 1e-12);
    assert!(
        transform
            .to_floor(FrameId::from_bytes([9; 16]).unwrap(), pixel(0., 0.))
            .is_err()
    );
}

#[test]
fn invalid_calibration_cannot_enter_via_constructor_or_serde() {
    let mut controls = calibration().controls().clone();
    controls.image_second = controls.image_first;
    assert!(TwoPointCalibration::new(controls.clone()).is_err());
    assert!(
        serde_json::from_value::<TwoPointCalibration>(serde_json::to_value(controls).unwrap())
            .is_err()
    );
    let mut controls = calibration().controls().clone();
    controls.known_distance = Meters::new(0.).unwrap();
    assert!(TwoPointCalibration::new(controls).is_err());
    let mut controls = calibration().controls().clone();
    controls.target_frame = controls.source_frame;
    assert!(TwoPointCalibration::new(controls).is_err());
    let mut controls = calibration().controls().clone();
    controls.control_point_uncertainty = Evidence::Known(Pixels::new(-1.).unwrap());
    assert!(TwoPointCalibration::new(controls).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn calibrated_pixel_round_trip(x in -1e4_f64..1e4,y in -1e4_f64..1e4,angle in -std::f64::consts::PI..std::f64::consts::PI) {
        let mut controls=calibration().controls().clone();controls.target_direction=Radians::new(angle).unwrap();
        let transform=TwoPointCalibration::new(controls).unwrap();
        let floor=transform.to_floor(transform.controls().source_frame,pixel(x,y)).unwrap();
        let back=transform.to_image(transform.controls().target_frame,floor).unwrap();
        prop_assert!((back.x.get()-x).abs()<1e-8);
        prop_assert!((back.y.get()-y).abs()<1e-8);
    }
}

#[test]
fn hierarchy_rejects_missing_parent_duplicate_frames_and_bad_height() {
    let state = populated();
    let floor = state
        .floor(FloorId::from_bytes([1; 16]).unwrap())
        .unwrap()
        .clone();
    let mut bad = floor.data().clone();
    bad.clear_height = Meters::new(0.).unwrap();
    assert!(Floor::new(bad).is_err());
    let mut bad = floor.data().clone();
    bad.id = FloorId::from_bytes([2; 16]).unwrap();
    assert!(
        state
            .execute(request(
                &state,
                9,
                ProjectCommand::CreateFloor(Floor::new(bad.clone()).unwrap())
            ))
            .is_err()
    );
    bad.frame = frame(9, FrameKind::FloorLocalMeters);
    bad.building_id = BuildingId::from_bytes([9; 16]).unwrap();
    assert!(
        state
            .execute(request(
                &state,
                9,
                ProjectCommand::CreateFloor(Floor::new(bad).unwrap())
            ))
            .is_err()
    );
}

#[test]
fn commands_are_deterministic_atomic_and_replay_validates_receipts() {
    let state = project();
    let command = request(
        &state,
        1,
        ProjectCommand::CreateSite(Site {
            id: SiteId::from_bytes([1; 16]).unwrap(),
            name: text("Site"),
        }),
    );
    let a = state.execute(command.clone()).unwrap();
    let b = state.execute(command).unwrap();
    assert_eq!(a, b);
    assert_eq!(state.revision(), 0);
    assert_eq!(state.replay(&a.record).unwrap(), a.project);
    let mut tampered = serde_json::to_value(&a.record).unwrap();
    tampered["revision_after"] = serde_json::json!(42);
    let tampered: OperationRecord = serde_json::from_value(tampered).unwrap();
    assert!(state.replay(&tampered).is_err());
    assert!(a.project.execute(a.record.request.clone()).is_err());
}

#[test]
fn name_changes_preserve_geometry_and_have_replayable_exact_inverses() {
    let original = populated();
    let site_id = SiteId::from_bytes([1; 16]).unwrap();
    for command in [
        ProjectCommand::SetProjectName {
            name: text("Office"),
        },
        ProjectCommand::SetSiteName {
            site_id,
            name: text("West campus"),
        },
    ] {
        let renamed = original
            .execute(request(&original, 20, command.clone()))
            .unwrap();
        assert_eq!(
            renamed,
            original.execute(request(&original, 20, command)).unwrap()
        );
        assert_eq!(renamed.project.id(), original.id());
        assert_eq!(renamed.project.revision(), original.revision() + 1);
        assert_eq!(
            renamed.project.floor(FloorId::from_bytes([1; 16]).unwrap()),
            original.floor(FloorId::from_bytes([1; 16]).unwrap())
        );
        assert_eq!(
            renamed
                .project
                .building(BuildingId::from_bytes([1; 16]).unwrap()),
            original.building(BuildingId::from_bytes([1; 16]).unwrap())
        );
        assert_eq!(
            renamed
                .project
                .map(MapAssetId::from_bytes([1; 16]).unwrap()),
            original.map(MapAssetId::from_bytes([1; 16]).unwrap())
        );
        let encoded = serde_json::to_vec(&renamed.record).unwrap();
        let decoded: OperationRecord = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(original.replay(&decoded).unwrap(), renamed.project);
        let inverse = renamed.record.undo.as_known().unwrap().clone();
        let restored = renamed
            .project
            .execute(request(&renamed.project, 21, inverse))
            .unwrap();
        assert_eq!(restored.project.name(), original.name());
        assert_eq!(restored.project.site(site_id), original.site(site_id));
        let redo = restored.record.undo.as_known().unwrap().clone();
        let redone = restored
            .project
            .execute(request(&restored.project, 22, redo))
            .unwrap();
        assert_eq!(redone.project.name(), renamed.project.name());
        assert_eq!(redone.project.site(site_id), renamed.project.site(site_id));
        match renamed.record.event {
            ProjectEvent::ProjectNameChanged { previous, current } => {
                assert_eq!(previous.as_str(), "Home");
                assert_eq!(current.as_str(), "Office");
                assert_eq!(renamed.project.name().as_str(), "Office");
                assert_eq!(renamed.project.site(site_id), original.site(site_id));
            }
            ProjectEvent::SiteNameChanged {
                site_id: actual,
                previous,
                current,
            } => {
                assert_eq!(actual, site_id);
                assert_eq!(previous.as_str(), "Site");
                assert_eq!(current.as_str(), "West campus");
                assert_eq!(
                    renamed.project.site(site_id).unwrap().name.as_str(),
                    "West campus"
                );
                assert_eq!(renamed.project.name(), original.name());
            }
            _ => panic!("name command returned an unrelated event"),
        }
    }
    assert_eq!(original.name().as_str(), "Home");
    assert_eq!(original.site(site_id).unwrap().name.as_str(), "Site");
}

#[test]
fn name_receipts_reject_forged_prior_values_and_stale_or_missing_targets() {
    let state = populated();
    let missing = ProjectCommand::SetSiteName {
        site_id: SiteId::from_bytes([99; 16]).unwrap(),
        name: text("missing"),
    };
    assert_eq!(
        state.execute(request(&state, 20, missing)),
        Err(ProjectError::MissingEntity)
    );
    let applied = state
        .execute(request(
            &state,
            20,
            ProjectCommand::SetProjectName {
                name: text("Office"),
            },
        ))
        .unwrap();
    let mut forged = applied.record.clone();
    forged.undo = Evidence::Known(ProjectCommand::SetProjectName {
        name: text("forged"),
    });
    assert_eq!(state.replay(&forged), Err(ProjectError::InvalidReceipt));
    let mut forged = applied.record.clone();
    if let ProjectEvent::ProjectNameChanged { previous, .. } = &mut forged.event {
        *previous = text("forged");
    }
    assert_eq!(state.replay(&forged), Err(ProjectError::InvalidReceipt));
    let stale = request(
        &state,
        21,
        ProjectCommand::SetProjectName {
            name: text("stale"),
        },
    );
    assert!(matches!(
        applied.project.execute(stale),
        Err(ProjectError::RevisionConflict { .. })
    ));
    assert_eq!(state.name().as_str(), "Home");
    assert_eq!(state.revision(), 4);
}

#[test]
fn calibration_undo_retains_history_and_evidence_binding_prevents_scale_changes() {
    let state = populated();
    let change = MapCalibration {
        id: CalibrationId::from_bytes([1; 16]).unwrap(),
        map_id: MapAssetId::from_bytes([1; 16]).unwrap(),
        transform: calibration(),
        provenance: text("known tape distance"),
        method_version: text("two-point/v1"),
    };
    let applied = state
        .execute(request(
            &state,
            5,
            ProjectCommand::CalibrateMap(change.clone()),
        ))
        .unwrap();
    assert_eq!(
        applied.project.active_calibration(change.map_id).unwrap(),
        Evidence::Known(change.id)
    );
    let undo = applied.record.undo.as_known().unwrap().clone();
    let restored = applied
        .project
        .execute(request(&applied.project, 6, undo))
        .unwrap()
        .project;
    assert!(matches!(
        restored.active_calibration(change.map_id).unwrap(),
        Evidence::Unknown(UnknownReason::NotMeasured)
    ));
    assert_eq!(restored.calibration(change.id), Some(&change));
    assert_eq!(
        restored
            .execute(request(
                &restored,
                9,
                ProjectCommand::RemoveMap {
                    map_id: change.map_id
                }
            ))
            .unwrap_err(),
        ProjectError::HasDependents
    );
    let locked = applied
        .project
        .execute(request(
            &applied.project,
            7,
            ProjectCommand::BindFloorEvidence {
                floor_id: FloorId::from_bytes([1; 16]).unwrap(),
                evidence: ArtifactReference {
                    sha256: ContentHash::from_sha256([7; 32]),
                    media_type: text("application/json"),
                    byte_length: 100,
                },
            },
        ))
        .unwrap()
        .project;
    let mut next = change;
    next.id = CalibrationId::from_bytes([2; 16]).unwrap();
    assert!(
        locked
            .execute(request(&locked, 8, ProjectCommand::CalibrateMap(next)))
            .is_err()
    );
    assert!(
        locked
            .execute(request(
                &locked,
                8,
                ProjectCommand::RemoveMap {
                    map_id: MapAssetId::from_bytes([1; 16]).unwrap()
                }
            ))
            .is_err()
    );
}

#[test]
fn serialized_project_revalidates_references_and_preserves_order() {
    let state = populated();
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(
        serde_json::from_value::<Project>(json.clone()).unwrap(),
        state
    );
    let mut invalid = json;
    invalid["sites"] = serde_json::json!({});
    assert!(serde_json::from_value::<Project>(invalid).is_err());
}

#[test]
fn floors_preserve_signed_elevation_rotation_and_coordinate_frames() {
    let state = populated();
    let original = state.floor(FloorId::from_bytes([1; 16]).unwrap()).unwrap();
    let mut data = original.data().clone();
    data.yaw = Radians::new(std::f64::consts::FRAC_PI_2).unwrap();
    data.origin.x = CoordinateMeters::new(10.).unwrap();
    let floor = Floor::new(data).unwrap();
    let local = Point3 {
        x: CoordinateMeters::new(2.).unwrap(),
        y: CoordinateMeters::new(3.).unwrap(),
        z: CoordinateMeters::new(0.5).unwrap(),
    };
    let mapped = floor.to_building(floor.data().frame.id, local).unwrap();
    assert!((mapped.x.get() - 7.).abs() < 1e-12);
    assert!((mapped.y.get() - 2.).abs() < 1e-12);
    assert_eq!(mapped.z.get(), -0.5);
    let back = floor
        .from_building(floor.data().building_frame, mapped)
        .unwrap();
    assert!((back.x.get() - local.x.get()).abs() < 1e-12);
    assert!((back.y.get() - local.y.get()).abs() < 1e-12);
    assert_eq!(back.z, local.z);
    assert!(
        floor
            .to_building(floor.data().building_frame, local)
            .is_err()
    );
}

#[test]
fn duplicates_stale_revisions_and_forged_undo_fail_atomically() {
    let state = populated();
    let original = serde_json::to_vec(&state).unwrap();
    let site = state
        .site(SiteId::from_bytes([1; 16]).unwrap())
        .unwrap()
        .clone();
    let mut stale = request(&state, 9, ProjectCommand::CreateSite(site.clone()));
    stale.expected_revision = 0;
    assert!(matches!(
        state.execute(stale),
        Err(ProjectError::RevisionConflict { .. })
    ));
    let mut wrong = request(&state, 9, ProjectCommand::CreateSite(site.clone()));
    wrong.project_id = ProjectId::from_bytes([9; 16]).unwrap();
    assert_eq!(
        state.execute(wrong).unwrap_err(),
        ProjectError::WrongProject
    );
    assert_eq!(
        state
            .execute(request(&state, 9, ProjectCommand::CreateSite(site)))
            .unwrap_err(),
        ProjectError::DuplicateEntity
    );
    assert_eq!(serde_json::to_vec(&state).unwrap(), original);
    let operation = state
        .execute(request(
            &state,
            9,
            ProjectCommand::CreateSite(Site {
                id: SiteId::from_bytes([9; 16]).unwrap(),
                name: text("New site"),
            }),
        ))
        .unwrap();
    let mut record = operation.record;
    record.undo = Evidence::Unknown(UnknownReason::NotApplicable);
    assert_eq!(
        state.replay(&record).unwrap_err(),
        ProjectError::InvalidReceipt
    );
}

#[test]
fn import_and_calibration_keep_assets_and_frame_ownership_separate() {
    let state = populated();
    let mut map = state
        .map(MapAssetId::from_bytes([1; 16]).unwrap())
        .unwrap()
        .data()
        .clone();
    map.id = MapAssetId::from_bytes([2; 16]).unwrap();
    map.image_frame = frame(4, FrameKind::ImagePixels);
    let updated = state
        .execute(request(
            &state,
            5,
            ProjectCommand::ImportMap(MapAsset::new(map).unwrap()),
        ))
        .unwrap()
        .project;
    let bad = MapCalibration {
        id: CalibrationId::from_bytes([1; 16]).unwrap(),
        map_id: MapAssetId::from_bytes([2; 16]).unwrap(),
        transform: calibration(),
        provenance: text("test"),
        method_version: text("two-point/v1"),
    };
    assert_eq!(
        updated
            .execute(request(&updated, 6, ProjectCommand::CalibrateMap(bad)))
            .unwrap_err(),
        ProjectError::InvalidReference
    );
    // Same content can belong to two distinct map identities without losing
    // either floor association; content deduplication belongs to storage.
    assert_eq!(
        updated
            .map(MapAssetId::from_bytes([1; 16]).unwrap())
            .unwrap()
            .data()
            .source
            .sha256,
        updated
            .map(MapAssetId::from_bytes([2; 16]).unwrap())
            .unwrap()
            .data()
            .source
            .sha256
    );
}

#[test]
fn duplicate_wire_identity_keys_are_rejected_even_when_values_match() {
    let state = populated();
    let site = state.sites().next().unwrap();
    let id: String = site.id.into();
    let entry = format!("\"{id}\":{}", serde_json::to_string(site).unwrap());
    let wire = serde_json::to_string(&state).unwrap();
    let duplicate = wire.replacen(
        &format!("\"sites\":{{{entry}}}"),
        &format!("\"sites\":{{{entry},{entry}}}"),
        1,
    );
    assert_ne!(duplicate, wire);
    assert!(serde_json::from_str::<Project>(&duplicate).is_err());
}

#[test]
fn operation_sequence_replays_and_divergent_branch_reports_conflict() {
    let initial = project();
    let mut state = initial.clone();
    let mut records = Vec::new();
    for n in 1..=10 {
        let applied = state
            .execute(request(
                &state,
                n,
                ProjectCommand::CreateSite(Site {
                    id: SiteId::from_bytes([n; 16]).unwrap(),
                    name: text("Site"),
                }),
            ))
            .unwrap();
        records.push(applied.record);
        state = applied.project;
    }
    let wire = serde_json::to_string(&records).unwrap();
    let decoded: Vec<OperationRecord> = serde_json::from_str(&wire).unwrap();
    let replayed = decoded
        .iter()
        .try_fold(initial.clone(), |state, record| state.replay(record))
        .unwrap();
    assert_eq!(replayed, state);
    let divergent = request(
        &initial,
        99,
        ProjectCommand::CreateSite(Site {
            id: SiteId::from_bytes([99; 16]).unwrap(),
            name: text("Other branch"),
        }),
    );
    assert!(matches!(
        state.execute(divergent),
        Err(ProjectError::RevisionConflict { .. })
    ));
    assert!(initial.replay(&records[1]).is_err());
}

#[test]
fn calibration_bounds_and_handedness_are_explicit() {
    let state = populated();
    let mut controls = calibration().controls().clone();
    controls.image_second = pixel(201., 20.);
    let invalid = MapCalibration {
        id: CalibrationId::from_bytes([1; 16]).unwrap(),
        map_id: MapAssetId::from_bytes([1; 16]).unwrap(),
        transform: TwoPointCalibration::new(controls).unwrap(),
        provenance: text("test"),
        method_version: text("two-point/v1"),
    };
    assert!(
        state
            .execute(request(&state, 5, ProjectCommand::CalibrateMap(invalid)))
            .is_err()
    );
    let mut controls = calibration().controls().clone();
    controls.image_y_axis = ImageYAxis::Up;
    let transform = TwoPointCalibration::new(controls).unwrap();
    let point = transform
        .to_floor(transform.controls().source_frame, pixel(10., 30.))
        .unwrap();
    assert_eq!(point.y.get(), 4.);
    let mut wire = serde_json::to_value(transform).unwrap();
    wire["known_distance"] = serde_json::json!("NaN");
    assert!(serde_json::from_value::<TwoPointCalibration>(wire).is_err());
}

#[test]
fn large_finite_direction_keeps_the_control_segment_rotation() {
    let mut controls = calibration().controls().clone();
    controls.image_first = pixel(0., 0.);
    controls.image_second = pixel(100., 100.);
    controls.target_origin = point(0., 0.);
    controls.target_direction = Radians::new(1e16).unwrap();
    controls.image_y_axis = ImageYAxis::Up;
    let transform = TwoPointCalibration::new(controls.clone()).unwrap();
    let decoded: TwoPointCalibration =
        serde_json::from_value(serde_json::to_value(controls).unwrap()).unwrap();
    let (expected_sine, expected_cosine) = 1e16_f64.sin_cos();
    for transform in [transform, decoded] {
        let result = transform
            .to_floor(transform.controls().source_frame, pixel(100., 100.))
            .unwrap();
        assert!((result.x.get() - 10. * expected_cosine).abs() < 1e-12);
        assert!((result.y.get() - 10. * expected_sine).abs() < 1e-12);
    }
}
