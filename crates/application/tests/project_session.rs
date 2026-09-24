use kyberia_application::{
    Application, ApplicationError, CalibrateMapIntent, CalibrateMapRequest, CreateProject,
    CreateProjectWithInitialFloor, ImportMapIntent, ImportMapRequest, MapIntentAuthority,
    MapOperationContext, OpenProject, ProjectQuery, ProjectQueryResult, ProjectState, SessionMode,
};
use kyberia_domain::{
    evidence::{ArtifactReference, Evidence, SchemaVersion, UnknownReason},
    identity::{
        BuildingId, CalibrationId, FloorId, MapAssetId, OperationId, ProjectId, SiteId, Text,
    },
    project::{
        Building, BuildingData, CommandRequest, Floor, FloorData, InitialProjectHierarchy,
        MapAsset, MapAssetData, MapCalibration, Project, ProjectCommand, Site,
    },
    spatial::{
        CalibrationControls, CoordinateFrame, FrameKind, ImageYAxis, PixelPoint, Point2, Point3,
        TwoPointCalibration,
    },
    units::{CoordinateMeters, Meters, Pixels, Radians},
};
use kyberia_operation_log::{
    CausalDepth, ImmutableReference, InversePrior, LogicalTimestamp, Mutation, NonReversibleReason,
    Operation, ProjectVersion,
};
use kyberia_project_store::{ArtifactEntry, ArtifactKind, Bundle, OpenMode};
use kyberia_resource_budget::{CancellationHook, ResourceBudget, ResourceLimits};
use std::num::NonZeroU64;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

fn retained_directory() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
    fs::create_dir_all(&root).unwrap();
    loop {
        let candidate = root.join(format!(
            "application-project-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot retain test directory {candidate:?}: {error}"),
        }
    }
}

fn create_request(path: &Path, name: &str) -> CreateProject {
    CreateProject {
        path: path.to_path_buf(),
        name: Text::new(name).unwrap(),
        created_utc_ms: 1,
    }
}

fn current(
    session: &kyberia_application::ProjectSession,
) -> kyberia_application::CurrentProjectView {
    let ProjectQueryResult::CurrentSnapshot(view) =
        session.query(ProjectQuery::CurrentSnapshot).unwrap();
    view
}

fn identity<T>(value: u8) -> T
where
    T: TryFrom<String>,
    <T as TryFrom<String>>::Error: std::fmt::Debug,
{
    T::try_from(format!("{value:02x}").repeat(16)).unwrap()
}

fn frame(value: u8, kind: FrameKind) -> CoordinateFrame {
    CoordinateFrame {
        id: identity(value),
        name: Text::new("frame").unwrap(),
        kind,
    }
}

fn execute_baseline(project: Project, operation: u8, command: ProjectCommand) -> Project {
    project
        .execute(CommandRequest {
            schema_version: SchemaVersion::V1,
            operation_id: identity(operation),
            project_id: project.id(),
            actor_id: identity(90),
            device_id: identity(91),
            logical_time: NonZeroU64::new(u64::from(operation - 199)).unwrap(),
            expected_revision: project.revision(),
            wall_time: Evidence::Unknown(UnknownReason::ClockUnavailable),
            command,
        })
        .unwrap()
        .project
}

fn project_with_floor() -> (Project, FloorId, CoordinateFrame, CoordinateFrame) {
    let project_id: ProjectId = identity(1);
    let site_id: SiteId = identity(2);
    let building_id: BuildingId = identity(3);
    let floor_id: FloorId = identity(4);
    let building_frame = frame(5, FrameKind::BuildingLocalMeters);
    let floor_frame = frame(6, FrameKind::FloorLocalMeters);
    let image_frame = frame(7, FrameKind::ImagePixels);
    let project = Project::new(project_id, Text::new("Maps").unwrap());
    let project = execute_baseline(
        project,
        200,
        ProjectCommand::CreateSite(Site {
            id: site_id,
            name: Text::new("Site").unwrap(),
        }),
    );
    let project = execute_baseline(
        project,
        201,
        ProjectCommand::CreateBuilding(
            Building::new(BuildingData {
                id: building_id,
                site_id,
                name: Text::new("Building").unwrap(),
                frame: building_frame.clone(),
            })
            .unwrap(),
        ),
    );
    let project = execute_baseline(
        project,
        202,
        ProjectCommand::CreateFloor(
            Floor::new(FloorData {
                id: floor_id,
                building_id,
                name: Text::new("Floor").unwrap(),
                frame: floor_frame.clone(),
                building_frame: building_frame.id,
                origin: Point3 {
                    x: CoordinateMeters::new(0.0).unwrap(),
                    y: CoordinateMeters::new(0.0).unwrap(),
                    z: CoordinateMeters::new(0.0).unwrap(),
                },
                yaw: Radians::new(0.0).unwrap(),
                clear_height: Meters::new(2.5).unwrap(),
            })
            .unwrap(),
        ),
    );
    (project, floor_id, floor_frame, image_frame)
}

