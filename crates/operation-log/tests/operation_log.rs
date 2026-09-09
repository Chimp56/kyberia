use kyberia_domain::{
    evidence::{Evidence, UnknownReason},
    identity::{
        ActorDeviceId, ActorId, CalibrationId, ContentHash, FloorId, MapAssetId, OperationId,
        ProjectId, SiteId, Text,
    },
};
use kyberia_operation_log::{
    AppendError, AppendOutcome, AppliedEffect, CausalDepth, ConflictIntent, FieldKey,
    ImmutableReference, InverseMetadata, InversePrior, LogicalTimestamp, MAX_OPERATION_COUNT,
    MAX_OPERATION_WIRE_BYTES, MergeConflict, MergeError, Mutation, NonReversibleReason, Operation,
    OperationError, OperationLog, OperationPayload, OperationReference, OperationSchemaVersion,
    OperationSet, ProjectVersion, ResolutionValue, ToggleDirection,
};
use kyberia_resource_budget::{BudgetKind, CancellationHook, ResourceBudget, ResourceLimits};
use proptest::prelude::*;
use sha2::Digest;

fn project() -> ProjectId {
    ProjectId::from_bytes([1; 16]).unwrap()
}

fn op_id(value: u8) -> OperationId {
    OperationId::from_bytes([value; 16]).unwrap()
}

fn op_id_u64(value: u64) -> OperationId {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&1u64.to_be_bytes());
    bytes[8..].copy_from_slice(&value.to_be_bytes());
    OperationId::from_bytes(bytes).unwrap()
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
    base: u64,
    parents: Vec<OperationId>,
    value: &str,
    previous: &str,
) -> Operation {
    Operation::try_apply(
        op_id(id),
        project(),
        actor(actor_value),
        device(actor_value),
        LogicalTimestamp::new(logical_time).unwrap(),
        CausalDepth::new(base),
        parents,
        Mutation::set_project_name(text(value)),
        Mutation::set_project_name(text(previous)),
    )
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn site_apply(
    id: u8,
    actor_value: u8,
    logical_time: u64,
    base: u64,
    parents: Vec<OperationId>,
    site: u8,
    value: &str,
    previous: &str,
) -> Operation {
    let site_id = SiteId::from_bytes([site; 16]).unwrap();
    Operation::try_apply(
        op_id(id),
        project(),
        actor(actor_value),
        device(actor_value),
        LogicalTimestamp::new(logical_time).unwrap(),
        CausalDepth::new(base),
        parents,
        Mutation::set_site_name(site_id, text(value)),
        Mutation::set_site_name(site_id, text(previous)),
    )
    .unwrap()
}

#[test]
fn inverse_and_redo_are_executable_and_history_remains_immutable() {
    let first = apply(2, 1, 1, 0, vec![], "first", "initial");
    let first_ref = OperationReference::from(&first);
    let mut log = OperationLog::new(project());
    assert_eq!(
        log.append(first.clone()).unwrap(),
        AppendOutcome::Appended {
            revision: ProjectVersion::new(1)
        }
    );

    let second = apply(3, 1, 2, 1, vec![first.operation_id()], "second", "first");
    log.append(second).unwrap();
    let undo = Operation::try_undo(
        op_id(4),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![op_id(3)],
        first_ref,
    )
    .unwrap();
    log.append(undo).unwrap();
    let redo = Operation::try_redo(
        op_id(5),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(4).unwrap(),
        CausalDepth::new(3),
        vec![op_id(4)],
        first_ref,
    )
    .unwrap();
    log.append(redo).unwrap();

    let set = log.as_set().unwrap();
    let replay = set.replay().unwrap();
    assert_eq!(replay.len(), 4);
    assert_eq!(
        set.replay_state().unwrap()[&FieldKey::ProjectName],
        Mutation::set_project_name(text("first"))
    );
    assert_eq!(set.operation(op_id(2)).unwrap(), &first);
    assert_eq!(
        set.operation(op_id(2)).unwrap().content_hash(),
        first_ref.content_hash()
    );
}

#[test]
fn append_is_idempotent_but_same_id_with_new_bytes_is_tampering() {
    let first = apply(2, 1, 1, 0, vec![], "first", "initial");
    let mut log = OperationLog::new(project());
    log.append(first.clone()).unwrap();
    assert_eq!(
        log.append(first.clone()).unwrap(),
        AppendOutcome::Duplicate {
            revision: ProjectVersion::new(1)
        }
    );
    let forged = apply(2, 1, 1, 0, vec![], "forged", "initial");
    assert_eq!(log.append(forged), Err(AppendError::TamperedDuplicate));
}

#[test]
fn concurrent_branch_edits_have_explicit_conflicts_and_are_not_auto_resolved() {
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let left = apply(3, 1, 2, 1, vec![root.operation_id()], "left", "root");
    let right = apply(4, 2, 2, 1, vec![root.operation_id()], "right", "root");
    let left_set = OperationSet::from_operations([root.clone(), left]).unwrap();
    let right_set = OperationSet::from_operations([root, right]).unwrap();

    let outcome = left_set.merge(&right_set).unwrap();
    assert_eq!(outcome.conflicts().len(), 1);
    let conflict = &outcome.conflicts()[0];
    assert_eq!(*conflict.field(), FieldKey::ProjectName);
    assert_eq!(
        conflict.left_mutation(),
        Some(&Mutation::set_project_name(text("left")))
    );
    assert_eq!(
        conflict.right_mutation(),
        Some(&Mutation::set_project_name(text("right")))
    );
    assert!(matches!(
        outcome.into_applyable(),
        Err(MergeError::Conflicts(conflicts)) if conflicts.len() == 1
    ));
}

#[test]
fn causal_edits_order_without_a_conflict() {
    let first = apply(2, 1, 1, 0, vec![], "first", "initial");
    let second = apply(3, 1, 2, 1, vec![first.operation_id()], "second", "first");
    let set = OperationSet::from_operations([second, first]).unwrap();
    let ordered = set.ordered().unwrap();
    assert_eq!(ordered[0].operation_id(), op_id(2));
    assert_eq!(ordered[1].operation_id(), op_id(3));
    assert_eq!(
        set.replay_state().unwrap()[&FieldKey::ProjectName],
        Mutation::set_project_name(text("second"))
    );
}

#[test]
fn malformed_inverse_causality_and_targets_are_rejected() {
    let mismatch = Operation::try_apply(
        op_id(2),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::set_project_name(text("new")),
        Mutation::set_site_name(SiteId::from_bytes([4; 16]).unwrap(), text("old")),
    );
    assert_eq!(mismatch, Err(OperationError::InvalidInverse));

    let unknown_parent = apply(2, 1, 1, 1, vec![op_id(9)], "new", "old");
    assert_eq!(
        OperationSet::from_operations([unknown_parent]),
        Err(OperationError::MissingParent)
    );

    let first = apply(2, 1, 1, 0, vec![], "new", "old");
    let mut log = OperationLog::new(project());
    log.append(first.clone()).unwrap();
    let target = OperationReference::new(first.operation_id(), ContentHash::from_sha256([9; 32]));
    let undo = Operation::try_undo(
        op_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id()],
        target,
    )
    .unwrap();
    assert_eq!(log.append(undo), Err(AppendError::TargetHashMismatch));
}

#[test]
fn canonical_hash_is_verified_and_encoding_is_strict() {
    let operation = apply(2, 1, 1, 0, vec![], "new", "old");
    let bytes = operation.to_bytes().unwrap();
    assert_eq!(Operation::from_bytes(&bytes).unwrap(), operation);
    assert!(
        bytes
            .windows(b"\"causal_depth\"".len())
            .any(|window| { window == b"\"causal_depth\"" })
    );
    assert!(
        !bytes
            .windows(b"\"base_version\"".len())
            .any(|window| { window == b"\"base_version\"" })
    );
    assert_eq!(
        operation.content_hash().bytes(),
        ContentHash::from_sha256(sha2::Sha256::digest(operation.canonical_bytes()).into()).bytes()
    );

    let mut tampered = bytes.clone();
    let marker = b"\"content_hash\":\"";
    let start = tampered
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    tampered[start] = if tampered[start] == b'0' { b'1' } else { b'0' };
    assert_eq!(
        Operation::from_bytes(&tampered),
        Err(OperationError::HashMismatch)
    );

    let mut future = bytes.clone();
    let version = b"\"schema_version\":\"1\"";
    let position = future
        .windows(version.len())
        .position(|window| window == version)
        .unwrap();
    future[position + version.len() - 2] = b'2';
    assert!(matches!(
        Operation::from_bytes(&future),
        Err(OperationError::MalformedEncoding
            | OperationError::HashMismatch
            | OperationError::InvalidInverse)
    ));

    let mut unknown = bytes[..bytes.len() - 1].to_vec();
    unknown.extend_from_slice(b",\"unknown\":true}");
    assert_eq!(
        Operation::from_bytes(&unknown),
        Err(OperationError::MalformedEncoding)
    );

    let mut noncanonical = bytes.clone();
    noncanonical.push(b'\n');
    assert_eq!(
        Operation::from_bytes(&noncanonical),
        Err(OperationError::NonCanonicalEncoding)
    );
}

