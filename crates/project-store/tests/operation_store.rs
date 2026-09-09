use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{
        ActorDeviceId, ActorId, CalibrationId, ContentHash, MapAssetId, OperationId, ProjectId,
        SiteId, Text,
    },
};
use kyberia_operation_log::{
    CausalDepth, InversePrior, LogicalTimestamp, Mutation, Operation, OperationReference,
    OperationSet, ProjectVersion, ResolutionValue,
};
use kyberia_project_store::{AppliedEffect, Bundle, OpenMode, OperationAppendOutcome, StoreError};
use rusqlite::Connection;
use std::{fs, path::PathBuf};

fn project() -> ProjectId {
    ProjectId::from_bytes([1; 16]).unwrap()
}

fn operation_id(value: u8) -> OperationId {
    OperationId::from_bytes([value; 16]).unwrap()
}

fn actor(value: u8) -> ActorId {
    ActorId::from_bytes([value; 16]).unwrap()
}

fn device(value: u8) -> ActorDeviceId {
    ActorDeviceId::from_bytes([value; 16]).unwrap()
}

fn text(value: &str) -> Text {
    Text::new(value).unwrap()
}

fn apply(
    id: u8,
    actor_value: u8,
    logical_time: u64,
    causal_depth: u64,
    parents: Vec<OperationId>,
    value: &str,
    previous: &str,
) -> Operation {
    Operation::try_apply(
        operation_id(id),
        project(),
        actor(actor_value),
        device(actor_value),
        LogicalTimestamp::new(logical_time).unwrap(),
        CausalDepth::new(causal_depth),
        parents,
        Mutation::set_project_name(text(value)),
        Mutation::set_project_name(text(previous)),
    )
    .unwrap()
}

fn site_apply(
    id: u8,
    actor_value: u8,
    logical_time: u64,
    causal_depth: u64,
    parents: Vec<OperationId>,
    value: &str,
    previous: &str,
) -> Operation {
    let site_id = SiteId::from_bytes([7; 16]).unwrap();
    Operation::try_apply(
        operation_id(id),
        project(),
        actor(actor_value),
        device(actor_value),
        LogicalTimestamp::new(logical_time).unwrap(),
        CausalDepth::new(causal_depth),
        parents,
        Mutation::set_site_name(site_id, text(value)),
        Mutation::set_site_name(site_id, text(previous)),
    )
    .unwrap()
}

fn undo(
    id: u8,
    actor_value: u8,
    logical_time: u64,
    causal_depth: u64,
    parents: Vec<OperationId>,
    target: &Operation,
) -> Operation {
    Operation::try_undo(
        operation_id(id),
        project(),
        actor(actor_value),
        device(actor_value),
        LogicalTimestamp::new(logical_time).unwrap(),
        CausalDepth::new(causal_depth),
        parents,
        OperationReference::from(target),
    )
    .unwrap()
}

fn redo(
    id: u8,
    actor_value: u8,
    logical_time: u64,
    causal_depth: u64,
    parents: Vec<OperationId>,
    target: &Operation,
) -> Operation {
    Operation::try_redo(
        operation_id(id),
        project(),
        actor(actor_value),
        device(actor_value),
        LogicalTimestamp::new(logical_time).unwrap(),
        CausalDepth::new(causal_depth),
        parents,
        OperationReference::from(target),
    )
    .unwrap()
}

fn bundle(path: &std::path::Path) -> Bundle {
    Bundle::create(path, project(), "Operation store".into(), 10).unwrap()
}

fn retained_test_dir(prefix: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(root)
        .unwrap()
        .keep()
}