fn initial_hierarchy() -> InitialProjectHierarchy {
    let site_id: SiteId = identity(40);
    let building_id: BuildingId = identity(41);
    let floor_id: FloorId = identity(42);
    let building_frame = frame(43, FrameKind::BuildingLocalMeters);
    let floor_frame = frame(44, FrameKind::FloorLocalMeters);
    InitialProjectHierarchy {
        site: Site {
            id: site_id,
            name: Text::new("Site").unwrap(),
        },
        building: Building::new(BuildingData {
            id: building_id,
            site_id,
            name: Text::new("Building").unwrap(),
            frame: building_frame.clone(),
        })
        .unwrap(),
        floor: Floor::new(FloorData {
            id: floor_id,
            building_id,
            name: Text::new("Floor 1").unwrap(),
            frame: floor_frame,
            building_frame: building_frame.id,
            origin: Point3 {
                x: CoordinateMeters::new(0.0).unwrap(),
                y: CoordinateMeters::new(0.0).unwrap(),
                z: CoordinateMeters::new(0.0).unwrap(),
            },
            yaw: Radians::new(0.0).unwrap(),
            clear_height: Meters::new(2.5).unwrap(),
        })
        .unwrap(),
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&out[4..]).to_be_bytes());
        out
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &[0x78, 1, 1]));
    out.extend(chunk(b"IEND", &[]));
    out
}

fn limits() -> ResourceLimits {
    ResourceLimits::new(
        16_000_000,
        16_000_000,
        8_000_000,
        64 * 1024 * 1024,
        64 * 1024 * 1024,
        128 * 1024 * 1024,
    )
}

fn context(
    operation: u8,
    logical: u64,
    depth: u64,
    parents: Vec<OperationId>,
    revision: u64,
    utc: i64,
) -> MapOperationContext {
    MapOperationContext {
        operation_id: identity(operation),
        actor_id: identity(20),
        device_id: identity(21),
        logical_time: LogicalTimestamp::new(logical).unwrap(),
        causal_depth: CausalDepth::new(depth),
        parents,
        expected_project_revision: ProjectVersion::new(revision),
        committed_utc_ms: utc,
    }
}

fn calibration(
    id: u8,
    map_id: MapAssetId,
    image: &CoordinateFrame,
    floor: &CoordinateFrame,
    distance: f64,
) -> MapCalibration {
    MapCalibration {
        id: identity(id),
        map_id,
        transform: TwoPointCalibration::new(CalibrationControls {
            source_frame: image.id,
            target_frame: floor.id,
            image_first: PixelPoint {
                x: Pixels::new(10.0).unwrap(),
                y: Pixels::new(20.0).unwrap(),
            },
            image_second: PixelPoint {
                x: Pixels::new(110.0).unwrap(),
                y: Pixels::new(20.0).unwrap(),
            },
            target_origin: Point2 {
                x: CoordinateMeters::new(2.0).unwrap(),
                y: CoordinateMeters::new(3.0).unwrap(),
            },
            known_distance: Meters::new(distance).unwrap(),
            target_direction: Radians::new(0.0).unwrap(),
            image_y_axis: ImageYAxis::Down,
            distance_uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
            control_point_uncertainty: Evidence::Unknown(UnknownReason::NotMeasured),
        })
        .unwrap(),
        provenance: Text::new("two selected controls").unwrap(),
        method_version: Text::new("two-point-v1").unwrap(),
    }
}

#[test]
fn create_then_reopen_returns_the_same_canonical_baseline() {
    let root = retained_directory();
    let path = root.join("home.rfatlas");
    let app = Application;
    let created = app.create(create_request(&path, "Home")).unwrap();
    assert_eq!(created.mode(), SessionMode::ReadWrite);
    let first = current(&created);
    assert_eq!(first.state(), ProjectState::BaselineOnly);
    assert_eq!(first.manifest_name(), "Home");
    assert_eq!(first.project().unwrap().name().as_str(), "Home");
    assert_eq!(first.revision().unwrap().project_revision(), 0);
    assert_eq!(first.revision().unwrap().bundle_revision(), 1);

    drop(created);
    let reopened = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap();
    assert_eq!(reopened.mode(), SessionMode::ReadOnly);
    assert_eq!(current(&reopened), first);
}