#[test]
fn wire_length_preflight_matches_every_operation_shape() {
    let root = apply(2, 1, 1, 0, vec![], "escaped \\\"value\\\"", "initial");
    let left = apply(3, 1, 2, 1, vec![root.operation_id()], "left", "root");
    let right = apply(4, 2, 2, 1, vec![root.operation_id()], "right", "root");
    let root_reference = OperationReference::from(&root);
    let undo = Operation::try_undo(
        op_id(6),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id()],
        root_reference,
    )
    .unwrap();
    let redo = Operation::try_redo(
        op_id(7),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(4).unwrap(),
        CausalDepth::new(3),
        vec![undo.operation_id()],
        root_reference,
    )
    .unwrap();
    let resolution = Operation::try_resolve(
        op_id(8),
        project(),
        actor(4),
        device(4),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id(), right.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&right),
        Mutation::set_project_name(text("chosen")),
        Mutation::set_project_name(text("root")),
    )
    .unwrap();
    let v2 = Operation::try_apply_v2(
        op_id(9),
        project(),
        actor(5),
        device(5),
        LogicalTimestamp::new(5).unwrap(),
        CausalDepth::new(0),
        Vec::new(),
        Mutation::set_project_name(text("v2")),
        InversePrior::ProjectName {
            name: text("initial"),
        },
    )
    .unwrap();

    for operation in [root, left, right, undo, redo, resolution, v2] {
        assert_eq!(
            operation.wire_bytes_len().unwrap(),
            operation.to_bytes().unwrap().len()
        );
    }
}

#[test]
fn v1_canonical_contract_remains_unchanged_while_v2_is_explicit() {
    let v1 = apply(2, 1, 1, 0, vec![], "new", "old");
    assert_eq!(v1.schema_version(), OperationSchemaVersion::V1);
    assert_eq!(
        v1.canonical_bytes(),
        br#"{"schema_version":"1","operation_id":"02020202020202020202020202020202","project_id":"01010101010101010101010101010101","actor_id":"01010101010101010101010101010101","device_id":"01010101010101010101010101010101","logical_time":1,"causal_depth":0,"parents":[],"payload":{"kind":"apply","data":{"mutation":{"kind":"set_project_name","data":{"name":"new"}}}},"inverse":{"kind":"apply","data":{"mutation":{"kind":"set_project_name","data":{"name":"old"}}}}}"#
    );
    assert_eq!(
        v1.content_hash().bytes(),
        [
            0x7b, 0x4f, 0x2b, 0xc6, 0x12, 0xd9, 0x34, 0x76, 0xaa, 0x8e, 0xb6, 0xb2, 0xf4, 0x3e,
            0x99, 0x84, 0xc5, 0xe6, 0xbe, 0x1d, 0x4a, 0x18, 0xfd, 0x56, 0x1f, 0x49, 0xe8, 0x65,
            0x6f, 0xda, 0x3a, 0xfa,
        ]
    );
    assert_eq!(
        Operation::from_bytes(
            br#"{"schema_version":"1","operation_id":"02020202020202020202020202020202","project_id":"01010101010101010101010101010101","actor_id":"01010101010101010101010101010101","device_id":"01010101010101010101010101010101","logical_time":1,"causal_depth":0,"parents":[],"payload":{"kind":"apply","data":{"mutation":{"kind":"set_project_name","data":{"name":"new"}}}},"inverse":{"kind":"apply","data":{"mutation":{"kind":"set_project_name","data":{"name":"old"}}}},"content_hash":"7b4f2bc612d93476aa8eb6b2f43e9984c5e6be1d4a18fd561f49e8656fda3afa"}"#
        )
        .unwrap(),
        v1
    );

    let v2 = Operation::try_apply_v2(
        op_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::set_project_name(text("new")),
        InversePrior::ProjectName { name: text("old") },
    )
    .unwrap();
    assert_eq!(v2.schema_version(), OperationSchemaVersion::V2);
    assert_ne!(v1.canonical_bytes(), v2.canonical_bytes());
    assert_eq!(Operation::from_bytes(&v2.to_bytes().unwrap()).unwrap(), v2);
}

#[test]
fn v2_priors_preserve_known_and_unknown_calibration_without_sentinels() {
    let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
    let calibration_id = CalibrationId::from_bytes([8; 16]).unwrap();
    let previous_calibration_id = CalibrationId::from_bytes([6; 16]).unwrap();
    let known = Operation::try_apply_v2(
        op_id(2),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::activate_calibration(map_id, calibration_id),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Known(previous_calibration_id),
        },
    )
    .unwrap();
    assert_eq!(
        known.inverse(),
        &InverseMetadata::ApplyV2 {
            prior: InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Known(previous_calibration_id),
            }
        }
    );
    assert_eq!(
        Operation::from_bytes(&known.to_bytes().unwrap()).unwrap(),
        known
    );
    let known_undo = Operation::try_undo_v2(
        op_id(6),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![known.operation_id()],
        OperationReference::from(&known),
    )
    .unwrap();
    let known_set = OperationSet::from_operations([known.clone(), known_undo]).unwrap();
    assert_eq!(
        known_set.replay_state().unwrap()[&FieldKey::MapCalibration(map_id)],
        Mutation::activate_calibration(map_id, previous_calibration_id)
    );

    let unknown = Operation::try_apply_v2(
        op_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::activate_calibration(map_id, calibration_id),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
        },
    )
    .unwrap();
    assert_eq!(
        Operation::from_bytes(&unknown.to_bytes().unwrap()).unwrap(),
        unknown
    );
    let InverseMetadata::ApplyV2 { prior } = unknown.inverse() else {
        panic!("V2 calibration apply must retain typed prior");
    };
    assert_eq!(prior.as_mutation(), Err(OperationError::TypedPriorRequired));
    let unknown_undo = Operation::try_undo_v2(
        op_id(5),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![unknown.operation_id()],
        OperationReference::from(&unknown),
    )
    .unwrap();
    let unknown_set = OperationSet::from_operations([unknown, unknown_undo]).unwrap();
    let typed_effects = unknown_set.replay_effects().unwrap();
    assert_eq!(typed_effects.len(), 2);
    assert_eq!(
        typed_effects[1].field_key(),
        FieldKey::MapCalibration(map_id)
    );
    assert_eq!(
        typed_effects[1].calibration(),
        Some(&Evidence::Unknown(UnknownReason::NotMeasured))
    );
    assert!(unknown_set.validate_replay_semantics().is_ok());
    assert_eq!(
        unknown_set.replay(),
        Err(MergeError::Operation(OperationError::TypedPriorRequired))
    );

    assert_eq!(
        Operation::try_apply_v2(
            op_id(4),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_id, calibration_id),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Unknown(UnknownReason::ClockUnavailable),
            },
        ),
        Err(OperationError::InvalidInverse)
    );
}

#[test]
fn v2_prior_identity_and_shape_are_validated_before_hashing() {
    let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
    let other_map_id = MapAssetId::from_bytes([9; 16]).unwrap();
    let calibration_id = CalibrationId::from_bytes([8; 16]).unwrap();
    let site_id = SiteId::from_bytes([4; 16]).unwrap();
    let other_site_id = SiteId::from_bytes([5; 16]).unwrap();
    let site = Operation::try_apply_v2(
        op_id(1),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::set_site_name(site_id, text("new")),
        InversePrior::SiteName {
            site_id,
            name: text("old"),
        },
    )
    .unwrap();
    let site_set = OperationSet::from_operations([site.clone()]).unwrap();
    assert_eq!(
        site_set.replay_state().unwrap()[&FieldKey::SiteName(site_id)],
        Mutation::set_site_name(site_id, text("new"))
    );
    assert_eq!(
        Operation::from_bytes(&site.to_bytes().unwrap()).unwrap(),
        site
    );
    let mut forged = site.to_bytes().unwrap();
    let name_marker = b"\"name\":\"old\"}";
    let name_end = forged
        .windows(name_marker.len())
        .position(|window| window == name_marker)
        .unwrap()
        + name_marker.len()
        - 1;
    forged.splice(name_end..name_end, br#",\"extra\":true"#.iter().copied());
    assert_eq!(
        Operation::from_bytes(&forged),
        Err(OperationError::MalformedEncoding)
    );
    assert_eq!(
        Operation::try_apply_v2(
            op_id(1),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::set_site_name(site_id, text("new")),
            InversePrior::SiteName {
                site_id: other_site_id,
                name: text("old"),
            },
        ),
        Err(OperationError::InvalidInverse)
    );
    assert_eq!(
        Operation::try_apply_v2(
            op_id(2),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::activate_calibration(map_id, calibration_id),
            InversePrior::MapCalibration {
                map_id: other_map_id,
                calibration: Evidence::Known(calibration_id),
            },
        ),
        Err(OperationError::InvalidInverse)
    );
    assert_eq!(
        Operation::try_apply_v2(
            op_id(2),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::set_project_name(text("new")),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Known(calibration_id),
            },
        ),
        Err(OperationError::InvalidInverse)
    );
}