#[test]
fn append_retry_reopen_and_replay_preserve_canonical_operation() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    assert_eq!(
        bundle.append_operation(root.clone()).unwrap(),
        OperationAppendOutcome::Appended {
            project_revision: ProjectVersion::new(1),
            bundle_revision: 1,
        }
    );
    // The first response may be lost after SQLite commit. Retrying with the
    // caller's stale pre-commit revision must still return Duplicate.
    assert_eq!(
        bundle
            .append_operation_if_revision(root.clone(), Some(ProjectVersion::new(0)))
            .unwrap(),
        OperationAppendOutcome::Duplicate {
            project_revision: ProjectVersion::new(1),
            bundle_revision: 1,
        }
    );
    assert_eq!(
        bundle.append_operation(root.clone()).unwrap(),
        OperationAppendOutcome::Duplicate {
            project_revision: ProjectVersion::new(1),
            bundle_revision: 1,
        }
    );
    let forged = apply(2, 1, 1, 0, vec![], "forged", "initial");
    assert!(matches!(
        bundle.append_operation(forged),
        Err(StoreError::Operation(message))
            if message.contains("different immutable bytes")
    ));
    assert_eq!(
        bundle.operation_store_state().unwrap().project_revision(),
        ProjectVersion::new(1)
    );
    let child = apply(3, 1, 2, 1, vec![root.operation_id()], "child", "root");
    bundle.append_operation(child.clone()).unwrap();
    let state = bundle.operation_store_state().unwrap();
    assert_eq!(state.project_id(), project());
    assert_eq!(state.project_revision(), ProjectVersion::new(2));
    assert_eq!(state.operation_count(), 2);
    assert_eq!(bundle.manifest().unwrap().revision, 2);
    drop(bundle);

    let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let set = reopened.operation_set().unwrap();
    let ordered = set.ordered().unwrap();
    assert_eq!(
        ordered
            .iter()
            .map(|operation| operation.operation_id())
            .collect::<Vec<_>>(),
        vec![root.operation_id(), child.operation_id()]
    );
    assert_eq!(reopened.replay_operations().unwrap().len(), 2);
    assert!(
        reopened
            .replay_operation_effects()
            .unwrap()
            .iter()
            .all(|effect| matches!(effect, AppliedEffect::Mutation(_)))
    );
    assert_eq!(
        reopened.operation_store_state().unwrap().operation_count(),
        2
    );
}

#[test]
fn typed_replay_persists_unknown_calibration_undo_and_reopens() {
    let dir = retained_test_dir("typed-unknown-undo-");
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
    let applied_calibration = CalibrationId::from_bytes([8; 16]).unwrap();
    let root = Operation::try_apply_v2(
        operation_id(2),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::activate_calibration(map_id, applied_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
        },
    )
    .unwrap();
    let undo = Operation::try_undo_v2(
        operation_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![root.operation_id()],
        OperationReference::from(&root),
    )
    .unwrap();
    bundle.append_operation(root).unwrap();
    bundle.append_operation(undo).unwrap();

    let effects = bundle.replay_operation_effects().unwrap();
    assert!(matches!(
        effects.last(),
        Some(AppliedEffect::Calibration {
            map_id: effect_map,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
            ..
        }) if *effect_map == map_id
    ));
    assert!(matches!(
        bundle.replay_operations(),
        Err(StoreError::Operation(message)) if message.contains("TypedPriorRequired")
    ));

    drop(bundle);
    let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(reopened.replay_operation_effects().unwrap(), effects);
}

#[test]
fn typed_replay_persists_and_reopens_resolved_unknown_calibration_conflict() {
    let dir = retained_test_dir("typed-unknown-resolution-");
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
    let root_calibration = CalibrationId::from_bytes([8; 16]).unwrap();
    let right_calibration = CalibrationId::from_bytes([9; 16]).unwrap();
    let root = Operation::try_apply_v2(
        operation_id(2),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::activate_calibration(map_id, root_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
        },
    )
    .unwrap();
    let left = Operation::try_undo_v2(
        operation_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![root.operation_id()],
        OperationReference::from(&root),
    )
    .unwrap();
    let right = Operation::try_apply_v2(
        operation_id(4),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![root.operation_id()],
        Mutation::activate_calibration(map_id, right_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
        },
    )
    .unwrap();
    let resolution = Operation::try_resolve_v2(
        operation_id(5),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id(), right.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&right),
        ResolutionValue::activate_calibration(
            map_id,
            Evidence::Unknown(UnknownReason::NotMeasured),
        ),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
        },
    )
    .unwrap();

    bundle.append_operation(root).unwrap();
    bundle.append_operation(left).unwrap();
    bundle.append_operation(right).unwrap();
    assert!(matches!(
        bundle.replay_operation_effects(),
        Err(StoreError::Operation(message)) if message.contains("Conflicts")
    ));
    bundle.append_operation(resolution).unwrap();

    let effects = bundle.replay_operation_effects().unwrap();
    assert!(matches!(
        effects.last(),
        Some(AppliedEffect::Calibration {
            map_id: effect_map,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
            ..
        }) if *effect_map == map_id
    ));
    drop(bundle);
    let reopened = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert_eq!(reopened.replay_operation_effects().unwrap(), effects);
}