#[test]
fn map_import_and_calibration_are_idempotent_published_and_reopenable() {
    let root = retained_directory();
    let path = root.join("maps.rfatlas");
    let (baseline, floor_id, floor_frame, image_frame) = project_with_floor();
    let mut bundle = Bundle::create(&path, baseline.id(), "Maps".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 1)
        .unwrap();
    drop(bundle);

    let app = Application;
    let mut session = app
        .open(OpenProject {
            path: path.clone(),
            mode: SessionMode::ReadWrite,
        })
        .unwrap();
    let map_id: MapAssetId = identity(30);
    let import_id: OperationId = identity(31);
    let import = ImportMapRequest {
        context: context(31, 4, 0, vec![], 0, 2),
        map_id,
        floor_id,
        name: Text::new("Ground plan").unwrap(),
        image_frame: image_frame.clone(),
        provenance: Text::new("fixture-import-1").unwrap(),
    };
    let bytes = png(320, 200);
    let mut budget = ResourceBudget::new(limits());
    let first = session
        .import_map_with_budget(import.clone(), &bytes, &mut budget)
        .unwrap();
    assert_eq!(first.project_revision.value(), 1);
    let mut retry_budget = ResourceBudget::new(limits());
    assert_eq!(
        session
            .import_map_with_budget(import, &bytes, &mut retry_budget)
            .unwrap(),
        first
    );
    let imported = current(&session);
    let map = imported.project().unwrap().map(map_id).unwrap();
    assert_eq!(
        (map.data().width.get(), map.data().height.get()),
        (320, 200)
    );
    assert_eq!(map.data().source.media_type.as_str(), "image/png");
    assert!(!map.data().provenance.as_str().contains('/'));

    let calibration_id: CalibrationId = identity(32);
    let calibrate_id: OperationId = identity(33);
    let request = CalibrateMapRequest {
        context: context(33, 5, 1, vec![import_id], 1, 3),
        calibration: calibration(32, map_id, &image_frame, &floor_frame, 20.0),
        prior_active: Evidence::Unknown(UnknownReason::NotMeasured),
    };
    let mut budget = ResourceBudget::new(limits());
    let calibrated = session
        .calibrate_map_with_budget(request.clone(), &mut budget)
        .unwrap();
    assert_eq!(calibrated.project_revision.value(), 2);
    let mut retry_budget = ResourceBudget::new(limits());
    assert_eq!(
        session
            .calibrate_map_with_budget(request, &mut retry_budget)
            .unwrap(),
        calibrated
    );
    let view = current(&session);
    let project = view.project().unwrap();
    assert_eq!(
        project.active_calibration(map_id),
        Some(Evidence::Known(calibration_id))
    );
    let point = project
        .calibration(calibration_id)
        .unwrap()
        .transform
        .to_floor(
            image_frame.id,
            PixelPoint {
                x: Pixels::new(110.0).unwrap(),
                y: Pixels::new(20.0).unwrap(),
            },
        )
        .unwrap();
    assert!((point.x.get() - 22.0).abs() < 1e-12);
    assert!((point.y.get() - 3.0).abs() < 1e-12);

    drop(session);
    let mut reopened = app
        .open(OpenProject {
            path: path.clone(),
            mode: SessionMode::ReadOnly,
        })
        .unwrap();
    assert_eq!(
        current(&reopened)
            .project()
            .unwrap()
            .active_calibration(map_id),
        Some(Evidence::Known(calibration_id))
    );
    let mut budget = ResourceBudget::new(limits());
    let error = reopened
        .calibrate_map_with_budget(
            CalibrateMapRequest {
                context: context(34, 6, 2, vec![calibrate_id], 2, 4),
                calibration: calibration(35, map_id, &image_frame, &floor_frame, 30.0),
                prior_active: Evidence::Known(calibration_id),
            },
            &mut budget,
        )
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::ReadOnly);

    drop(reopened);
    let bind_id: OperationId = identity(36);
    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let bind = Operation::try_apply_v2_non_reversible(
        bind_id,
        baseline.id(),
        identity(20),
        identity(21),
        LogicalTimestamp::new(6).unwrap(),
        CausalDepth::new(2),
        vec![calibrate_id],
        Mutation::bind_floor_evidence(
            floor_id,
            ImmutableReference::new(
                kyberia_domain::identity::ContentHash::from_sha256([9; 32]),
                Text::new("application/x-rfatlas-survey").unwrap(),
                1,
            )
            .unwrap(),
        ),
        NonReversibleReason::FloorEvidenceBinding,
    )
    .unwrap();
    writer
        .append_operation_if_revision(bind, Some(ProjectVersion::new(2)))
        .unwrap();
    let operations = writer.operation_set().unwrap();
    let materialized = kyberia_causal_materializer::materialize(&baseline, &operations).unwrap();
    writer
        .publish_materialized_project(
            &baseline,
            &operations,
            &materialized,
            ProjectVersion::new(3),
            4,
        )
        .unwrap();
    drop(writer);

    let mut writer_session = app
        .open(OpenProject {
            path: path.clone(),
            mode: SessionMode::ReadWrite,
        })
        .unwrap();
    let before_locked = current(&writer_session);
    let mut budget = ResourceBudget::new(limits());
    let locked = writer_session
        .calibrate_map_with_budget(
            CalibrateMapRequest {
                context: context(37, 7, 3, vec![bind_id], 3, 5),
                calibration: calibration(38, map_id, &image_frame, &floor_frame, 30.0),
                prior_active: Evidence::Known(calibration_id),
            },
            &mut budget,
        )
        .unwrap_err();
    assert_eq!(
        locked.kind(),
        kyberia_application::ErrorKind::InvalidRequest
    );
    assert_eq!(current(&writer_session), before_locked);
    drop(writer_session);
    assert_eq!(
        Bundle::open(&path, OpenMode::ReadOnly)
            .unwrap()
            .operation_store_state()
            .unwrap()
            .project_revision(),
        ProjectVersion::new(3)
    );
}