#[test]
fn v2_resolution_retains_a_typed_prior_and_replays_the_selected_value() {
    let root = Operation::try_apply_v2(
        op_id(20),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::set_project_name(text("root")),
        InversePrior::ProjectName {
            name: text("initial"),
        },
    )
    .unwrap();
    let left = Operation::try_apply_v2(
        op_id(21),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![root.operation_id()],
        Mutation::set_project_name(text("left")),
        InversePrior::ProjectName { name: text("root") },
    )
    .unwrap();
    let right = Operation::try_apply_v2(
        op_id(22),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![root.operation_id()],
        Mutation::set_project_name(text("right")),
        InversePrior::ProjectName { name: text("root") },
    )
    .unwrap();
    let resolution = Operation::try_resolve_v2(
        op_id(23),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id(), right.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&right),
        Mutation::set_project_name(text("chosen")),
        InversePrior::ProjectName { name: text("root") },
    )
    .unwrap();
    assert!(matches!(
        resolution.inverse(),
        InverseMetadata::ApplyV2 {
            prior: InversePrior::ProjectName { .. }
        }
    ));
    let set = OperationSet::from_operations([root, left, right, resolution]).unwrap();
    assert_eq!(
        set.merge(&OperationSet::empty(project()))
            .unwrap()
            .into_applyable()
            .unwrap()
            .replay_state()
            .unwrap()[&FieldKey::ProjectName],
        Mutation::set_project_name(text("chosen"))
    );
}

#[test]
fn v2_unknown_and_known_concurrent_effects_are_typed_and_resolvable() {
    let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
    let root_calibration = CalibrationId::from_bytes([8; 16]).unwrap();
    let right_calibration = CalibrationId::from_bytes([9; 16]).unwrap();
    let chosen_calibration = CalibrationId::from_bytes([10; 16]).unwrap();
    let root = Operation::try_apply_v2(
        op_id(2),
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
        op_id(3),
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
        op_id(4),
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

    let branched =
        OperationSet::from_operations([root.clone(), left.clone(), right.clone()]).unwrap();
    let outcome = branched.merge(&OperationSet::empty(project())).unwrap();
    assert_eq!(outcome.conflicts().len(), 1);
    let conflict = &outcome.conflicts()[0];
    assert_eq!(conflict.left(), OperationReference::from(&left));
    assert_eq!(conflict.right(), OperationReference::from(&right));
    assert!(conflict.left_mutation().is_none());
    assert_eq!(
        conflict.right_mutation(),
        Some(&Mutation::activate_calibration(map_id, right_calibration))
    );
    assert!(matches!(
        conflict.left_effect(),
        AppliedEffect::Calibration {
            operation_id,
            map_id: effect_map,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
        } if *operation_id == left.operation_id() && *effect_map == map_id
    ));
    let conflict_bytes = serde_json::to_vec(conflict).unwrap();
    let decoded_conflict: MergeConflict = serde_json::from_slice(&conflict_bytes).unwrap();
    assert_eq!(&decoded_conflict, conflict);

    let resolution = Operation::try_resolve_v2(
        op_id(5),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id(), right.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&right),
        Mutation::activate_calibration(map_id, chosen_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
        },
    )
    .unwrap();
    let resolved =
        OperationSet::from_operations([root.clone(), left.clone(), right.clone(), resolution])
            .unwrap();
    let resolved_outcome = resolved
        .merge(&OperationSet::empty(project()))
        .unwrap()
        .into_applyable()
        .unwrap();
    let effects = resolved_outcome.replay_effects().unwrap();
    assert!(matches!(
        effects.last(),
        Some(AppliedEffect::Mutation(applied))
            if applied.mutation() == &Mutation::activate_calibration(map_id, chosen_calibration)
    ));

    let unknown_resolution = Operation::try_resolve_v2(
        op_id(6),
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
            calibration: Evidence::Known(root_calibration),
        },
    )
    .unwrap();
    assert_eq!(
        Operation::from_bytes(&unknown_resolution.to_bytes().unwrap()).unwrap(),
        unknown_resolution
    );
    let unknown_resolved =
        OperationSet::from_operations([root, left.clone(), right.clone(), unknown_resolution])
            .unwrap();
    let unknown_effects = unknown_resolved
        .merge(&OperationSet::empty(project()))
        .unwrap()
        .into_applyable()
        .unwrap()
        .replay_effects()
        .unwrap();
    assert!(matches!(
        unknown_effects.last(),
        Some(AppliedEffect::Calibration {
            map_id: effect_map,
            calibration: Evidence::Unknown(UnknownReason::NotMeasured),
            ..
        }) if *effect_map == map_id
    ));
    assert!(unknown_resolved.validate_replay_semantics().is_ok());

    assert_eq!(
        Operation::try_resolve_v2(
            op_id(7),
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
                Evidence::Unknown(UnknownReason::ClockUnavailable),
            ),
            InversePrior::MapCalibration {
                map_id,
                calibration: Evidence::Known(root_calibration),
            },
        ),
        Err(OperationError::InvalidInverse)
    );
}

#[test]
fn v2_known_typed_undo_and_known_activation_share_merge_identity() {
    let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
    let prior_calibration = CalibrationId::from_bytes([8; 16]).unwrap();
    let target_calibration = CalibrationId::from_bytes([9; 16]).unwrap();
    let target = Operation::try_apply_v2(
        op_id(10),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::activate_calibration(map_id, target_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Known(prior_calibration),
        },
    )
    .unwrap();
    let undo = Operation::try_undo_v2(
        op_id(11),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![target.operation_id()],
        OperationReference::from(&target),
    )
    .unwrap();
    let activation = Operation::try_apply_v2(
        op_id(12),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![target.operation_id()],
        Mutation::activate_calibration(map_id, prior_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Known(target_calibration),
        },
    )
    .unwrap();

    let set = OperationSet::from_operations([target, undo, activation]).unwrap();
    let outcome = set.merge(&OperationSet::empty(project())).unwrap();
    assert!(outcome.conflicts().is_empty());
    assert!(set.replay_effects().is_ok());
}

#[test]
fn equal_value_branches_do_not_mask_distinct_concurrent_toggle_intents() {
    let map_id = MapAssetId::from_bytes([7; 16]).unwrap();
    let prior_calibration = CalibrationId::from_bytes([8; 16]).unwrap();
    let applied_calibration = CalibrationId::from_bytes([9; 16]).unwrap();
    let first = Operation::try_apply_v2(
        op_id(1),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::activate_calibration(map_id, applied_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Known(prior_calibration),
        },
    )
    .unwrap();
    let second = Operation::try_apply_v2(
        op_id(2),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::activate_calibration(map_id, applied_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Known(prior_calibration),
        },
    )
    .unwrap();
    let undo_first = Operation::try_undo_v2(
        op_id(10),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id(), second.operation_id()],
        OperationReference::from(&first),
    )
    .unwrap();
    let activate_prior = Operation::try_apply_v2(
        op_id(11),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id(), second.operation_id()],
        Mutation::activate_calibration(map_id, prior_calibration),
        InversePrior::MapCalibration {
            map_id,
            calibration: Evidence::Known(applied_calibration),
        },
    )
    .unwrap();
    let undo_second = Operation::try_undo_v2(
        op_id(12),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id(), second.operation_id()],
        OperationReference::from(&second),
    )
    .unwrap();

    let all = [
        first.clone(),
        second.clone(),
        undo_first.clone(),
        activate_prior.clone(),
        undo_second.clone(),
    ];
    let set = OperationSet::from_operations(all.clone()).unwrap();
    let outcome = set.merge(&OperationSet::empty(project())).unwrap();
    assert_eq!(outcome.conflicts().len(), 1);
    assert_eq!(
        outcome.conflicts()[0].left(),
        OperationReference::from(&undo_first)
    );
    assert_eq!(
        outcome.conflicts()[0].right(),
        OperationReference::from(&undo_second)
    );

    let reversed = [undo_second, activate_prior, second, undo_first, first];
    let reversed_set = OperationSet::from_operations(reversed).unwrap();
    let reversed_outcome = reversed_set.merge(&OperationSet::empty(project())).unwrap();
    assert_eq!(reversed_outcome.conflicts(), outcome.conflicts());
}

#[test]
fn v2_floor_binding_is_explicitly_non_reversible_and_cannot_be_undone() {
    let reference = ImmutableReference::new(
        ContentHash::from_sha256([7; 32]),
        text("application/vnd.apache.parquet"),
        1024,
    )
    .unwrap();
    let mutation = Mutation::bind_floor_evidence(FloorId::from_bytes([3; 16]).unwrap(), reference);
    let binding = Operation::try_apply_v2_non_reversible(
        op_id(2),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        mutation.clone(),
        NonReversibleReason::FloorEvidenceBinding,
    )
    .unwrap();
    assert!(matches!(
        binding.inverse(),
        InverseMetadata::NonReversible {
            reason: NonReversibleReason::FloorEvidenceBinding
        }
    ));
    assert_eq!(
        Operation::from_bytes(&binding.to_bytes().unwrap()).unwrap(),
        binding
    );

    let undo = Operation::try_undo_v2(
        op_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![binding.operation_id()],
        OperationReference::from(&binding),
    )
    .unwrap();
    assert_eq!(
        OperationSet::from_operations([binding.clone(), undo]),
        Err(OperationError::NonReversibleTarget)
    );

    let binding_ref = OperationReference::from(&binding);
    let mut log = OperationLog::new(project());
    log.append(binding).unwrap();
    let undo = Operation::try_undo_v2(
        op_id(4),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![op_id(2)],
        binding_ref,
    )
    .unwrap();
    assert_eq!(
        log.append(undo),
        Err(AppendError::Operation(OperationError::NonReversibleTarget))
    );

    assert_eq!(
        Operation::try_apply_v2_non_reversible(
            op_id(5),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(1).unwrap(),
            CausalDepth::new(0),
            vec![],
            Mutation::set_project_name(text("new")),
            NonReversibleReason::FloorEvidenceBinding,
        ),
        Err(OperationError::InvalidInverse)
    );
}

