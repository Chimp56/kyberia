use kyberia_causal_materializer::materialize;
use kyberia_domain::{
    identity::{ProjectId, Text},
    project::Project,
};
use kyberia_operation_log::{
    CausalDepth, LogicalTimestamp, Mutation, Operation, OperationSet, ProjectVersion,
};
use kyberia_project_store::{
    Bundle, MaterializationPublicationOutcome, OpenMode, PublicationError, StoreError,
};
use kyberia_resource_budget::{CancellationHook, ResourceBudget, ResourceLimits};
use std::{cell::Cell, rc::Rc};

struct CountHook {
    polls: Rc<Cell<usize>>,
    cancel_after: Option<usize>,
}

impl CancellationHook for CountHook {
    fn is_cancelled(&mut self) -> bool {
        let polls = self.polls.get().saturating_add(1);
        self.polls.set(polls);
        self.cancel_after.is_some_and(|limit| polls > limit)
    }
}

fn retained_test_root() -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap().keep()
}

#[test]
fn registration_is_immutable_across_handles_before_first_publication() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([3; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Registered").unwrap());
    let competing = Project::new(id, Text::new("Competing").unwrap());
    let mut first = Bundle::create(&root, id, "Registered".into(), 1).unwrap();
    let mut second = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
    let identity = first
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    assert_eq!(
        second.materialization_baseline().unwrap(),
        Some(baseline.clone())
    );
    let reopened_before_publication = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(
        reopened_before_publication
            .materialization_baseline()
            .unwrap(),
        Some(baseline.clone())
    );
    assert!(
        reopened_before_publication
            .materialized_project()
            .unwrap()
            .is_none()
    );
    let manifest = std::fs::read(root.join("manifest.json")).unwrap();
    assert_eq!(
        second
            .register_materialization_baseline(&baseline, 3)
            .unwrap(),
        identity
    );
    assert_eq!(std::fs::read(root.join("manifest.json")).unwrap(), manifest);
    assert!(matches!(
        second.register_materialization_baseline(&competing, 4),
        Err(StoreError::Materialization(
            PublicationError::InputIdentityMismatch
        ))
    ));
    let operations = OperationSet::empty(id);
    let competing_result = materialize(&competing, &operations).unwrap();
    assert!(matches!(
        second.publish_materialized_project(
            &competing,
            &operations,
            &competing_result,
            ProjectVersion::new(0),
            5
        ),
        Err(StoreError::Materialization(
            PublicationError::InputIdentityMismatch
        ))
    ));
    assert!(first.materialized_project().unwrap().is_none());
    assert_eq!(std::fs::read(root.join("manifest.json")).unwrap(), manifest);
    let mut read_only = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert!(matches!(
        read_only.register_materialization_baseline(&baseline, 6),
        Err(StoreError::ReadOnly)
    ));
    assert!(read_only.verify().unwrap().failures.is_empty());
}

#[test]
fn empty_operation_publication_survives_reopen_and_exact_retry() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let baseline = Project::new(
        ProjectId::from_bytes([1; 16]).unwrap(),
        Text::new("Baseline").unwrap(),
    );
    let operations = OperationSet::empty(baseline.id());
    let result = materialize(&baseline, &operations).unwrap();
    let mut bundle = Bundle::create(&root, baseline.id(), "Baseline".into(), 1).unwrap();
    assert!(bundle.materialized_project().unwrap().is_none());
    assert!(bundle.materialization_baseline().unwrap().is_none());
    assert!(matches!(
        bundle.publish_materialized_project(
            &baseline,
            &operations,
            &result,
            ProjectVersion::new(0),
            2
        ),
        Err(StoreError::Materialization(
            PublicationError::BaselineNotRegistered
        ))
    ));
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    let outcome = bundle
        .publish_materialized_project(&baseline, &operations, &result, ProjectVersion::new(0), 2)
        .unwrap();
    let MaterializationPublicationOutcome::Published(receipt) = outcome else {
        panic!("first publication must be new")
    };
    drop(bundle);
    let read_only = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(
        read_only.materialization_baseline().unwrap(),
        Some(baseline.clone())
    );
    let loaded = read_only.materialized_project().unwrap().unwrap();
    assert_eq!(loaded.project(), &baseline);
    assert_eq!(loaded.receipt(), &receipt);
    assert!(read_only.verify().unwrap().failures.is_empty());
    drop(read_only);
    let mut reopened = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
    let retry = reopened
        .publish_materialized_project(&baseline, &operations, &result, ProjectVersion::new(0), 3)
        .unwrap();
    assert_eq!(
        retry,
        MaterializationPublicationOutcome::Duplicate(receipt.clone())
    );
    assert_eq!(
        reopened.materialized_project().unwrap().unwrap().receipt(),
        &receipt
    );
}

