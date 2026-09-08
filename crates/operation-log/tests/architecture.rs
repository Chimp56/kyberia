use std::{fs, path::PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn operation_log_stays_inward_and_side_effect_free() {
    let manifest = fs::read_to_string(manifest_dir().join("Cargo.toml")).unwrap();
    let source = fs::read_to_string(manifest_dir().join("src/lib.rs")).unwrap();
    for forbidden in ["rusqlite", "project-store", "std::time", "SystemTime"] {
        assert!(
            !manifest.contains(forbidden),
            "Cargo.toml imports {forbidden}"
        );
        assert!(
            !source.contains(forbidden),
            "operation-log source imports {forbidden}"
        );
    }
    assert!(!source.contains("kyberia_packet"));
    assert!(!source.contains("kyberia_project_store"));
    assert!(!source.contains("serde_json::Value"));
    assert!(source.contains("MAX_OPERATION_CANONICAL_BYTES"));
    assert!(source.contains("OperationSet"));
    assert!(source.contains("MergeConflict"));
}