#[test]
fn toggles_cannot_cross_v1_and_v2_inverse_contracts() {
    let v1 = apply(2, 1, 1, 0, vec![], "new", "old");
    let v1_undo = Operation::try_undo_v2(
        op_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![v1.operation_id()],
        OperationReference::from(&v1),
    )
    .unwrap();
    assert_eq!(
        OperationSet::from_operations([v1, v1_undo]),
        Err(OperationError::VersionMismatch)
    );
}

#[test]
fn future_and_resource_limited_inputs_fail_closed() {
    assert_eq!(
        Operation::from_bytes(&vec![b' '; MAX_OPERATION_WIRE_BYTES + 1]),
        Err(OperationError::ResourceLimit("operation_wire_bytes"))
    );
    assert_eq!(
        LogicalTimestamp::new(0),
        Err(OperationError::InvalidLogicalTimestamp)
    );
}

#[test]
fn observation_chunks_are_represented_only_as_bounded_immutable_references() {
    let reference = ImmutableReference::new(
        ContentHash::from_sha256([7; 32]),
        text("application/vnd.apache.parquet"),
        1024,
    )
    .unwrap();
    let mutation = Mutation::bind_floor_evidence(FloorId::from_bytes([3; 16]).unwrap(), reference);
    let operation = Operation::try_apply(
        op_id(2),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        mutation.clone(),
        mutation,
    )
    .unwrap();
    let encoded = operation.to_bytes().unwrap();
    assert!(encoded.len() < MAX_OPERATION_WIRE_BYTES);
    assert!(!encoded.windows(5).any(|window| window == b"bytes"));
    assert!(ImmutableReference::new(ContentHash::from_sha256([7; 32]), text("chunk"), 0).is_err());
}

#[test]
fn strict_typed_payloads_do_not_admit_unknown_operation_kinds() {
    let operation = apply(2, 1, 1, 0, vec![], "new", "old");
    let mut bytes = operation.to_bytes().unwrap();
    let marker = b"\"apply\"";
    let position = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    bytes[position + 2] = b'x';
    assert_eq!(
        Operation::from_bytes(&bytes),
        Err(OperationError::MalformedEncoding)
    );
}

#[test]
fn log_rejects_wrong_causal_depth_and_parent_without_mutating_state() {
    let first = apply(2, 1, 1, 0, vec![], "new", "old");
    let mut log = OperationLog::new(project());
    log.append(first).unwrap();
    assert_eq!(
        log.append(apply(3, 1, 2, 1, vec![op_id(9)], "wrong-parent", "old")),
        Err(AppendError::ParentConflict)
    );
    assert_eq!(
        log.append(apply(4, 2, 2, 2, vec![op_id(2)], "wrong-base", "old")),
        Err(AppendError::CausalDepthConflict {
            expected: CausalDepth::new(1),
            actual: CausalDepth::new(2)
        })
    );
    assert_eq!(log.revision(), ProjectVersion::new(1));
}

#[test]
fn references_and_payloads_round_trip_strictly() {
    let reference = ImmutableReference::new(
        ContentHash::from_sha256([8; 32]),
        text("application/octet-stream"),
        4,
    )
    .unwrap();
    let operation = Operation::try_apply(
        op_id(2),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::bind_floor_evidence(FloorId::from_bytes([5; 16]).unwrap(), reference.clone()),
        Mutation::bind_floor_evidence(FloorId::from_bytes([5; 16]).unwrap(), reference),
    )
    .unwrap();
    let decoded: Operation = serde_json::from_slice(&operation.to_bytes().unwrap()).unwrap();
    assert_eq!(decoded, operation);
    assert!(matches!(decoded.payload(), OperationPayload::Apply { .. }));
    assert!(matches!(decoded.inverse(), InverseMetadata::Apply { .. }));
    let _ = CalibrationId::from_bytes([6; 16]).unwrap();
    let _ = MapAssetId::from_bytes([7; 16]).unwrap();
}

#[test]
fn large_same_field_no_conflict_uses_a_bounded_frontier() {
    let count = 10_000u64;
    let mut operations = Vec::with_capacity(count as usize);
    let mut parent = None;
    for index in 0..count {
        let operation_id = op_id_u64(index + 1);
        let operation = Operation::try_apply(
            operation_id,
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(index + 1).unwrap(),
            CausalDepth::new(index),
            parent.into_iter().collect(),
            Mutation::set_project_name(text("stable")),
            Mutation::set_project_name(text("previous")),
        )
        .unwrap();
        parent = Some(operation_id);
        operations.push(operation);
    }

    let set = OperationSet::from_operations(operations).unwrap();
    assert_eq!(set.replay().unwrap().len(), count as usize);
}

