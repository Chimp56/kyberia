use kyberia_desktop_lib::{
    DesktopState, IPC_SCHEMA, consume_open_grant, create_project_at, current_project,
    issue_open_grant,
};
use std::path::PathBuf;

fn retained_project(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.trash/test-runs");
    std::fs::create_dir_all(&root).expect("retained test root");
    root.join(format!(
        "integration-{name}-{}.rfatlas",
        uuid::Uuid::new_v4()
    ))
}

#[test]
fn application_create_and_query_mapping_stays_inside_versioned_boundary() {
    let mut state = DesktopState::default();
    let response = create_project_at(
        &mut state,
        retained_project("mapping"),
        "Integration project".to_owned(),
        1_800_000_000_000,
    )
    .expect("application create mapping");
    assert_eq!(response.schema, IPC_SCHEMA);
    assert_eq!(response.state, "baseline_only");
    assert_eq!(
        response
            .project
            .as_ref()
            .map(|project| project.name.as_str()),
        Some("Integration project")
    );

    let current = current_project(&state).expect("application query mapping");
    assert_eq!(
        current
            .project
            .as_ref()
            .map(|project| project.project_id.clone()),
        response
            .project
            .as_ref()
            .map(|project| project.project_id.clone())
    );
    assert!(
        current
            .capabilities
            .iter()
            .all(|capability| capability.state == "unavailable")
    );
}

#[test]
fn native_grant_is_opaque_single_use_and_root_bound() {
    let mut state = DesktopState::default();
    let path = retained_project("grant");
    std::fs::create_dir_all(&path).expect("project root");
    let selection = issue_open_grant(&mut state, path.clone()).expect("native selection adapter");
    let selection = selection.selection.expect("selection");
    assert!(!selection.grant_id.contains(path.to_str().expect("path")));
    let canonical = std::fs::canonicalize(path).expect("canonical root");
    assert_eq!(
        consume_open_grant(
            &mut state,
            &selection.grant_id,
            Some(&selection.display_name)
        )
        .expect("consume grant"),
        canonical
    );
    assert!(consume_open_grant(&mut state, &selection.grant_id, None).is_err());
}