#[test]
fn cancelled_or_stale_map_mutation_does_not_publish_a_new_current_project() {
    #[derive(Debug)]
    struct Cancelled;
    impl CancellationHook for Cancelled {
        fn is_cancelled(&mut self) -> bool {
            true
        }
    }

    let root = retained_directory();
    let path = root.join("cancelled-map.rfatlas");
    let (baseline, floor_id, _, image_frame) = project_with_floor();
    let mut bundle = Bundle::create(&path, baseline.id(), "Maps".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 1)
        .unwrap();
    drop(bundle);
    let mut session = Application
        .open(OpenProject {
            path,
            mode: SessionMode::ReadWrite,
        })
        .unwrap();
    let request = ImportMapRequest {
        context: context(40, 4, 0, vec![], 1, 2),
        map_id: identity(41),
        floor_id,
        name: Text::new("No publish").unwrap(),
        image_frame,
        provenance: Text::new("fixture-import-2").unwrap(),
    };
    let before = current(&session);
    let mut cancelled = ResourceBudget::with_cancellation(limits(), Cancelled);
    assert_eq!(
        session
            .import_map_with_budget(request.clone(), &png(1, 1), &mut cancelled)
            .unwrap_err()
            .kind(),
        kyberia_application::ErrorKind::Cancelled
    );
    assert_eq!(current(&session), before);
    let mut budget = ResourceBudget::new(limits());
    assert_eq!(
        session
            .import_map_with_budget(request, &png(1, 1), &mut budget)
            .unwrap_err()
            .kind(),
        kyberia_application::ErrorKind::Conflict
    );
    assert_eq!(current(&session).project(), before.project());
}

#[test]
fn duplicate_create_is_structured_and_does_not_replace_the_existing_project() {
    let root = retained_directory();
    let path = root.join("protected.rfatlas");
    let app = Application;
    let created = app.create(create_request(&path, "Original")).unwrap();
    let before = current(&created);
    let error = app
        .create(create_request(&path, "Replacement"))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        kyberia_application::ErrorKind::ProjectAlreadyExists
    );
    assert_eq!(current(&created), before);
}

#[test]
fn missing_project_is_rejected_at_open() {
    let path = retained_directory().join("missing.rfatlas");
    let error = Application
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::MissingProject);
}

#[test]
fn corrupt_canonical_artifact_is_rejected_by_the_application_query() {
    let root = retained_directory();
    let path = root.join("corrupt.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Corruptible")).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let artifact = manifest["artifacts"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .to_owned();
    fs::write(
        path.join("artifacts").join(artifact),
        b"corrupt canonical bytes",
    )
    .unwrap();
    let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn unsupported_schema_is_rejected_even_when_read_only_metadata_is_supported_by_storage() {
    let root = retained_directory();
    let path = root.join("future.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Future")).unwrap();
    drop(session);

    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    manifest["schema_version"] = serde_json::Value::from(2_u64);
    db.execute(
        "UPDATE bundle_manifest SET body=?1",
        [serde_json::to_vec(&manifest).unwrap()],
    )
    .unwrap();
    db.execute_batch("PRAGMA user_version=2").unwrap();

    let error = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(
        error.kind(),
        kyberia_application::ErrorKind::UnsupportedVersion
    );
}