#[test]
fn width_max_equal_effect_roots_collapse_to_one_frontier_representative() {
    let count = MAX_OPERATION_COUNT as u64;
    let operations = (0..count)
        .map(|index| {
            Operation::try_apply(
                op_id_u64(index + 1),
                project(),
                actor(1),
                device(1),
                LogicalTimestamp::new(index + 1).unwrap(),
                CausalDepth::new(0),
                vec![],
                Mutation::set_project_name(text("same")),
                Mutation::set_project_name(text("old")),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let set = OperationSet::from_operations(operations).unwrap();
    let outcome = set.merge(&OperationSet::empty(project())).unwrap();
    assert!(outcome.conflicts().is_empty());
    assert_eq!(outcome.merged().operations().count(), MAX_OPERATION_COUNT);
}

#[test]
fn concurrent_duplicate_toggles_are_idempotent_but_mixed_intents_conflict() {
    let first = apply(2, 1, 1, 0, vec![], "first", "initial");
    let first_ref = OperationReference::from(&first);
    let undo_left = Operation::try_undo(
        op_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id()],
        first_ref,
    )
    .unwrap();
    let undo_right = Operation::try_undo(
        op_id(4),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id()],
        first_ref,
    )
    .unwrap();
    let duplicate_undo =
        OperationSet::from_operations([first.clone(), undo_left.clone(), undo_right.clone()])
            .unwrap();
    let duplicate_undo_outcome = duplicate_undo
        .merge(&OperationSet::empty(project()))
        .unwrap();
    assert!(duplicate_undo_outcome.conflicts().is_empty());
    assert_eq!(duplicate_undo.replay().unwrap().len(), 2);

    let redo_left = Operation::try_redo(
        op_id(5),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![undo_left.operation_id()],
        first_ref,
    )
    .unwrap();
    let redo_right = Operation::try_redo(
        op_id(6),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![undo_left.operation_id()],
        first_ref,
    )
    .unwrap();
    let duplicate_redo =
        OperationSet::from_operations([first.clone(), undo_left.clone(), redo_left, redo_right])
            .unwrap();
    let duplicate_redo_outcome = duplicate_redo
        .merge(&OperationSet::empty(project()))
        .unwrap();
    assert!(duplicate_redo_outcome.conflicts().is_empty());
    assert_eq!(duplicate_redo.replay().unwrap().len(), 3);

    let mixed_redo = Operation::try_redo(
        op_id(7),
        project(),
        actor(2),
        device(2),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![first.operation_id()],
        first_ref,
    )
    .unwrap();
    let mixed =
        OperationSet::from_operations([first.clone(), undo_left.clone(), mixed_redo]).unwrap();
    let mixed_outcome = mixed.merge(&OperationSet::empty(project())).unwrap();
    assert_eq!(mixed_outcome.conflicts().len(), 1);
    let mixed_conflict = &mixed_outcome.conflicts()[0];
    assert!(matches!(
        (mixed_conflict.left_intent(), mixed_conflict.right_intent()),
        (
            ConflictIntent::Toggle {
                direction: ToggleDirection::Undo,
                ..
            },
            ConflictIntent::Toggle {
                direction: ToggleDirection::Redo,
                ..
            }
        ) | (
            ConflictIntent::Toggle {
                direction: ToggleDirection::Redo,
                ..
            },
            ConflictIntent::Toggle {
                direction: ToggleDirection::Undo,
                ..
            }
        )
    ));
    assert!(matches!(
        mixed_outcome.into_applyable(),
        Err(MergeError::Conflicts(conflicts)) if conflicts.len() == 1
    ));

    let mixed_join = apply(
        10,
        3,
        3,
        2,
        vec![undo_left.operation_id(), op_id(7)],
        "joined-mixed",
        "initial",
    );
    let mixed_joined = OperationSet::from_operations([
        mixed.operation(undo_left.operation_id()).unwrap().clone(),
        mixed.operation(op_id(7)).unwrap().clone(),
        mixed.operation(op_id(2)).unwrap().clone(),
        mixed_join,
    ])
    .unwrap();
    assert!(matches!(
        mixed_joined
            .merge(&OperationSet::empty(project()))
            .unwrap()
            .into_applyable(),
        Err(MergeError::Conflicts(conflicts)) if conflicts.len() == 1
    ));

    let sequential_duplicate = Operation::try_undo(
        op_id(9),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![undo_left.operation_id()],
        first_ref,
    )
    .unwrap();
    assert_eq!(
        OperationSet::from_operations([first.clone(), undo_left.clone(), sequential_duplicate])
            .unwrap()
            .replay(),
        Err(MergeError::InvalidToggle)
    );

    let duplicate_join = apply(
        8,
        3,
        3,
        2,
        vec![undo_left.operation_id(), undo_right.operation_id()],
        "joined",
        "initial",
    );
    let joined_duplicate_toggles = OperationSet::from_operations([
        duplicate_undo
            .operation(undo_left.operation_id())
            .unwrap()
            .clone(),
        duplicate_undo
            .operation(undo_right.operation_id())
            .unwrap()
            .clone(),
        duplicate_undo.operation(op_id(2)).unwrap().clone(),
        duplicate_join,
    ])
    .unwrap();
    assert!(
        joined_duplicate_toggles
            .merge(&OperationSet::empty(project()))
            .unwrap()
            .conflicts()
            .is_empty()
    );
    assert!(joined_duplicate_toggles.replay().is_ok());
}

#[test]
fn ancestry_validation_fails_with_a_structured_budget_on_a_deep_toggle_chain() {
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let root_ref = OperationReference::from(&root);
    let mut operations = vec![root];
    let mut parent = op_id(2);
    for index in 1..=3_000u64 {
        let operation_id = op_id_u64(index + 2);
        let operation = if index % 2 == 1 {
            Operation::try_undo(
                operation_id,
                project(),
                actor(1),
                device(1),
                LogicalTimestamp::new(index + 1).unwrap(),
                CausalDepth::new(index),
                vec![parent],
                root_ref,
            )
        } else {
            Operation::try_redo(
                operation_id,
                project(),
                actor(1),
                device(1),
                LogicalTimestamp::new(index + 1).unwrap(),
                CausalDepth::new(index),
                vec![parent],
                root_ref,
            )
        }
        .unwrap();
        parent = operation_id;
        operations.push(operation);
    }
    assert_eq!(
        OperationSet::from_operations(operations),
        Err(OperationError::ResourceLimit("ancestry_work"))
    );
}

#[test]
fn cyclic_parent_admission_fails_closed_before_topological_replay() {
    let first = apply(2, 1, 1, 1, vec![op_id(3)], "first", "initial");
    let second = apply(3, 1, 2, 1, vec![op_id(2)], "second", "first");
    assert_eq!(
        OperationSet::from_operations([first, second]),
        Err(OperationError::InvalidCausality)
    );

    let self_parent = apply(4, 1, 1, 1, vec![op_id(4)], "self", "initial");
    assert_eq!(
        OperationSet::from_operations([self_parent]),
        Err(OperationError::CyclicCausality)
    );
}

#[test]
fn unequal_branch_lengths_join_at_max_causal_depth_and_replay() {
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let left = site_apply(3, 1, 2, 1, vec![root.operation_id()], 3, "left", "old");
    let left_tip = site_apply(4, 1, 3, 2, vec![left.operation_id()], 3, "left-tip", "left");
    let right = site_apply(5, 2, 2, 1, vec![root.operation_id()], 4, "right", "old");
    let join = apply(
        6,
        3,
        4,
        3,
        vec![left_tip.operation_id(), right.operation_id()],
        "joined",
        "root",
    );
    let set = OperationSet::from_operations([
        join.clone(),
        right.clone(),
        left_tip.clone(),
        left.clone(),
        root.clone(),
    ])
    .unwrap();
    assert_eq!(join.causal_depth(), CausalDepth::new(3));
    // The join has four unique proper ancestors but causal depth three; the
    // depth is a DAG coordinate, not a materialized project revision.
    assert_eq!(set.operations().count(), 5);
    assert_eq!(
        set.replay_state().unwrap()[&FieldKey::ProjectName],
        Mutation::set_project_name(text("joined"))
    );
    assert_eq!(
        set.replay_state().unwrap()[&FieldKey::SiteName(SiteId::from_bytes([3; 16]).unwrap())],
        Mutation::set_site_name(SiteId::from_bytes([3; 16]).unwrap(), text("left-tip"))
    );

    let invalid_base = apply(
        7,
        3,
        4,
        2,
        vec![left_tip.operation_id(), right.operation_id()],
        "invalid-base",
        "root",
    );
    assert_eq!(
        OperationSet::from_operations([
            root.clone(),
            left.clone(),
            left_tip.clone(),
            right.clone(),
            invalid_base,
        ]),
        Err(OperationError::InvalidCausality)
    );

    let redundant_parent = apply(
        8,
        3,
        4,
        3,
        vec![root.operation_id(), left_tip.operation_id()],
        "redundant-parent",
        "root",
    );
    assert_eq!(
        OperationSet::from_operations([root, left, left_tip, redundant_parent]),
        Err(OperationError::InvalidCausality)
    );
}

#[test]
fn repeated_undo_and_redo_are_rejected_before_log_mutation() {
    let first = apply(2, 1, 1, 0, vec![], "first", "initial");
    let first_ref = OperationReference::from(&first);
    let mut log = OperationLog::new(project());
    log.append(first).unwrap();
    let undo = Operation::try_undo(
        op_id(3),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(2).unwrap(),
        CausalDepth::new(1),
        vec![op_id(2)],
        first_ref,
    )
    .unwrap();
    log.append(undo).unwrap();

    let before_revision = log.revision();
    let before_tip = log.tip();
    let before_count = log.operations().count();
    let repeated_undo = Operation::try_undo(
        op_id(4),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![op_id(3)],
        first_ref,
    )
    .unwrap();
    assert_eq!(log.append(repeated_undo), Err(AppendError::InvalidToggle));
    assert_eq!(log.revision(), before_revision);
    assert_eq!(log.tip(), before_tip);
    assert_eq!(log.operations().count(), before_count);

    let redo = Operation::try_redo(
        op_id(5),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![op_id(3)],
        first_ref,
    )
    .unwrap();
    log.append(redo).unwrap();
    let repeated_redo = Operation::try_redo(
        op_id(6),
        project(),
        actor(1),
        device(1),
        LogicalTimestamp::new(4).unwrap(),
        CausalDepth::new(3),
        vec![op_id(5)],
        first_ref,
    )
    .unwrap();
    assert_eq!(log.append(repeated_redo), Err(AppendError::InvalidToggle));
    assert_eq!(log.revision(), ProjectVersion::new(3));
    assert_eq!(log.tip(), Some(op_id(5)));
    assert_eq!(log.operations().count(), 3);
}

#[test]
fn typed_resolution_clears_its_referenced_conflict_and_retains_audit_refs() {
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let left = apply(3, 1, 2, 1, vec![root.operation_id()], "left", "root");
    let right = apply(4, 2, 2, 1, vec![root.operation_id()], "right", "root");
    let left_set = OperationSet::from_operations([root.clone(), left.clone()]).unwrap();
    let right_set = OperationSet::from_operations([root.clone(), right.clone()]).unwrap();
    assert_eq!(left_set.merge(&right_set).unwrap().conflicts().len(), 1);

    let resolution = Operation::try_resolve(
        op_id(5),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id(), right.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&right),
        Mutation::set_project_name(text("chosen")),
        Mutation::set_project_name(text("root")),
    )
    .unwrap();
    let resolved =
        OperationSet::from_operations([root, left.clone(), right.clone(), resolution.clone()])
            .unwrap();
    let resolved_outcome = resolved.merge(&OperationSet::empty(project())).unwrap();
    assert!(resolved_outcome.conflicts().is_empty());
    assert_eq!(
        resolved_outcome.merged().replay_state().unwrap()[&FieldKey::ProjectName],
        Mutation::set_project_name(text("chosen"))
    );
    assert_eq!(
        resolved_outcome
            .merged()
            .operation(resolution.operation_id())
            .unwrap()
            .resolution_references(),
        Some((
            OperationReference::from(&left),
            OperationReference::from(&right)
        ))
    );

    let wrong_field = Operation::try_resolve(
        op_id(6),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id(), right.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&right),
        Mutation::set_site_name(SiteId::from_bytes([9; 16]).unwrap(), text("wrong-field")),
        Mutation::set_site_name(SiteId::from_bytes([9; 16]).unwrap(), text("old")),
    )
    .unwrap();
    assert_eq!(
        OperationSet::from_operations([root_for_test(), left.clone(), right.clone(), wrong_field]),
        Err(OperationError::InvalidResolution)
    );

    let invalid_reference = Operation::try_resolve(
        op_id(7),
        project(),
        actor(3),
        device(3),
        LogicalTimestamp::new(3).unwrap(),
        CausalDepth::new(2),
        vec![left.operation_id(), right.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&root_for_test()),
        Mutation::set_project_name(text("invalid")),
        Mutation::set_project_name(text("root")),
    )
    .unwrap();
    assert_eq!(
        OperationSet::from_operations([left, right, invalid_reference, root_for_test()]),
        Err(OperationError::InvalidResolution)
    );
}