#[test]
fn project_revision_is_independent_from_bundle_revision() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    bundle.append_operation(root).unwrap();
    let entry = kyberia_project_store::ArtifactEntry {
        kind: kyberia_project_store::ArtifactKind::MapSource,
        bytes: 3,
        media_type: "text/plain".into(),
        provenance_id: "operation-store-test".into(),
    };
    bundle.put_artifact(b"map", entry, 10).unwrap();
    let child = apply(3, 1, 2, 1, vec![operation_id(2)], "child", "root");
    bundle.append_operation(child).unwrap();
    assert_eq!(
        bundle.operation_store_state().unwrap().project_revision(),
        ProjectVersion::new(2)
    );
    assert_eq!(bundle.manifest().unwrap().revision, 3);
}

#[test]
fn branches_join_with_explicit_conflict_and_replay_refuses_silent_resolution() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let left = apply(3, 1, 2, 1, vec![root.operation_id()], "left", "root");
    let right = apply(4, 2, 2, 1, vec![root.operation_id()], "right", "root");
    bundle.append_operation(root.clone()).unwrap();
    bundle.append_operation(left.clone()).unwrap();
    bundle.append_operation(right.clone()).unwrap();
    let outcome = bundle.operation_set().unwrap();
    assert_eq!(outcome.operations().count(), 3);
    assert!(OperationSet::from_operations(outcome.operations().cloned()).is_ok());
    assert!(matches!(
        bundle.replay_operations(),
        Err(StoreError::Operation(message)) if message.contains("Conflicts")
    ));
}

#[test]
fn wrong_project_duplicate_and_invalid_parent_are_rejected_without_mutation() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    bundle.append_operation(root.clone()).unwrap();
    let before = bundle.operation_store_state().unwrap();
    let mut other = apply(3, 1, 2, 1, vec![root.operation_id()], "other", "root");
    let other_project = ProjectId::from_bytes([9; 16]).unwrap();
    other = Operation::try_apply(
        other.operation_id(),
        other_project,
        other.actor_id(),
        other.device_id(),
        other.logical_time(),
        other.causal_depth(),
        other.parents().to_vec(),
        Mutation::set_project_name(text("other")),
        Mutation::set_project_name(text("root")),
    )
    .unwrap();
    assert!(matches!(
        bundle.append_operation(other),
        Err(StoreError::Operation(message)) if message.contains("another project")
    ));
    let missing_parent = apply(5, 1, 2, 1, vec![operation_id(9)], "missing", "root");
    assert!(bundle.append_operation(missing_parent).is_err());
    assert_eq!(bundle.operation_store_state().unwrap(), before);
}

#[test]
fn stale_revision_is_a_cancellation_point_before_commit() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    assert!(matches!(
        bundle.append_operation_if_revision(root, Some(ProjectVersion::new(7))),
        Err(StoreError::Invalid(message)) if message.contains("stale operation project revision")
    ));
    assert_eq!(bundle.operation_store_state().unwrap().operation_count(), 0);
    assert_eq!(bundle.manifest().unwrap().revision, 0);
}

#[test]
fn sequential_repeated_undo_is_rejected_before_sql_write() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let first_undo = undo(3, 1, 2, 1, vec![root.operation_id()], &root);
    let repeated_undo = undo(4, 1, 3, 2, vec![first_undo.operation_id()], &root);
    bundle.append_operation(root).unwrap();
    bundle.append_operation(first_undo).unwrap();
    let before_state = bundle.operation_store_state().unwrap();
    let before_manifest = bundle.manifest().unwrap();
    assert!(matches!(
        bundle.append_operation(repeated_undo),
        Err(StoreError::Operation(message)) if message.contains("operation replay is invalid")
    ));
    assert_eq!(bundle.operation_store_state().unwrap(), before_state);
    assert_eq!(bundle.manifest().unwrap(), before_manifest);
    assert_eq!(bundle.operation_set().unwrap().operations().count(), 2);
    assert_eq!(bundle.replay_operations().unwrap().len(), 2);
}