#[test]
fn query_view_is_immutable_and_reads_canonical_publication_after_reopen() {
    use kyberia_causal_materializer::materialize;
    use kyberia_operation_log::{OperationSet, ProjectVersion};
    use kyberia_project_store::MaterializationPublicationOutcome;

    let root = retained_directory();
    let path = root.join("published.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Canonical")).unwrap();
    let before = current(&session);
    let before_revision = before.revision().unwrap();
    let before_project = before.project().unwrap().clone();

    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let baseline = writer.materialization_baseline().unwrap().unwrap();
    let operations = OperationSet::empty(baseline.id());
    let materialized = materialize(&baseline, &operations).unwrap();
    assert!(matches!(
        writer
            .publish_materialized_project(
                &baseline,
                &operations,
                &materialized,
                ProjectVersion::new(0),
                2,
            )
            .unwrap(),
        MaterializationPublicationOutcome::Published(_)
    ));
    drop(writer);

    assert_eq!(before.project().unwrap(), &before_project);
    assert_eq!(before.revision().unwrap(), before_revision);
    let after = current(&session);
    assert_eq!(after.state(), ProjectState::MaterializedCurrent);
    assert_eq!(after.project().unwrap(), &before_project);
    assert_eq!(after.revision().unwrap().bundle_revision(), 2);
    assert_eq!(
        after.revision().unwrap().publication_bundle_revision(),
        Some(2)
    );
    assert_eq!(after.revision().unwrap().operation_revision(), Some(0));
    assert_ne!(after, before);

    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let ordinary_bytes = b"ordinary map source metadata";
    writer
        .put_artifact(
            ordinary_bytes,
            ArtifactEntry {
                kind: ArtifactKind::MapSource,
                bytes: ordinary_bytes.len() as u64,
                media_type: "application/octet-stream".into(),
                provenance_id: "application-test-map-source".into(),
            },
            3,
        )
        .unwrap();
    drop(writer);

    let after_artifact = current(&session);
    assert_eq!(after_artifact.revision().unwrap().bundle_revision(), 3);
    assert_eq!(
        after_artifact
            .revision()
            .unwrap()
            .publication_bundle_revision(),
        Some(2)
    );
}