#[test]
fn resolution_requires_current_canonical_heads_and_preserves_unrelated_conflicts() {
    let root = apply(2, 1, 1, 0, vec![], "root", "initial");
    let left = apply(3, 1, 2, 1, vec![root.operation_id()], "left", "root");
    let right = apply(4, 2, 2, 1, vec![root.operation_id()], "right", "root");
    let third = apply(5, 3, 2, 1, vec![root.operation_id()], "third", "root");
    let left_tip = apply(6, 1, 3, 2, vec![left.operation_id()], "left-tip", "left");
    let right_tip = apply(7, 2, 3, 2, vec![right.operation_id()], "right-tip", "right");

    let stale = Operation::try_resolve(
        op_id(8),
        project(),
        actor(4),
        device(4),
        LogicalTimestamp::new(4).unwrap(),
        CausalDepth::new(3),
        vec![left_tip.operation_id(), right_tip.operation_id()],
        OperationReference::from(&left),
        OperationReference::from(&right),
        Mutation::set_project_name(text("stale")),
        Mutation::set_project_name(text("root")),
    )
    .unwrap();
    assert_eq!(
        OperationSet::from_operations([
            root.clone(),
            left.clone(),
            right.clone(),
            left_tip.clone(),
            right_tip.clone(),
            stale,
        ]),
        Err(OperationError::InvalidResolution)
    );

    let reversed = Operation::try_resolve(
        op_id(9),
        project(),
        actor(4),
        device(4),
        LogicalTimestamp::new(4).unwrap(),
        CausalDepth::new(3),
        vec![left_tip.operation_id(), right_tip.operation_id()],
        OperationReference::from(&right_tip),
        OperationReference::from(&left_tip),
        Mutation::set_project_name(text("reversed")),
        Mutation::set_project_name(text("root")),
    )
    .unwrap();
    assert_eq!(
        OperationSet::from_operations([
            root.clone(),
            left.clone(),
            right.clone(),
            left_tip.clone(),
            right_tip.clone(),
            reversed,
        ]),
        Err(OperationError::InvalidResolution)
    );

    let resolution = Operation::try_resolve(
        op_id(10),
        project(),
        actor(4),
        device(4),
        LogicalTimestamp::new(4).unwrap(),
        CausalDepth::new(3),
        vec![left_tip.operation_id(), right_tip.operation_id()],
        OperationReference::from(&left_tip),
        OperationReference::from(&right_tip),
        Mutation::set_project_name(text("chosen")),
        Mutation::set_project_name(text("root")),
    )
    .unwrap();
    let outcome = OperationSet::from_operations([
        root,
        left,
        right,
        third.clone(),
        left_tip.clone(),
        right_tip.clone(),
        resolution.clone(),
    ])
    .unwrap()
    .merge(&OperationSet::empty(project()))
    .unwrap();
    assert!(!outcome.conflicts().is_empty());
    assert!(outcome.conflicts().iter().any(|conflict| {
        (conflict.left() == OperationReference::from(&third)
            || conflict.right() == OperationReference::from(&third))
            && (conflict.left() == OperationReference::from(&resolution)
                || conflict.right() == OperationReference::from(&resolution))
    }));
    assert!(!outcome.conflicts().iter().any(|conflict| {
        conflict.left() == OperationReference::from(&left_tip)
            && conflict.right() == OperationReference::from(&right_tip)
    }));
}

fn root_for_test() -> Operation {
    apply(2, 1, 1, 0, vec![], "root", "initial")
}

#[derive(Debug)]
struct CancelImmediately;

impl CancellationHook for CancelImmediately {
    fn is_cancelled(&mut self) -> bool {
        true
    }
}

#[derive(Debug)]
struct CancelAfter {
    remaining_checks: usize,
}

impl CancellationHook for CancelAfter {
    fn is_cancelled(&mut self) -> bool {
        if self.remaining_checks == 0 {
            true
        } else {
            self.remaining_checks -= 1;
            false
        }
    }
}

struct DuplicateThenPanic {
    operation: Operation,
    yielded: u8,
}

struct TooManyDuplicates {
    operation: Operation,
    yielded: usize,
}

impl Iterator for TooManyDuplicates {
    type Item = Operation;

    fn next(&mut self) -> Option<Self::Item> {
        if self.yielded > MAX_OPERATION_COUNT {
            panic!("duplicate iterator was not bounded at the format limit");
        }
        self.yielded += 1;
        Some(self.operation.clone())
    }
}

impl Iterator for DuplicateThenPanic {
    type Item = Operation;

    fn next(&mut self) -> Option<Self::Item> {
        match self.yielded {
            0 | 1 => {
                self.yielded += 1;
                Some(self.operation.clone())
            }
            _ => panic!("duplicate iterator was not cancelled before unbounded input"),
        }
    }
}

#[test]
fn caller_budget_accumulates_across_replay_calls() {
    let set = OperationSet::from_operations([root_for_test()]).unwrap();
    let operation_bytes = set.operation(op_id(2)).unwrap().canonical_bytes().len();
    let mut budget = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        384 + operation_bytes.saturating_mul(4),
    ));

    set.replay_effects_with_budget(&mut budget).unwrap();
    assert_eq!(
        budget.usage().working_set_bytes(),
        256 + operation_bytes.saturating_mul(4)
    );
    assert_eq!(
        set.replay_effects_with_budget(&mut budget),
        Err(MergeError::ResourceLimit(
            BudgetKind::WorkingSetBytes.label()
        ))
    );
    assert_eq!(
        budget.usage().working_set_bytes(),
        384 + operation_bytes.saturating_mul(4)
    );
}

#[test]
fn replay_budget_charges_referenced_payloads_for_repeated_toggles() {
    let unlimited = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    );

    fn replay_working_set_usage(target: Operation, limits: ResourceLimits) -> usize {
        let target_reference = OperationReference::from(&target);
        let undo = Operation::try_undo(
            op_id(3),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![target.operation_id()],
            target_reference,
        )
        .unwrap();
        let redo = Operation::try_redo(
            op_id(4),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(3).unwrap(),
            CausalDepth::new(2),
            vec![undo.operation_id()],
            target_reference,
        )
        .unwrap();
        let set = OperationSet::from_operations([target, undo, redo]).unwrap();
        let mut budget = ResourceBudget::new(limits);
        set.validate_replay_semantics_with_budget(&mut budget)
            .unwrap();
        budget.usage().working_set_bytes()
    }

    let short_target = apply(2, 1, 1, 0, vec![], "short", "initial");
    let long_value = "x".repeat(1024);
    let long_target = apply(2, 1, 1, 0, vec![], &long_value, "initial");
    let short_usage = replay_working_set_usage(short_target.clone(), unlimited);
    let long_usage = replay_working_set_usage(long_target.clone(), unlimited);
    let target_delta = long_target
        .canonical_bytes()
        .len()
        .saturating_sub(short_target.canonical_bytes().len());
    // The target is charged once as the current Apply and once for each of
    // the two later toggles. This differential oracle is independent of the
    // absolute structural charge schedule and fails if referenced payloads
    // are omitted from the budget.
    assert_eq!(
        long_usage.saturating_sub(short_usage),
        target_delta.saturating_mul(3)
    );

    let short_inverse_target = apply(5, 1, 1, 0, vec![], "short", "initial");
    let long_inverse_target = apply(5, 1, 1, 0, vec![], "short", &long_value);
    let short_inverse_usage = replay_working_set_usage(short_inverse_target.clone(), unlimited);
    let long_inverse_usage = replay_working_set_usage(long_inverse_target.clone(), unlimited);
    let inverse_target_delta = long_inverse_target
        .canonical_bytes()
        .len()
        .saturating_sub(short_inverse_target.canonical_bytes().len());
    assert_eq!(
        long_inverse_usage.saturating_sub(short_inverse_usage),
        inverse_target_delta.saturating_mul(3)
    );
}