#[test]
fn sequential_repeated_redo_is_rejected_before_sql_write() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let first_undo = undo(3, 1, 2, 1, vec![root.operation_id()], &root);
    let first_redo = redo(4, 1, 3, 2, vec![first_undo.operation_id()], &root);
    let repeated_redo = redo(5, 1, 4, 3, vec![first_redo.operation_id()], &root);
    bundle.append_operation(root).unwrap();
    bundle.append_operation(first_undo).unwrap();
    bundle.append_operation(first_redo).unwrap();
    let before_state = bundle.operation_store_state().unwrap();
    let before_manifest = bundle.manifest().unwrap();
    assert!(matches!(
        bundle.append_operation(repeated_redo),
        Err(StoreError::Operation(message)) if message.contains("operation replay is invalid")
    ));
    assert_eq!(bundle.operation_store_state().unwrap(), before_state);
    assert_eq!(bundle.manifest().unwrap(), before_manifest);
    assert_eq!(bundle.operation_set().unwrap().operations().count(), 3);
    assert_eq!(bundle.replay_operations().unwrap().len(), 3);
}

#[test]
fn conflict_does_not_mask_repeated_undo_before_sql_write() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let left = site_apply(3, 1, 2, 1, vec![root.operation_id()], "left", "initial");
    let right = site_apply(4, 2, 2, 1, vec![root.operation_id()], "right", "initial");
    let first_undo = undo(5, 1, 3, 1, vec![root.operation_id()], &root);
    let repeated_undo = undo(6, 1, 4, 2, vec![first_undo.operation_id()], &root);
    bundle.append_operation(root).unwrap();
    bundle.append_operation(left).unwrap();
    bundle.append_operation(right).unwrap();
    bundle.append_operation(first_undo).unwrap();
    let before_state = bundle.operation_store_state().unwrap();
    let before_manifest = bundle.manifest().unwrap();
    assert!(matches!(
        bundle.append_operation(repeated_undo),
        Err(StoreError::Operation(message)) if message.contains("operation replay is invalid")
    ));
    assert_eq!(bundle.operation_store_state().unwrap(), before_state);
    assert_eq!(bundle.manifest().unwrap(), before_manifest);
    assert_eq!(bundle.operation_set().unwrap().operations().count(), 4);
}

#[test]
fn conflict_does_not_mask_repeated_redo_before_sql_write() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let left = site_apply(3, 1, 2, 1, vec![root.operation_id()], "left", "initial");
    let right = site_apply(4, 2, 2, 1, vec![root.operation_id()], "right", "initial");
    let first_undo = undo(5, 1, 3, 1, vec![root.operation_id()], &root);
    let first_redo = redo(6, 1, 4, 2, vec![first_undo.operation_id()], &root);
    let repeated_redo = redo(7, 1, 5, 3, vec![first_redo.operation_id()], &root);
    bundle.append_operation(root).unwrap();
    bundle.append_operation(left).unwrap();
    bundle.append_operation(right).unwrap();
    bundle.append_operation(first_undo).unwrap();
    bundle.append_operation(first_redo).unwrap();
    let before_state = bundle.operation_store_state().unwrap();
    let before_manifest = bundle.manifest().unwrap();
    assert!(matches!(
        bundle.append_operation(repeated_redo),
        Err(StoreError::Operation(message)) if message.contains("operation replay is invalid")
    ));
    assert_eq!(bundle.operation_store_state().unwrap(), before_state);
    assert_eq!(bundle.manifest().unwrap(), before_manifest);
    assert_eq!(bundle.operation_set().unwrap().operations().count(), 5);
}

#[test]
fn projection_failure_rolls_back_operation_state_and_manifest() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let before = bundle.manifest().unwrap();
    fs::rename(path.join("manifest.json"), path.join("manifest.saved")).unwrap();
    fs::create_dir(path.join("manifest.json")).unwrap();
    assert!(
        bundle
            .append_operation(apply(2, 1, 1, 0, vec![], "root", "initial"))
            .is_err()
    );
    assert_eq!(bundle.manifest().unwrap(), before);
    assert_eq!(bundle.operation_store_state().unwrap().operation_count(), 0);
    assert!(
        bundle
            .operation_set()
            .unwrap()
            .operations()
            .next()
            .is_none()
    );
}