#[test]
fn missing_project_database_inside_an_existing_root_is_corruption() {
    let root = retained_directory();
    let path = root.join("missing-database.rfatlas");
    let app = Application;
    drop(
        app.create(create_request(&path, "Missing database"))
            .unwrap(),
    );
    let retained = retained_directory().join("project.sqlite.moved");
    fs::rename(path.join("project.sqlite"), &retained).unwrap();

    let error = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn missing_artifacts_directory_inside_an_existing_root_is_corruption() {
    let root = retained_directory();
    let path = root.join("missing-artifacts-directory.rfatlas");
    let app = Application;
    drop(
        app.create(create_request(&path, "Missing artifacts directory"))
            .unwrap(),
    );
    let retained = retained_directory().join("artifacts.moved");
    fs::rename(path.join("artifacts"), retained).unwrap();

    let error = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn missing_declared_artifact_is_corruption_at_query_time() {
    let root = retained_directory();
    let path = root.join("missing-artifact.rfatlas");
    let app = Application;
    let session = app
        .create(create_request(&path, "Missing artifact"))
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let artifact = manifest["artifacts"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .to_owned();
    let retained = retained_directory().join("declared-artifact.moved");
    fs::rename(path.join("artifacts").join(artifact), retained).unwrap();

    let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn malformed_manifest_after_open_is_corruption_at_query_time() {
    let root = retained_directory();
    let path = root.join("malformed-manifest.rfatlas");
    let app = Application;
    let session = app
        .create(create_request(&path, "Malformed manifest"))
        .unwrap();
    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE bundle_manifest SET body=?1",
        [b"not-json".as_slice()],
    )
    .unwrap();

    let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::CorruptProject);
}

#[test]
fn caller_budget_is_cumulative_and_reports_resource_limit() {
    let root = retained_directory();
    let path = root.join("budget.rfatlas");
    let session = Application.create(create_request(&path, "Budget")).unwrap();
    let mut budget = ResourceBudget::new(ResourceLimits::new(0, 0, 0, 0, 0, 0));

    let error = session
        .query_with_budget(ProjectQuery::CurrentSnapshot, &mut budget)
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::ResourceLimit);
    assert!(budget.usage().working_set_bytes() == 0);
}

#[test]
fn query_budget_is_shared_across_materialization_publication_history() {
    use kyberia_causal_materializer::materialize;
    use kyberia_domain::identity::{ActorDeviceId, ActorId, OperationId};
    use kyberia_operation_log::{
        CausalDepth, LogicalTimestamp, Mutation, Operation, OperationSet, ProjectVersion,
    };
    use kyberia_project_store::MaterializationPublicationOutcome;

    let root = retained_directory();
    let path = root.join("budget-history.rfatlas");
    let app = Application;
    let session = app.create(create_request(&path, "Budget history")).unwrap();
    let id = current(&session).project_id();
    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let baseline = writer.materialization_baseline().unwrap().unwrap();
    let empty = OperationSet::empty(id);
    let initial = materialize(&baseline, &empty).unwrap();
    assert!(matches!(
        writer
            .publish_materialized_project(&baseline, &empty, &initial, ProjectVersion::new(0), 2)
            .unwrap(),
        MaterializationPublicationOutcome::Published(_)
    ));

    let first = Operation::try_apply(
        OperationId::from_bytes([1; 16]).unwrap(),
        id,
        ActorId::from_bytes([21; 16]).unwrap(),
        ActorDeviceId::from_bytes([41; 16]).unwrap(),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::set_project_name(Text::new("Budget one").unwrap()),
        Mutation::set_project_name(Text::new("Budget history").unwrap()),
    )
    .unwrap();
    writer
        .append_operation_if_revision(first.clone(), Some(ProjectVersion::new(0)))
        .unwrap();
    let first_set = OperationSet::from_operations([first.clone()]).unwrap();
    let first_project = materialize(&baseline, &first_set).unwrap();
    writer
        .publish_materialized_project(
            &baseline,
            &first_set,
            &first_project,
            ProjectVersion::new(1),
            3,
        )
        .unwrap();

    let second = Operation::try_apply(
        OperationId::from_bytes([2; 16]).unwrap(),
        id,
        ActorId::from_bytes([22; 16]).unwrap(),
        ActorDeviceId::from_bytes([42; 16]).unwrap(),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id()],
        Mutation::set_project_name(Text::new("Budget two").unwrap()),
        Mutation::set_project_name(Text::new("Budget one").unwrap()),
    )
    .unwrap();
    writer
        .append_operation_if_revision(second.clone(), Some(ProjectVersion::new(1)))
        .unwrap();
    let second_set = OperationSet::from_operations([first, second]).unwrap();
    let second_project = materialize(&baseline, &second_set).unwrap();
    writer
        .publish_materialized_project(
            &baseline,
            &second_set,
            &second_project,
            ProjectVersion::new(2),
            4,
        )
        .unwrap();
    drop(writer);

    let mut unrestricted = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    ));
    session
        .query_with_budget(ProjectQuery::CurrentSnapshot, &mut unrestricted)
        .unwrap();
    let total_copy_bytes = unrestricted.usage().project_copy_bytes();
    assert!(total_copy_bytes > 1);

    let mut cumulative = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        total_copy_bytes - 1,
        usize::MAX,
    ));
    let error = session
        .query_with_budget(ProjectQuery::CurrentSnapshot, &mut cumulative)
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::ResourceLimit);
    assert!(cumulative.usage().project_copy_bytes() > 0);
    assert!(cumulative.usage().project_copy_bytes() < total_copy_bytes);
}

#[test]
fn already_open_session_revalidates_schema_and_required_features() {
    for (label, schema_version, required_features, database_version) in [
        ("logical-schema", 2_u64, serde_json::json!([]), 2_u64),
        (
            "logical-feature",
            1_u64,
            serde_json::json!(["future-materialization"]),
            1_u64,
        ),
    ] {
        let root = retained_directory();
        let path = root.join(format!("{label}.rfatlas"));
        let app = Application;
        let session = app.create(create_request(&path, label)).unwrap();
        let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
        manifest["schema_version"] = serde_json::Value::from(schema_version);
        manifest["required_features"] = required_features;
        db.execute(
            "UPDATE bundle_manifest SET body=?1",
            [serde_json::to_vec(&manifest).unwrap()],
        )
        .unwrap();
        db.execute_batch(&format!("PRAGMA user_version={database_version}"))
            .unwrap();

        let error = session.query(ProjectQuery::CurrentSnapshot).unwrap_err();
        assert_eq!(
            error.kind(),
            kyberia_application::ErrorKind::UnsupportedVersion
        );
    }
}

struct CancelImmediately;

impl CancellationHook for CancelImmediately {
    fn is_cancelled(&mut self) -> bool {
        true
    }
}

#[test]
fn cancellation_prevents_returning_a_query_result() {
    let root = retained_directory();
    let path = root.join("cancelled.rfatlas");
    let session = Application
        .create(create_request(&path, "Cancelled"))
        .unwrap();
    let mut cancellation = CancelImmediately;
    let error = session
        .query_with_cancel(ProjectQuery::CurrentSnapshot, &mut cancellation)
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::Cancelled);
}