#[test]
fn conflict_budget_charges_each_effect_payload_copy() {
    fn unlimited() -> ResourceLimits {
        ResourceLimits::new(
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
        )
    }

    fn conflicting_apply_usage(left: Operation) -> usize {
        let right = apply(3, 2, 1, 0, vec![], "right", "initial");
        let set = OperationSet::from_operations([left, right]).unwrap();
        let mut budget = ResourceBudget::new(unlimited());
        assert!(matches!(
            set.replay_effects_with_budget(&mut budget),
            Err(MergeError::Conflicts(conflicts)) if conflicts.len() == 1
        ));
        budget.usage().working_set_bytes()
    }

    let short = apply(2, 1, 1, 0, vec![], "short", "initial");
    let long_value = "x".repeat(1024);
    let long = apply(2, 1, 1, 0, vec![], &long_value, "initial");
    let short_usage = conflicting_apply_usage(short.clone());
    let long_usage = conflicting_apply_usage(long.clone());
    let payload_delta = long.canonical_bytes().len() - short.canonical_bytes().len();
    // A conflicting Apply owns four payload-bearing copies: event creation,
    // EffectValue conversion, canonical identity normalization, and the
    // retained conflict effect. This differential does not depend on the
    // absolute structural charge schedule.
    assert_eq!(long_usage - short_usage, payload_delta.saturating_mul(4));

    let target_short_inverse = apply(2, 1, 1, 0, vec![], "short", "initial");
    let target_long_inverse = apply(2, 1, 1, 0, vec![], "short", &long_value);
    fn conflicting_undo_usage(target: Operation) -> usize {
        let target_reference = OperationReference::from(&target);
        let undo = Operation::try_undo(
            op_id(3),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![target.operation_id()],
            target_reference,
        )
        .unwrap();
        let competitor = apply(4, 2, 3, 0, vec![], "competitor", "initial");
        let set = OperationSet::from_operations([target, undo, competitor]).unwrap();
        let mut budget = ResourceBudget::new(unlimited());
        assert!(matches!(
            set.replay_effects_with_budget(&mut budget),
            Err(MergeError::Conflicts(conflicts)) if conflicts.len() == 1
        ));
        budget.usage().working_set_bytes()
    }
    let short_inverse_usage = conflicting_undo_usage(target_short_inverse.clone());
    let long_inverse_usage = conflicting_undo_usage(target_long_inverse.clone());
    let inverse_delta =
        target_long_inverse.canonical_bytes().len() - target_short_inverse.canonical_bytes().len();
    // The target is the source for Apply and Undo event construction and
    // normalization (three copies each), plus the retained Undo conflict
    // effect (one copy).
    assert_eq!(
        long_inverse_usage - short_inverse_usage,
        inverse_delta.saturating_mul(7)
    );

    let target_short_forward = apply(2, 1, 1, 0, vec![], "short", "initial");
    let target_long_forward = apply(2, 1, 1, 0, vec![], &long_value, "initial");
    fn conflicting_redo_usage(target: Operation) -> usize {
        let target_reference = OperationReference::from(&target);
        let undo = Operation::try_undo(
            op_id(3),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![target.operation_id()],
            target_reference,
        )
        .unwrap();
        let redo = Operation::try_redo(
            op_id(4),
            project(),
            actor(1),
            device(1),
            LogicalTimestamp::new(3).unwrap(),
            CausalDepth::new(2),
            vec![undo.operation_id()],
            target_reference,
        )
        .unwrap();
        let competitor = apply(5, 2, 4, 0, vec![], "competitor", "initial");
        let set = OperationSet::from_operations([target, undo, redo, competitor]).unwrap();
        let mut budget = ResourceBudget::new(unlimited());
        assert!(matches!(
            set.replay_effects_with_budget(&mut budget),
            Err(MergeError::Conflicts(conflicts)) if conflicts.len() == 1
        ));
        budget.usage().working_set_bytes()
    }
    let short_forward_usage = conflicting_redo_usage(target_short_forward.clone());
    let long_forward_usage = conflicting_redo_usage(target_long_forward.clone());
    let forward_delta =
        target_long_forward.canonical_bytes().len() - target_short_forward.canonical_bytes().len();
    // Apply, Undo, and Redo each construct and normalize an event from the
    // target (three copies each), then the retained Redo conflict owns one.
    assert_eq!(
        long_forward_usage - short_forward_usage,
        forward_delta.saturating_mul(10)
    );

    fn resolved_usage(value: &str) -> (usize, usize) {
        let left = apply(6, 1, 1, 0, vec![], "left", "initial");
        let right = apply(7, 2, 1, 0, vec![], "right", "initial");
        let resolution = Operation::try_resolve(
            op_id(8),
            project(),
            actor(3),
            device(3),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![left.operation_id(), right.operation_id()],
            OperationReference::from(&left),
            OperationReference::from(&right),
            Mutation::set_project_name(text(value)),
            Mutation::set_project_name(text("initial")),
        )
        .unwrap();
        let resolution_bytes = resolution.canonical_bytes().len();
        let set = OperationSet::from_operations([left, right, resolution]).unwrap();
        let mut budget = ResourceBudget::new(unlimited());
        set.replay_effects_with_budget(&mut budget).unwrap();
        (resolution_bytes, budget.usage().working_set_bytes())
    }
    let (short_resolution_bytes, short_resolution_usage) = resolved_usage("short");
    let (long_resolution_bytes, long_resolution_usage) = resolved_usage(&long_value);
    let resolution_delta = long_resolution_bytes - short_resolution_bytes;
    // Resolve owns the selected value in conflict inspection and once again
    // in direct replay after the conflict has been cleared.
    assert_eq!(
        long_resolution_usage - short_resolution_usage,
        resolution_delta.saturating_mul(4)
    );
}

#[test]
fn conflict_budget_rejects_before_a_large_effect_copy() {
    let short = apply(2, 1, 1, 0, vec![], "short", "initial");
    let long_value = "x".repeat(1024);
    let long = apply(2, 1, 1, 0, vec![], &long_value, "initial");
    let right = apply(3, 2, 1, 0, vec![], "right", "initial");

    fn conflict_usage(
        left: Operation,
        right: Operation,
        limits: ResourceLimits,
    ) -> Result<usize, MergeError> {
        let set = OperationSet::from_operations([left, right]).unwrap();
        let mut budget = ResourceBudget::new(limits);
        let result = set.replay_effects_with_budget(&mut budget);
        match result {
            Err(MergeError::Conflicts(_)) => Ok(budget.usage().working_set_bytes()),
            Err(error) => Err(error),
            Ok(_) => panic!("the inputs must conflict"),
        }
    }

    let unlimited = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    );
    let short_usage = conflict_usage(short, right.clone(), unlimited).unwrap();
    let payload_delta = long.canonical_bytes().len()
        - apply(2, 1, 1, 0, vec![], "short", "initial")
            .canonical_bytes()
            .len();
    let expected_long_usage = short_usage + payload_delta.saturating_mul(4);
    let constrained = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        expected_long_usage - 1,
    );
    let set = OperationSet::from_operations([long, right]).unwrap();
    let mut budget = ResourceBudget::new(constrained);
    assert_eq!(
        set.replay_effects_with_budget(&mut budget),
        Err(MergeError::ResourceLimit("working_set_bytes"))
    );
    assert!(budget.usage().working_set_bytes() < expected_long_usage);
}

#[test]
fn resolution_admission_charges_repeated_effect_copies_and_cancels() {
    fn resolution_operations(value: &str) -> (Vec<Operation>, usize) {
        let left_site = site_apply(2, 1, 1, 0, vec![], 2, "left", "initial");
        let right_site = site_apply(3, 2, 1, 0, vec![], 2, "right", "initial");
        let first_resolution = Operation::try_resolve(
            op_id(4),
            project(),
            actor(3),
            device(3),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![left_site.operation_id(), right_site.operation_id()],
            OperationReference::from(&left_site),
            OperationReference::from(&right_site),
            Mutation::set_site_name(SiteId::from_bytes([2; 16]).unwrap(), text(value)),
            Mutation::set_site_name(SiteId::from_bytes([2; 16]).unwrap(), text("initial")),
        )
        .unwrap();
        let second_left = site_apply(5, 3, 1, 0, vec![], 3, "left", "initial");
        let second_right = site_apply(6, 4, 1, 0, vec![], 3, "right", "initial");
        let second_resolution = Operation::try_resolve(
            op_id(7),
            project(),
            actor(5),
            device(5),
            LogicalTimestamp::new(2).unwrap(),
            CausalDepth::new(1),
            vec![second_left.operation_id(), second_right.operation_id()],
            OperationReference::from(&second_left),
            OperationReference::from(&second_right),
            Mutation::set_site_name(SiteId::from_bytes([3; 16]).unwrap(), text(value)),
            Mutation::set_site_name(SiteId::from_bytes([3; 16]).unwrap(), text("initial")),
        )
        .unwrap();
        let resolution_bytes = first_resolution.canonical_bytes().len();
        (
            vec![
                left_site,
                right_site,
                first_resolution,
                second_left,
                second_right,
                second_resolution,
            ],
            resolution_bytes,
        )
    }

    let unlimited = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    );
    let (short_operations, short_resolution_bytes) = resolution_operations("short");
    let mut short_budget = ResourceBudget::new(unlimited);
    OperationSet::from_operations_with_budget(short_operations, &mut short_budget).unwrap();
    let short_usage = short_budget.usage().working_set_bytes();

    let (long_operations, long_resolution_bytes) = resolution_operations(&"x".repeat(1024));
    let mut long_budget = ResourceBudget::new(unlimited);
    OperationSet::from_operations_with_budget(long_operations.clone(), &mut long_budget).unwrap();
    let long_usage = long_budget.usage().working_set_bytes();
    let resolution_delta = long_resolution_bytes - short_resolution_bytes;
    // Two independent resolutions each charge their selected-value validation
    // copy. Operation bytes are a separate budget kind, so the working-set
    // differential is 2x.
    assert_eq!(long_usage - short_usage, resolution_delta.saturating_mul(2));

    let constrained = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        short_usage + resolution_delta.saturating_mul(2) - 1,
    );
    let mut constrained_budget = ResourceBudget::new(constrained);
    assert_eq!(
        OperationSet::from_operations_with_budget(long_operations, &mut constrained_budget),
        Err(OperationError::ResourceLimit("working_set_bytes"))
    );
    assert!(constrained_budget.usage().working_set_bytes() < long_usage);

    let (cancellable_operations, _) = resolution_operations(&"y".repeat(1024));
    let mut cancellation_budget = ResourceBudget::with_cancellation(
        unlimited,
        CancelAfter {
            remaining_checks: 40,
        },
    );
    assert_eq!(
        OperationSet::from_operations_with_budget(cancellable_operations, &mut cancellation_budget,),
        Err(OperationError::Cancelled)
    );
    assert!(cancellation_budget.usage().operation_ancestry_work() > 0);
}

