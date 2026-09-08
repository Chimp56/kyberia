use kyberia_domain::identity::{
    ActorDeviceId, ActorId, CalibrationId, ContentHash, FloorId, MapAssetId, OperationId,
    ProjectId, SiteId, Text,
};
use kyberia_operation_log::{
    AppendError, AppendOutcome, CausalDepth, ConflictIntent, FieldKey, ImmutableReference,
    InverseMetadata, LogicalTimestamp, MAX_OPERATION_COUNT, MAX_OPERATION_WIRE_BYTES, MergeError,
    Mutation, Operation, OperationError, OperationLog, OperationPayload, OperationReference,
    OperationSet, ProjectVersion, ToggleDirection,
};
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
        conflict.left_effect(),
        &Mutation::set_project_name(text("left"))
    );
    assert_eq!(
        conflict.right_effect(),
        &Mutation::set_project_name(text("right"))
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
        Err(OperationError::MalformedEncoding | OperationError::HashMismatch)
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