#[test]
fn read_only_append_is_rejected_and_additive_migration_adds_empty_operation_store() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let bundle = bundle(&path);
    drop(bundle);
    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute_batch("DROP TABLE project_operations; DROP TABLE operation_log_state;")
        .unwrap();
    drop(database);
    let reader = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        reader.operation_set(),
        Err(StoreError::UnsupportedVersion(1))
    ));
    drop(reader);
    let mut migrated = Bundle::open(&path, OpenMode::ReadWrite).unwrap();
    assert_eq!(
        migrated.operation_store_state().unwrap().operation_count(),
        0
    );
    assert!(matches!(
        migrated.append_operation(apply(2, 1, 1, 0, vec![], "root", "initial")),
        Ok(OperationAppendOutcome::Appended { .. })
    ));
    drop(migrated);
    let reader = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let mut reader = reader;
    assert!(matches!(
        reader.append_operation(apply(3, 1, 2, 1, vec![operation_id(2)], "child", "root")),
        Err(StoreError::ReadOnly)
    ));
}

#[test]
fn corrupt_hash_bytes_and_future_wire_version_fail_closed() {
    for mutation in [
        "UPDATE project_operations SET content_hash='ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'",
        "UPDATE project_operations SET canonical_bytes=x'00'",
        "UPDATE project_operations SET wire_bytes=json_set(CAST(wire_bytes AS TEXT),'$.schema_version','2')",
    ] {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("project");
        let mut bundle = bundle(&path);
        bundle
            .append_operation(apply(2, 1, 1, 0, vec![], "root", "initial"))
            .unwrap();
        drop(bundle);
        let database = Connection::open(path.join("project.sqlite")).unwrap();
        database.execute(mutation, []).unwrap();
        drop(database);
        let reader = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(matches!(
            reader.operation_set(),
            Err(StoreError::Corrupt(_)) | Err(StoreError::Sql(_))
        ));
    }
}

#[test]
fn operation_row_resource_limit_is_checked_before_replay() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    bundle
        .append_operation(apply(2, 1, 1, 0, vec![], "root", "initial"))
        .unwrap();
    drop(bundle);
    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    database
        .execute(
            "UPDATE project_operations SET wire_bytes=?1",
            [vec![
                0_u8;
                kyberia_operation_log::MAX_OPERATION_WIRE_BYTES + 1
            ]],
        )
        .unwrap();
    drop(database);
    let reader = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        reader.operation_set(),
        Err(StoreError::Corrupt(message)) if message.contains("wire bytes exceed read budget")
    ));
}

#[test]
fn four_megabyte_blob_is_rejected_by_length_preflight() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    bundle
        .append_operation(apply(2, 1, 1, 0, vec![], "root", "initial"))
        .unwrap();
    drop(bundle);
    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    database
        .execute(
            "UPDATE project_operations SET wire_bytes=?1",
            [vec![0_u8; 4 * 1024 * 1024]],
        )
        .unwrap();
    drop(database);
    let reader = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        reader.operation_set(),
        Err(StoreError::Corrupt(message)) if message.contains("wire bytes exceed read budget")
    ));
}

#[test]
fn deterministic_replay_uses_operation_set_and_preserves_target_hash() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let root_hash = root.content_hash();
    let child = apply(3, 1, 2, 1, vec![root.operation_id()], "child", "root");
    bundle.append_operation(child.clone()).unwrap_err();
    bundle.append_operation(root.clone()).unwrap();
    bundle.append_operation(child).unwrap();
    let set = bundle.operation_set().unwrap();
    assert_eq!(
        set.operation(root.operation_id()).unwrap().content_hash(),
        root_hash
    );
    assert_eq!(bundle.replay_operations().unwrap().len(), 2);
}

#[test]
fn operation_state_corruption_is_reported_by_verify() {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("project");
    let mut bundle = bundle(&path);
    bundle
        .append_operation(apply(2, 1, 1, 0, vec![], "root", "initial"))
        .unwrap();
    drop(bundle);
    let database = Connection::open(path.join("project.sqlite")).unwrap();
    database
        .execute("UPDATE operation_log_state SET project_revision=0", [])
        .unwrap();
    drop(database);
    let reader = Bundle::open(&path, OpenMode::ReadOnly).unwrap();
    let verification = reader.verify().unwrap();
    assert!(
        verification
            .failures
            .iter()
            .any(|failure| failure.contains("operation row count"))
    );
}

#[test]
fn content_hash_column_is_the_canonical_digest() {
    let operation = apply(2, 1, 1, 0, vec![], "root", "initial");
    let expected = kyberia_project_store::content_hash(operation.canonical_bytes());
    assert_eq!(String::from(operation.content_hash()), expected);
    assert_ne!(operation.content_hash(), ContentHash::from_sha256([0; 32]));
}