#[test]
fn same_project_baseline_substitution_preserves_committed_publication() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([2; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Original").unwrap());
    let replacement = Project::new(id, Text::new("Substituted").unwrap());
    let operations = OperationSet::empty(id);
    let original_result = materialize(&baseline, &operations).unwrap();
    let replacement_result = materialize(&replacement, &operations).unwrap();
    let mut bundle = Bundle::create(&root, id, "Original".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    bundle
        .publish_materialized_project(
            &baseline,
            &operations,
            &original_result,
            ProjectVersion::new(0),
            2,
        )
        .unwrap();
    let before = bundle.materialized_project().unwrap().unwrap();
    let manifest_before = std::fs::read(root.join("manifest.json")).unwrap();
    let error = bundle
        .publish_materialized_project(
            &replacement,
            &operations,
            &replacement_result,
            ProjectVersion::new(0),
            3,
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            StoreError::Materialization(PublicationError::InputIdentityMismatch)
        ),
        "unexpected error: {error:?}"
    );
    assert_eq!(bundle.materialized_project().unwrap().unwrap(), before);
    assert_eq!(
        std::fs::read(root.join("manifest.json")).unwrap(),
        manifest_before
    );
    drop(bundle);
    let reopened = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(reopened.materialized_project().unwrap().unwrap(), before);
    assert!(reopened.verify().unwrap().failures.is_empty());
}

#[test]
fn registration_retry_rejects_missing_manifest_inventory() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([4; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Inventory").unwrap());
    let mut bundle = Bundle::create(&root, id, "Inventory".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM bundle_manifest WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&body).unwrap();
    manifest["artifacts"] = serde_json::json!({});
    db.execute(
        "UPDATE bundle_manifest SET body=?1 WHERE singleton=1",
        [serde_json::to_vec(&manifest).unwrap()],
    )
    .unwrap();
    let error = bundle
        .register_materialization_baseline(&baseline, 3)
        .unwrap_err();
    assert!(
        matches!(
            error,
            StoreError::Materialization(PublicationError::Corrupt(_))
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn exact_publication_retry_after_append_preserves_current_until_fresh_publication() {
    use kyberia_domain::identity::{ActorDeviceId, ActorId, OperationId};
    use kyberia_operation_log::{CausalDepth, LogicalTimestamp, Mutation, Operation};
    let retained_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
    std::fs::create_dir_all(&retained_root).unwrap();
    let retained = tempfile::tempdir_in(&retained_root).unwrap().keep();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([5; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Before").unwrap());
    let empty = OperationSet::empty(id);
    let initial = materialize(&baseline, &empty).unwrap();
    let mut first = Bundle::create(&root, id, "Before".into(), 1).unwrap();
    first
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    let MaterializationPublicationOutcome::Published(receipt) = first
        .publish_materialized_project(&baseline, &empty, &initial, ProjectVersion::new(0), 3)
        .unwrap()
    else {
        panic!("expected publication")
    };
    let mut second = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
    let operation = Operation::try_apply(
        OperationId::from_bytes([6; 16]).unwrap(),
        id,
        ActorId::from_bytes([7; 16]).unwrap(),
        ActorDeviceId::from_bytes([8; 16]).unwrap(),
        LogicalTimestamp::new(1).unwrap(),
        CausalDepth::new(0),
        vec![],
        Mutation::set_project_name(Text::new("After").unwrap()),
        Mutation::set_project_name(Text::new("Before").unwrap()),
    )
    .unwrap();
    second
        .append_operation_if_revision(operation.clone(), Some(ProjectVersion::new(0)))
        .unwrap();
    let manifest_after_append = first.manifest().unwrap();
    assert_eq!(
        first
            .publish_materialized_project(&baseline, &empty, &initial, ProjectVersion::new(0), 4)
            .unwrap(),
        MaterializationPublicationOutcome::Duplicate(receipt.clone())
    );
    assert_eq!(first.manifest().unwrap(), manifest_after_append);
    assert_eq!(
        first.materialized_project().unwrap().unwrap().receipt(),
        &receipt
    );
    let updated_set = OperationSet::from_operations([operation]).unwrap();
    let updated = materialize(&baseline, &updated_set).unwrap();
    assert!(matches!(
        first.publish_materialized_project(
            &baseline,
            &updated_set,
            &updated,
            ProjectVersion::new(0),
            5
        ),
        Err(StoreError::Materialization(
            PublicationError::StaleOperationRevision { .. }
        ))
    ));
    assert_eq!(first.manifest().unwrap(), manifest_after_append);
    first
        .publish_materialized_project(&baseline, &updated_set, &updated, ProjectVersion::new(1), 6)
        .unwrap();
    drop(first);
    let reopened = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert_eq!(
        reopened.materialized_project().unwrap().unwrap().project(),
        updated.project()
    );
    assert!(reopened.verify().unwrap().failures.is_empty());
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute(
        "UPDATE materialized_project_state SET publication_id=?1,operation_project_revision=0,bundle_revision=(SELECT bundle_revision FROM materialized_project_publications WHERE publication_id=?1)",
        [receipt.publication_id()],
    ).unwrap();
    assert!(
        reopened
            .verify()
            .unwrap()
            .failures
            .iter()
            .any(|failure| failure
                .contains("materialization current pointer precedes published operation history")),
        "history verification must report the specific current-pointer rollback diagnostic"
    );
    db.execute(
        "UPDATE materialized_project_state SET publication_id=(SELECT publication_id FROM materialized_project_publications WHERE operation_project_revision=1),operation_project_revision=1,bundle_revision=(SELECT bundle_revision FROM materialized_project_publications WHERE operation_project_revision=1)",
        [],
    ).unwrap();
    assert!(reopened.verify().unwrap().failures.is_empty());
    db.execute(
        "UPDATE materialized_project_publications SET bundle_revision=999 WHERE publication_id=?1",
        [receipt.publication_id()],
    )
    .unwrap();
    // The current snapshot remains readable, but history verification must not
    // overlook an invalid older publication simply because its pointer moved.
    assert_eq!(
        reopened.materialized_project().unwrap().unwrap().project(),
        updated.project()
    );
    assert!(!reopened.verify().unwrap().failures.is_empty());
    assert!(
        second
            .publish_materialized_project(&baseline, &empty, &initial, ProjectVersion::new(0), 7)
            .is_err()
    );
}

#[test]
fn current_publication_rejects_tampered_identity_versions_and_bounds() {
    let mutations = [
        "UPDATE materialized_project_publications SET operation_set_identity_hash=printf('%064d',1)",
        "UPDATE materialized_project_publications SET publication_id=printf('%064d',2); UPDATE materialized_project_state SET publication_id=printf('%064d',2)",
        "PRAGMA ignore_check_constraints=ON; UPDATE materialized_project_publications SET protocol_version=0",
        "PRAGMA ignore_check_constraints=ON; UPDATE materialized_project_publications SET result_schema_version=99",
        "UPDATE materialized_project_publications SET operation_max_causal_depth=1",
        "PRAGMA ignore_check_constraints=ON; UPDATE materialized_project_publications SET operation_count=8193",
        "PRAGMA ignore_check_constraints=ON; UPDATE materialized_project_publications SET operation_project_revision=8193; UPDATE materialized_project_state SET operation_project_revision=8193",
        "UPDATE materialized_project_publications SET bundle_revision=999; UPDATE materialized_project_state SET bundle_revision=999",
    ];
    for mutation in mutations {
        let retained = tempfile::tempdir().unwrap().keep();
        let root = retained.join("project");
        let id = ProjectId::from_bytes([10; 16]).unwrap();
        let baseline = Project::new(id, Text::new("Tamper fixture").unwrap());
        let operations = OperationSet::empty(id);
        let result = materialize(&baseline, &operations).unwrap();
        let mut bundle = Bundle::create(&root, id, "Tamper fixture".into(), 1).unwrap();
        bundle
            .register_materialization_baseline(&baseline, 2)
            .unwrap();
        bundle
            .publish_materialized_project(
                &baseline,
                &operations,
                &result,
                ProjectVersion::new(0),
                3,
            )
            .unwrap();
        assert!(bundle.verify().unwrap().failures.is_empty());
        let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
        db.execute_batch(mutation).unwrap();
        assert!(
            bundle.materialized_project().is_err(),
            "accepted mutation: {mutation}"
        );
        assert!(
            !bundle.verify().unwrap().failures.is_empty(),
            "verification accepted mutation: {mutation}"
        );
    }
}

#[test]
fn verification_rejects_corrupt_standalone_baseline_registration() {
    for mutation in [
        "UPDATE materialization_baselines SET logical_time=1",
        "UPDATE materialization_baselines SET project_id=printf('%032d',9)",
        "INSERT INTO materialization_baselines SELECT printf('%064d',9),project_id,artifact_hash,artifact_bytes,protocol_version,project_revision,logical_time,committed_utc_ms FROM materialization_baselines",
    ] {
        let retained = tempfile::tempdir().unwrap().keep();
        let root = retained.join("project");
        let id = ProjectId::from_bytes([11; 16]).unwrap();
        let baseline = Project::new(id, Text::new("Standalone").unwrap());
        let mut bundle = Bundle::create(&root, id, "Standalone".into(), 1).unwrap();
        bundle
            .register_materialization_baseline(&baseline, 2)
            .unwrap();
        assert!(bundle.verify().unwrap().failures.is_empty());
        let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
        db.execute_batch(mutation).unwrap();
        assert!(bundle.materialization_baseline().is_err());
        assert!(
            !bundle.verify().unwrap().failures.is_empty(),
            "accepted corrupt standalone registration: {mutation}"
        );
    }
}

#[test]
fn legacy_bundle_without_publication_schema_preserves_unknown_state() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([12; 16]).unwrap();
    drop(Bundle::create(&root, id, "Legacy".into(), 1).unwrap());
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute_batch("DROP TABLE materialized_project_state; DROP TABLE materialized_project_publications; DROP TABLE materialization_baselines;").unwrap();
    drop(db);
    let read_only = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    assert!(read_only.verify().unwrap().failures.is_empty());
    assert!(read_only.materialized_project().unwrap().is_none());
    assert!(read_only.materialization_baseline().unwrap().is_none());
    drop(read_only);
    let migrated = Bundle::open(&root, OpenMode::ReadWrite).unwrap();
    assert!(migrated.materialized_project().unwrap().is_none());
    assert!(migrated.verify().unwrap().failures.is_empty());
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM materialization_baselines",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn corrupted_baseline_or_result_bytes_fail_reads_and_retry() {
    for corrupt_baseline in [true, false] {
        let retained = tempfile::tempdir().unwrap().keep();
        let root = retained.join("project");
        let id = ProjectId::from_bytes([13; 16]).unwrap();
        let baseline = Project::new(id, Text::new("Artifact integrity").unwrap());
        let operations = OperationSet::empty(id);
        let result = materialize(&baseline, &operations).unwrap();
        let mut bundle = Bundle::create(&root, id, "Artifact integrity".into(), 1).unwrap();
        bundle
            .register_materialization_baseline(&baseline, 2)
            .unwrap();
        let MaterializationPublicationOutcome::Published(receipt) = bundle
            .publish_materialized_project(
                &baseline,
                &operations,
                &result,
                ProjectVersion::new(0),
                3,
            )
            .unwrap()
        else {
            panic!("expected first publication")
        };
        let hash = if corrupt_baseline {
            receipt.baseline_artifact_hash()
        } else {
            receipt.materialized_artifact_hash()
        };
        let path = root.join("artifacts").join(hash);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        let manifest = bundle.manifest().unwrap();
        assert!(bundle.materialized_project().is_err());
        assert!(!bundle.verify().unwrap().failures.is_empty());
        assert!(
            bundle
                .publish_materialized_project(
                    &baseline,
                    &operations,
                    &result,
                    ProjectVersion::new(0),
                    4
                )
                .is_err()
        );
        assert_eq!(bundle.manifest().unwrap(), manifest);
        drop(bundle);
        let reopened = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
        assert!(reopened.materialized_project().is_err());
        assert!(!reopened.verify().unwrap().failures.is_empty());
    }
}

#[test]
fn consistent_checksums_cannot_replace_replayed_project_fields() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([14; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Original").unwrap());
    let operations = OperationSet::empty(id);
    let result = materialize(&baseline, &operations).unwrap();
    let mut bundle = Bundle::create(&root, id, "Original".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    bundle
        .publish_materialized_project(&baseline, &operations, &result, ProjectVersion::new(0), 3)
        .unwrap();
    let receipt = bundle
        .materialized_project()
        .unwrap()
        .unwrap()
        .receipt()
        .clone();
    let forged = Project::new(id, Text::new("Replaced").unwrap());
    let bytes = serde_json::to_vec(&forged).unwrap();
    let hash = kyberia_project_store::content_hash(&bytes);
    std::fs::write(root.join("artifacts").join(&hash), &bytes).unwrap();
    let mut manifest = bundle.manifest().unwrap();
    let mut entry = manifest.artifacts[receipt.materialized_artifact_hash()].clone();
    entry.bytes = bytes.len() as u64;
    manifest.artifacts.insert(hash.clone(), entry);
    let db = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    db.execute("UPDATE materialized_project_publications SET materialized_artifact_hash=?1,materialized_artifact_bytes=?2", rusqlite::params![hash, bytes.len() as i64]).unwrap();
    db.execute(
        "UPDATE bundle_manifest SET body=?1",
        [serde_json::to_vec(&manifest).unwrap()],
    )
    .unwrap();
    bundle.recover_manifest().unwrap();
    assert!(
        bundle.materialized_project().is_err(),
        "accepted a valid but unrelated result project"
    );
    assert!(!bundle.verify().unwrap().failures.is_empty());
}

#[test]
fn replaced_artifact_larger_than_declaration_is_rejected_before_reading_it() {
    let retained = retained_test_root();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([23; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Artifact size").unwrap());
    let operations = OperationSet::empty(id);
    let result = materialize(&baseline, &operations).unwrap();
    let mut bundle = Bundle::create(&root, id, "Artifact size".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    bundle
        .publish_materialized_project(&baseline, &operations, &result, ProjectVersion::new(0), 3)
        .unwrap();

    let (artifact_hash, declared_bytes) = bundle
        .manifest()
        .unwrap()
        .artifacts
        .iter()
        .find(|(_, entry)| entry.kind == kyberia_project_store::ArtifactKind::MaterializedProject)
        .map(|(hash, entry)| (hash.clone(), entry.bytes))
        .unwrap();
    let artifact_path = root.join("artifacts").join(&artifact_hash);
    assert_eq!(
        std::fs::metadata(&artifact_path).unwrap().len(),
        declared_bytes
    );
    std::fs::write(
        &artifact_path,
        vec![0_u8; usize::try_from(declared_bytes).unwrap() + 4_096],
    )
    .unwrap();

    let error = bundle.materialized_project().unwrap_err();
    assert!(matches!(
        error,
        StoreError::Corrupt(message) if message.contains("artifact checksum/length mismatch")
    ));
    let mut budget = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    ));
    let verification_error = bundle
        .verify_materialized_project_publication_with_budget(&mut budget)
        .unwrap_err();
    assert!(matches!(
        verification_error,
        StoreError::Corrupt(message) if message.contains("artifact checksum/length mismatch")
    ));
}

#[test]
fn wrong_project_and_read_only_publication_preserve_committed_state() {
    let retained = tempfile::tempdir().unwrap().keep();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([16; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Local").unwrap());
    let foreign = Project::new(
        ProjectId::from_bytes([17; 16]).unwrap(),
        Text::new("Foreign").unwrap(),
    );
    let mut bundle = Bundle::create(&root, id, "Local".into(), 1).unwrap();
    let before = bundle.manifest().unwrap();
    assert!(matches!(
        bundle.register_materialization_baseline(&foreign, 2),
        Err(StoreError::Materialization(PublicationError::WrongProject))
    ));
    assert_eq!(bundle.manifest().unwrap(), before);
    assert!(bundle.materialization_baseline().unwrap().is_none());
    bundle
        .register_materialization_baseline(&baseline, 3)
        .unwrap();
    let operations = OperationSet::empty(id);
    let result = materialize(&baseline, &operations).unwrap();
    let mut read_only = Bundle::open(&root, OpenMode::ReadOnly).unwrap();
    let registered = read_only.manifest().unwrap();
    let foreign_result = materialize(&foreign, &OperationSet::empty(foreign.id())).unwrap();
    assert!(matches!(
        bundle.publish_materialized_project(
            &baseline,
            &operations,
            &foreign_result,
            ProjectVersion::new(0),
            4
        ),
        Err(StoreError::Materialization(
            PublicationError::InputIdentityMismatch
        ))
    ));
    assert_eq!(bundle.manifest().unwrap(), registered);
    assert!(matches!(
        read_only.publish_materialized_project(
            &baseline,
            &operations,
            &result,
            ProjectVersion::new(0),
            4
        ),
        Err(StoreError::ReadOnly)
    ));
    assert_eq!(read_only.manifest().unwrap(), registered);
    assert!(read_only.materialized_project().unwrap().is_none());
    assert!(read_only.verify().unwrap().failures.is_empty());
}

#[test]
fn publication_verification_uses_one_cumulative_budget_for_all_history() {
    let retained = retained_test_root();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([18; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Budget history").unwrap());
    let mut bundle = Bundle::create(&root, id, "Budget history".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    let empty = OperationSet::empty(id);
    let initial = materialize(&baseline, &empty).unwrap();
    bundle
        .publish_materialized_project(&baseline, &empty, &initial, ProjectVersion::new(0), 3)
        .unwrap();

    let make_operation = |number: u8, before: &str, after: &str, parents: Vec<_>, depth: u64| {
        Operation::try_apply(
            kyberia_domain::identity::OperationId::from_bytes([number; 16]).unwrap(),
            id,
            kyberia_domain::identity::ActorId::from_bytes([number + 20; 16]).unwrap(),
            kyberia_domain::identity::ActorDeviceId::from_bytes([number + 40; 16]).unwrap(),
            LogicalTimestamp::new(number as u64).unwrap(),
            CausalDepth::new(depth),
            parents,
            Mutation::set_project_name(Text::new(after).unwrap()),
            Mutation::set_project_name(Text::new(before).unwrap()),
        )
        .unwrap()
    };
    let first = make_operation(1, "Budget history", "Budget one", vec![], 0);
    bundle
        .append_operation_if_revision(first.clone(), Some(ProjectVersion::new(0)))
        .unwrap();
    let first_set = OperationSet::from_operations([first.clone()]).unwrap();
    let first_project = materialize(&baseline, &first_set).unwrap();
    bundle
        .publish_materialized_project(
            &baseline,
            &first_set,
            &first_project,
            ProjectVersion::new(1),
            4,
        )
        .unwrap();
    let second = make_operation(2, "Budget one", "Budget two", vec![first.operation_id()], 1);
    bundle
        .append_operation_if_revision(second.clone(), Some(ProjectVersion::new(1)))
        .unwrap();
    let second_set = OperationSet::from_operations([first, second]).unwrap();
    let second_project = materialize(&baseline, &second_set).unwrap();
    bundle
        .publish_materialized_project(
            &baseline,
            &second_set,
            &second_project,
            ProjectVersion::new(2),
            5,
        )
        .unwrap();

    let polls = Rc::new(Cell::new(0));
    let mut unrestricted = ResourceBudget::with_cancellation(
        ResourceLimits::new(
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
        ),
        CountHook {
            polls: Rc::clone(&polls),
            cancel_after: None,
        },
    );
    bundle
        .verify_materialized_project_publication_with_budget(&mut unrestricted)
        .unwrap();
    let total_project_copy = unrestricted.usage().project_copy_bytes();
    assert!(
        total_project_copy > 0,
        "history verification must charge replay copies"
    );

    let limits = ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        total_project_copy - 1,
        usize::MAX,
    );
    let mut exhausted = ResourceBudget::new(limits);
    let error = bundle
        .verify_materialized_project_publication_with_budget(&mut exhausted)
        .unwrap_err();
    assert!(matches!(
        error,
        StoreError::Materialization(PublicationError::ResourceLimit("causal_copy_bytes"))
    ));
    assert!(exhausted.usage().project_copy_bytes() > 0);
    assert!(exhausted.usage().project_copy_bytes() < total_project_copy);

    let mut repeated = ResourceBudget::new(limits);
    let repeated_error = bundle
        .verify_materialized_project_publication_with_budget(&mut repeated)
        .unwrap_err();
    assert_eq!(repeated_error.to_string(), error.to_string());
    assert_eq!(repeated.usage(), exhausted.usage());

    let cancel_after = polls.get() / 2;
    let cancel_polls = Rc::new(Cell::new(0));
    let mut cancelled = ResourceBudget::with_cancellation(
        ResourceLimits::new(
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
            usize::MAX,
        ),
        CountHook {
            polls: Rc::clone(&cancel_polls),
            cancel_after: Some(cancel_after),
        },
    );
    let cancellation = bundle
        .verify_materialized_project_publication_with_budget(&mut cancelled)
        .unwrap_err();
    assert!(matches!(cancellation, StoreError::Cancelled));
    assert!(
        cancelled.usage().project_copy_bytes() > 0,
        "cancellation must be observable after replay begins"
    );
    assert!(cancel_polls.get() > cancel_after);
}

#[test]
fn publication_verification_rejects_before_any_work_when_budget_is_empty() {
    let retained = retained_test_root();
    let root = retained.join("project");
    let id = ProjectId::from_bytes([19; 16]).unwrap();
    let baseline = Project::new(id, Text::new("Empty budget").unwrap());
    let empty = OperationSet::empty(id);
    let result = materialize(&baseline, &empty).unwrap();
    let mut bundle = Bundle::create(&root, id, "Empty budget".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    bundle
        .publish_materialized_project(&baseline, &empty, &result, ProjectVersion::new(0), 3)
        .unwrap();
    let limits = ResourceLimits::new(usize::MAX, usize::MAX, usize::MAX, usize::MAX, 0, 0);
    let mut budget = ResourceBudget::new(limits);
    let error = bundle
        .verify_materialized_project_publication_with_budget(&mut budget)
        .unwrap_err();
    assert!(matches!(
        error,
        StoreError::Materialization(PublicationError::ResourceLimit("working_set_bytes"))
    ));
    assert_eq!(budget.usage().working_set_bytes(), 0);

    let baseline_bytes = bundle
        .manifest()
        .unwrap()
        .artifacts
        .values()
        .find(|entry| entry.kind == kyberia_project_store::ArtifactKind::MaterializationBaseline)
        .unwrap()
        .bytes as usize;
    let database = rusqlite::Connection::open(root.join("project.sqlite")).unwrap();
    let manifest_bytes = database
        .query_row("SELECT length(body) FROM bundle_manifest", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap() as usize;
    let baseline_text_bytes = database
        .query_row(
            "SELECT length(CAST(baseline_identity_hash AS BLOB)) + length(CAST(project_id AS BLOB)) + length(CAST(artifact_hash AS BLOB)) FROM materialization_baselines",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap() as usize;
    let predecode = manifest_bytes
        .saturating_mul(4)
        .saturating_add(128)
        .saturating_add(256)
        .saturating_add(baseline_text_bytes)
        .saturating_add(baseline_bytes)
        .saturating_add(128);
    let mut decoder_budget = ResourceBudget::new(ResourceLimits::new(
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        usize::MAX,
        predecode
            .saturating_add(baseline_bytes.saturating_mul(4))
            .saturating_add(127),
    ));
    let decoder_error = bundle
        .verify_materialized_project_publication_with_budget(&mut decoder_budget)
        .unwrap_err();
    assert!(matches!(
        decoder_error,
        StoreError::Materialization(PublicationError::ResourceLimit("working_set_bytes"))
    ));
    assert_eq!(decoder_budget.usage().working_set_bytes(), predecode);
}