#[test]
fn resolution_toggle_validation_charges_referenced_target_payloads() {
    fn toggle_resolution_operations(value: &str, redo: bool) -> (Vec<Operation>, usize) {
        let target = if redo {
            apply(2, 1, 1, 0, vec![], value, "initial")
        } else {
            apply(2, 1, 1, 0, vec![], "forward", value)
        };
        let other_target = apply(3, 2, 1, 0, vec![], "other", "prior");
        let target_reference = OperationReference::from(&target);
        let other_reference = OperationReference::from(&other_target);
        let left = if redo {
            Operation::try_redo(
                op_id(4),
                project(),
                actor(3),
                device(3),
                LogicalTimestamp::new(2).unwrap(),
                CausalDepth::new(1),
                vec![target.operation_id()],
                target_reference,
            )
        } else {
            Operation::try_undo(
                op_id(4),
                project(),
                actor(3),
                device(3),
                LogicalTimestamp::new(2).unwrap(),
                CausalDepth::new(1),
                vec![target.operation_id()],
                target_reference,
            )
        }
        .unwrap();
        let right = if redo {
            Operation::try_redo(
                op_id(5),
                project(),
                actor(4),
                device(4),
                LogicalTimestamp::new(2).unwrap(),
                CausalDepth::new(1),
                vec![other_target.operation_id()],
                other_reference,
            )
        } else {
            Operation::try_undo(
                op_id(5),
                project(),
                actor(4),
                device(4),
                LogicalTimestamp::new(2).unwrap(),
                CausalDepth::new(1),
                vec![other_target.operation_id()],
                other_reference,
            )
        }
        .unwrap();
        let resolution = Operation::try_resolve(
            op_id(6),
            project(),
            actor(5),
            device(5),
            LogicalTimestamp::new(3).unwrap(),
            CausalDepth::new(2),
            vec![left.operation_id(), right.operation_id()],
            OperationReference::from(&left),
            OperationReference::from(&right),
            Mutation::set_project_name(text("selected")),
            Mutation::set_project_name(text("initial")),
        )
        .unwrap();
        let target_bytes = target.canonical_bytes().len();
        (
            vec![target, other_target, left, right, resolution],
            target_bytes,
        )
    }

    let unlimited = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    );
    let (short_undo, short_undo_target_bytes) = toggle_resolution_operations("prior", false);
    let mut short_undo_budget = ResourceBudget::new(unlimited);
    OperationSet::from_operations_with_budget(short_undo, &mut short_undo_budget).unwrap();
    let short_undo_usage = short_undo_budget.usage().working_set_bytes();
    let (long_undo, long_undo_target_bytes) =
        toggle_resolution_operations(&"x".repeat(1024), false);
    let mut long_undo_budget = ResourceBudget::new(unlimited);
    OperationSet::from_operations_with_budget(long_undo, &mut long_undo_budget).unwrap();
    assert_eq!(
        long_undo_budget.usage().working_set_bytes() - short_undo_usage,
        (long_undo_target_bytes - short_undo_target_bytes).saturating_mul(3)
    );

    let (short_redo, short_redo_target_bytes) = toggle_resolution_operations("short", true);
    let mut short_redo_budget = ResourceBudget::new(unlimited);
    OperationSet::from_operations_with_budget(short_redo, &mut short_redo_budget).unwrap();
    let short_redo_usage = short_redo_budget.usage().working_set_bytes();
    let (long_redo, long_redo_target_bytes) = toggle_resolution_operations(&"y".repeat(1024), true);
    let mut long_redo_budget = ResourceBudget::new(unlimited);
    OperationSet::from_operations_with_budget(long_redo, &mut long_redo_budget).unwrap();
    assert_eq!(
        long_redo_budget.usage().working_set_bytes() - short_redo_usage,
        (long_redo_target_bytes - short_redo_target_bytes).saturating_mul(3)
    );
}

#[test]
fn duplicate_input_is_budgeted_and_cancelled_before_the_next_item() {
    let operation = root_for_test();
    let mut budget = ResourceBudget::with_cancellation(
        ResourceLimits::new(
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
        ),
        CancelAfter {
            remaining_checks: 4,
        },
    );
    assert_eq!(
        OperationSet::from_operations_with_budget(
            DuplicateThenPanic {
                operation,
                yielded: 0,
            },
            &mut budget,
        ),
        Err(OperationError::Cancelled)
    );
}

#[test]
fn repeated_duplicate_input_exhausts_work_before_validation() {
    let operation = root_for_test();
    let mut budget = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        128 * 2,
    ));
    assert_eq!(
        OperationSet::from_operations_with_budget(
            [operation.clone(), operation.clone(), operation],
            &mut budget,
        ),
        Err(OperationError::ResourceLimit("working_set_bytes"))
    );
    assert_eq!(budget.usage().working_set_bytes(), 128 * 2);
}

#[test]
fn default_input_admission_bounds_duplicate_streams() {
    let result = OperationSet::from_operations(TooManyDuplicates {
        operation: root_for_test(),
        yielded: 0,
    });
    assert_eq!(
        result,
        Err(OperationError::ResourceLimit("operation_count"))
    );
}

#[test]
fn operation_bytes_are_charged_before_set_insertion() {
    let operation = root_for_test();
    let byte_length = operation.canonical_bytes().len();
    let mut budget = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        byte_length - 1,
        usize::MAX,
        usize::MAX,
    ));
    assert_eq!(
        OperationSet::from_operations_with_budget([operation], &mut budget),
        Err(OperationError::ResourceLimit("operation_bytes"))
    );
    assert_eq!(budget.usage().operation_bytes(), 0);
}

#[test]
fn cancellation_is_preserved_at_the_operation_boundary() {
    let set = OperationSet::from_operations([root_for_test()]).unwrap();
    let mut budget = ResourceBudget::with_cancellation(
        ResourceLimits::new(
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
        ),
        CancelImmediately,
    );
    assert_eq!(
        set.replay_effects_with_budget(&mut budget),
        Err(MergeError::Cancelled)
    );
}

#[test]
fn replay_work_accounting_is_input_order_independent() {
    let operations = vec![
        apply(2, 1, 1, 0, vec![], "one", "initial"),
        site_apply(3, 2, 1, 0, vec![], 3, "two", "old"),
    ];
    let forward = OperationSet::from_operations(operations.clone()).unwrap();
    let reverse = OperationSet::from_operations(operations.into_iter().rev()).unwrap();
    let limits = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    );
    let mut first_budget = ResourceBudget::new(limits);
    let mut second_budget = ResourceBudget::new(limits);
    assert_eq!(
        forward.replay_effects_with_budget(&mut first_budget),
        reverse.replay_effects_with_budget(&mut second_budget)
    );
    assert_eq!(first_budget.usage(), second_budget.usage());
}

proptest! {
    #[test]
    fn operation_order_is_invariant_under_input_permutation(seed in any::<u8>()) {
        let operations: Vec<_> = (0..3u8)
            .map(|index| {
                let site = SiteId::from_bytes([index + 10; 16]).unwrap();
                let value = text(&format!("{}-{}", seed, index));
                Operation::try_apply(
                    op_id(index + 2),
                    project(),
                    actor(index + 1),
                    device(index + 1),
                    LogicalTimestamp::new(u64::from(index) + 1).unwrap(),
        CausalDepth::new(0),
                    vec![],
                    Mutation::set_site_name(site, value.clone()),
                    Mutation::set_site_name(site, text("old")),
                ).unwrap()
            })
            .collect();
        let forward = OperationSet::from_operations(operations.clone()).unwrap();
        let mut reverse = operations;
        reverse.reverse();
        let backward = OperationSet::from_operations(reverse).unwrap();
        let forward_ids: Vec<_> = forward.ordered().unwrap().iter().map(|op| op.operation_id()).collect();
        let backward_ids: Vec<_> = backward.ordered().unwrap().iter().map(|op| op.operation_id()).collect();
        prop_assert_eq!(forward_ids, backward_ids);
    }
}