#[test]
fn legacy_bundle_without_a_baseline_is_explicitly_absent() {
    let root = retained_directory();
    let path = root.join("legacy.rfatlas");
    let id = kyberia_domain::identity::ProjectId::from_bytes([7; 16]).unwrap();
    drop(Bundle::create(&path, id, "Legacy".into(), 1).unwrap());
    let db = rusqlite::Connection::open(path.join("project.sqlite")).unwrap();
    db.execute_batch(
        "DROP TABLE materialized_project_state;
         DROP TABLE materialized_project_publications;
         DROP TABLE materialization_baselines;",
    )
    .unwrap();
    let session = Application
        .open(OpenProject {
            path,
            mode: SessionMode::ReadOnly,
        })
        .unwrap();
    let view = current(&session);
    assert_eq!(view.state(), ProjectState::LegacyAbsent);
    assert!(view.project().is_none());
    assert!(view.revision().is_none());
}

fn assert_application_error_is_structured(error: ApplicationError) {
    assert!(!error.message().is_empty());
}

#[test]
fn invalid_create_timestamp_is_rejected_before_reserving_a_directory() {
    let root = retained_directory();
    let path = root.join("invalid.rfatlas");
    let error = Application
        .create(CreateProject {
            path: path.clone(),
            name: Text::new("Invalid").unwrap(),
            created_utc_ms: -1,
        })
        .unwrap_err();
    assert_eq!(error.kind(), kyberia_application::ErrorKind::InvalidRequest);
    assert_application_error_is_structured(error);
    assert!(!path.exists());
}

