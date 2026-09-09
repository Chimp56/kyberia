use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn retained_directory() -> PathBuf {
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(".trash")
        .join("test-runs");
    std::fs::create_dir_all(&root).unwrap();
    let process = std::process::id();
    loop {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let candidate = root.join(format!("canonical-project-{process}-{ordinal}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot retain test directory {candidate:?}: {error}"),
        }
    }
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kyberia"))
        .args(args)
        .output()
        .unwrap()
}

fn project(
    id: kyberia_domain::identity::ProjectId,
    name: &str,
) -> kyberia_domain::project::Project {
    kyberia_domain::project::Project::new(id, kyberia_domain::identity::Text::new(name).unwrap())
}

fn create_baseline_bundle(path: &Path, name: &str) -> kyberia_domain::project::Project {
    use kyberia_domain::identity::ProjectId;
    let id = ProjectId::from_bytes([31; 16]).unwrap();
    let baseline = project(id, name);
    let mut bundle = kyberia_project_store::Bundle::create(path, id, name.into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    baseline
}

fn create_current_bundle(path: &Path) -> kyberia_project_store::MaterializationPublicationReceipt {
    use kyberia_causal_materializer::materialize;
    use kyberia_domain::identity::ProjectId;
    use kyberia_operation_log::{OperationSet, ProjectVersion};
    use kyberia_project_store::{Bundle, MaterializationPublicationOutcome};

    let id = ProjectId::from_bytes([32; 16]).unwrap();
    let baseline = project(id, "Published");
    let operations = OperationSet::empty(id);
    let materialized = materialize(&baseline, &operations).unwrap();
    let mut bundle = Bundle::create(path, id, "Published".into(), 1).unwrap();
    bundle
        .register_materialization_baseline(&baseline, 2)
        .unwrap();
    let MaterializationPublicationOutcome::Published(receipt) = bundle
        .publish_materialized_project(
            &baseline,
            &operations,
            &materialized,
            ProjectVersion::new(0),
            3,
        )
        .unwrap()
    else {
        panic!("empty publication must be new");
    };
    receipt
}

#[test]
fn new_registers_an_empty_canonical_baseline_and_query_reopens_it() {
    let root = retained_directory();
    let path = root.join("new.rfatlas");
    let created = run(&["new", path.to_str().unwrap(), "New Home"]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let manifest: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(manifest["revision"], 1);
    assert_eq!(manifest["artifacts"].as_object().unwrap().len(), 1);

    let queried = run(&["query-canonical-project", path.to_str().unwrap()]);
    assert!(
        queried.status.success(),
        "{}",
        String::from_utf8_lossy(&queried.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&queried.stdout).unwrap();
    assert_eq!(report["schema"], "kyberia.canonical-project-query/1");
    assert_eq!(report["state"], "baseline_only");
    assert_eq!(report["manifest"]["project_id"], manifest["project_id"]);
    assert_eq!(report["manifest"]["name"], "New Home");
    assert_eq!(report["project"]["id"], manifest["project_id"]);
    assert_eq!(report["project"]["name"], "New Home");
    assert_eq!(report["project"]["revision"], 0);
    assert_eq!(report["baseline"]["project_revision"], 0);
    assert!(report["publication"].is_null());
}

#[test]
fn query_distinguishes_legacy_absence_from_registered_baseline() {
    use kyberia_domain::identity::ProjectId;
    use kyberia_project_store::Bundle;

    let root = retained_directory();
    let legacy = root.join("legacy.rfatlas");
    let id = ProjectId::from_bytes([33; 16]).unwrap();
    let bundle = Bundle::create(&legacy, id, "Legacy".into(), 1).unwrap();
    drop(bundle);
    let report: serde_json::Value =
        serde_json::from_slice(&run(&["query-canonical-project", legacy.to_str().unwrap()]).stdout)
            .unwrap();
    assert_eq!(report["state"], "legacy_absent");
    assert!(report["project"].is_null());
    assert!(report["baseline"].is_null());
    assert!(report["publication"].is_null());

    let baseline_path = root.join("baseline.rfatlas");
    let baseline = create_baseline_bundle(&baseline_path, "Baseline");
    let queried = run(&["query-canonical-project", baseline_path.to_str().unwrap()]);
    assert!(
        queried.status.success(),
        "{}",
        String::from_utf8_lossy(&queried.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&queried.stdout).unwrap();
    assert_eq!(report["state"], "baseline_only");
    assert_eq!(report["project"]["id"], String::from(baseline.id()));
    assert_eq!(report["baseline"]["logical_time"], 0);
}

#[test]
fn query_reports_verified_current_publication_and_rejects_corrupt_result() {
    let root = retained_directory();
    let path = root.join("published.rfatlas");
    let receipt = create_current_bundle(&path);
    let queried = run(&["query-canonical-project", path.to_str().unwrap()]);
    assert!(
        queried.status.success(),
        "{}",
        String::from_utf8_lossy(&queried.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&queried.stdout).unwrap();
    assert_eq!(report["state"], "materialized_current");
    assert_eq!(report["project"]["name"], "Published");
    assert_eq!(report["project"]["revision"], 0);
    assert_eq!(
        report["publication"]["publication_id"],
        receipt.publication_id()
    );
    assert_eq!(report["publication"]["operation_project_revision"], 0);
    assert_eq!(report["publication"]["operation_max_causal_depth"], 0);
    assert_eq!(report["publication"]["publication_bundle_revision"], 2);

    std::fs::write(
        path.join("artifacts")
            .join(receipt.materialized_artifact_hash()),
        b"corrupt canonical result",
    )
    .unwrap();
    let failed = run(&["query-canonical-project", path.to_str().unwrap()]);
    assert_eq!(failed.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("integrity"));
}

#[test]
fn new_rejects_existing_path_without_modifying_the_existing_canonical_bundle() {
    let root = retained_directory();
    let path = root.join("protected.rfatlas");
    let first = run(&["new", path.to_str().unwrap(), "Original"]);
    assert!(first.status.success());
    let before = std::fs::read(path.join("manifest.json")).unwrap();
    let second = run(&["new", path.to_str().unwrap(), "Replacement"]);
    assert_eq!(second.status.code(), Some(2));
    assert_eq!(std::fs::read(path.join("manifest.json")).unwrap(), before);

    let invalid = root.join("invalid.rfatlas");
    let failed = run(&["new", invalid.to_str().unwrap(), "   "]);
    assert_eq!(failed.status.code(), Some(2));
    assert!(!invalid.exists());
}
