//! Retrieve enumeration tests: argv shape, zero-plan, dedupe.
//!
//! Declared via `#[path]` from `retrieve_reports.rs` under `cfg(test)`.

use super::*;

#[test]
fn retrieve_argv_names_exact_artifact() {
    let args = retrieve_args(7, "velnor-crate-r7-a2-crate_foo", Path::new("/tmp/x")).expect("argv");
    let text: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        text,
        [
            "run",
            "download",
            "7",
            "--name",
            "velnor-crate-r7-a2-crate_foo",
            "--dir",
            "/tmp/x"
        ]
    );
    assert!(retrieve_args(7, "velnor-crate-*", Path::new("/tmp/x")).is_err());
    assert!(retrieve_args(7, "../escape", Path::new("/tmp/x")).is_err());
}

#[test]
fn missing_plan_retrieves_zero_without_spawning() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let run = dir.path().join("r7-a2");
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}

#[test]
fn enumeration_follows_plan_order() {
    let plan = serde_json::json!({
        "matrix": {"include": [
            {"artifact_id": "velnor-crate-r7-a2-crate_b"},
            {"artifact_id": "velnor-crate-r7-a2-crate_a"},
        ]}
    });
    assert_eq!(
        expected_artifact_ids(&plan),
        ["velnor-crate-r7-a2-crate_b", "velnor-crate-r7-a2-crate_a",]
    );
    assert!(expected_artifact_ids(&serde_json::json!({})).is_empty());
}

#[test]
fn enumeration_dedupes_shared_job_artifacts() {
    let plan = serde_json::json!({
        "matrix": {"include": [
            {"artifact_id": "velnor-crate-r7-a2-crate_a"},
            {"artifact_id": "velnor-crate-r7-a2-crate_a"},
            {"artifact_id": "velnor-crate-r7-a2-plan"},
        ]}
    });
    assert_eq!(
        expected_artifact_ids(&plan),
        ["velnor-crate-r7-a2-crate_a", "velnor-crate-r7-a2-plan",]
    );
}