#[test]
fn intent_workflow_creates_floor_derives_causality_and_retries_after_reopen() {
    let root = retained_directory();
    let path = root.join("intent-map.rfatlas");
    let hierarchy = initial_hierarchy();
    let floor_id = hierarchy.floor.data().id;
    let floor_frame = hierarchy.floor.data().frame.clone();
    let image_frame = frame(45, FrameKind::ImagePixels);
    let app = Application;
    let mut session = app
        .create_with_initial_floor(CreateProjectWithInitialFloor {
            path: path.clone(),
            name: Text::new("Intent map").unwrap(),
            created_utc_ms: 1,
            hierarchy,
        })
        .unwrap();
    let created = current(&session);
    let project_id = created.project_id();
    assert_eq!(created.project().unwrap().floors().count(), 1);
    assert_eq!(created.revision().unwrap().project_revision(), 0);

    let import_operation: OperationId = identity(50);
    let map_id: MapAssetId = identity(51);
    let import = ImportMapIntent {
        authority: MapIntentAuthority {
            operation_id: import_operation,
            actor_id: identity(52),
            device_id: identity(53),
            committed_utc_ms: 2,
        },
        map_id,
        floor_id,
        name: Text::new("Ground plan").unwrap(),
        image_frame: image_frame.clone(),
        provenance: Text::new("native-picker:grant-123").unwrap(),
    };
    let bytes = png(200, 120);
    let mut budget = ResourceBudget::new(limits());
    let imported = session
        .import_map_intent_with_budget(import.clone(), &bytes, &mut budget)
        .unwrap();
    assert!(imported.current.is_ok());
    assert!(
        imported
            .current
            .as_ref()
            .unwrap()
            .project()
            .unwrap()
            .map(map_id)
            .is_some()
    );
    assert_eq!(imported.receipt.project_revision.value(), 1);
    drop(session);

    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let operations = bundle.operation_set().unwrap();
    let imported_operation = operations.operation(import_operation).unwrap();
    assert_eq!(imported_operation.logical_time().value(), 1);
    assert_eq!(imported_operation.causal_depth().value(), 0);
    assert!(imported_operation.parents().is_empty());
    drop(bundle);

    let mut session = app
        .open(OpenProject {
            path: path.clone(),
            mode: SessionMode::ReadWrite,
        })
        .unwrap();
    let invalid_operation: OperationId = identity(54);
    let mut out_of_bounds = calibration(55, map_id, &image_frame, &floor_frame, 10.0);
    let mut controls = out_of_bounds.transform.controls().clone();
    controls.image_second.x = Pixels::new(200.0).unwrap();
    out_of_bounds.transform = TwoPointCalibration::new(controls).unwrap();
    let invalid = CalibrateMapIntent {
        authority: MapIntentAuthority {
            operation_id: invalid_operation,
            actor_id: identity(52),
            device_id: identity(53),
            committed_utc_ms: 3,
        },
        calibration: out_of_bounds,
    };
    let mut invalid_budget = ResourceBudget::new(limits());
    assert_eq!(
        session
            .calibrate_map_intent_with_budget(invalid, &mut invalid_budget)
            .unwrap_err()
            .kind(),
        kyberia_application::ErrorKind::InvalidRequest
    );
    assert_eq!(current(&session).revision().unwrap().project_revision(), 1);

    let calibration_operation: OperationId = identity(56);
    let calibration_id: CalibrationId = identity(57);
    let calibration_intent = CalibrateMapIntent {
        authority: MapIntentAuthority {
            operation_id: calibration_operation,
            actor_id: identity(52),
            device_id: identity(53),
            committed_utc_ms: 4,
        },
        calibration: calibration(57, map_id, &image_frame, &floor_frame, 10.0),
    };
    let mut calibration_budget = ResourceBudget::new(limits());
    let calibrated = session
        .calibrate_map_intent_with_budget(calibration_intent.clone(), &mut calibration_budget)
        .unwrap();
    assert!(calibrated.current.is_ok());
    assert_eq!(calibrated.receipt.project_revision.value(), 2);
    assert_eq!(
        calibrated
            .current
            .as_ref()
            .unwrap()
            .project()
            .unwrap()
            .active_calibration(map_id),
        Some(Evidence::Known(calibration_id))
    );
    drop(session);

    let bundle = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let operations = bundle.operation_set().unwrap();
    let calibration_record = operations.operation(calibration_operation).unwrap();
    assert_eq!(calibration_record.logical_time().value(), 2);
    assert_eq!(calibration_record.causal_depth().value(), 1);
    assert_eq!(calibration_record.parents(), &[import_operation]);
    drop(bundle);

    // Model a future multi-device DAG with nine concurrent heads. Exact
    // retry of the already committed import must not need to derive a new
    // eight-parent frontier.
    let mut writer = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    let mut expected_revision = writer.operation_store_state().unwrap().project_revision();
    let mut concurrent_map_budget = ResourceBudget::new(limits());
    let admitted =
        kyberia_application::admit_map_asset(&bytes, &mut concurrent_map_budget).unwrap();
    for index in 0..8_u8 {
        let map_id: MapAssetId = identity(100 + index);
        let map = MapAsset::new(MapAssetData {
            id: map_id,
            floor_id,
            name: Text::new(format!("Concurrent map {index}")).unwrap(),
            image_frame: frame(130 + index, FrameKind::ImagePixels),
            width: admitted.width(),
            height: admitted.height(),
            source: ArtifactReference {
                sha256: imported.receipt.content_hash,
                media_type: Text::new("image/png").unwrap(),
                byte_length: bytes.len() as u64,
            },
            provenance: Text::new("native-picker:grant-123").unwrap(),
        })
        .unwrap();
        let operation = Operation::try_apply_v3(
            identity(60 + index),
            project_id,
            identity(70 + index),
            identity(80 + index),
            LogicalTimestamp::new(3).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::import_map(map),
            InversePrior::MapAbsent { map_id },
        )
        .unwrap();
        writer
            .append_operation_if_revision(operation, Some(expected_revision))
            .unwrap();
        expected_revision = writer.operation_store_state().unwrap().project_revision();
    }
    let operations = writer.operation_set().unwrap();
    let head_count = operations
        .operations()
        .filter(|operation| {
            !operations
                .operations()
                .any(|child| child.parents().contains(&operation.operation_id()))
        })
        .count();
    assert_eq!(head_count, 9);
    let baseline = writer.materialization_baseline().unwrap().unwrap();
    let materialized = kyberia_causal_materializer::materialize(&baseline, &operations).unwrap();
    writer
        .publish_materialized_project(&baseline, &operations, &materialized, expected_revision, 5)
        .unwrap();
    drop(writer);

    let mut reopened = app
        .open(OpenProject {
            path,
            mode: SessionMode::ReadWrite,
        })
        .unwrap();
    let mut retry_import_budget = ResourceBudget::new(limits());
    let retry_import = reopened
        .import_map_intent_with_budget(import, &bytes, &mut retry_import_budget)
        .unwrap();
    assert_eq!(retry_import.receipt.operation_id, import_operation);
    assert_eq!(
        retry_import.receipt.content_hash,
        imported.receipt.content_hash
    );
    assert!(retry_import.current.is_ok());
    let mut retry_calibration_budget = ResourceBudget::new(limits());
    let retry_calibration = reopened
        .calibrate_map_intent_with_budget(calibration_intent, &mut retry_calibration_budget)
        .unwrap();
    assert_eq!(
        retry_calibration.receipt.operation_id,
        calibration_operation
    );
    assert_eq!(
        retry_calibration.receipt.content_hash,
        calibrated.receipt.content_hash
    );
    assert!(retry_calibration.current.is_ok());
}
