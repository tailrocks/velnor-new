//! `fetch-reports-v1` retrieval tests.
//!
//! Declared via `#[path]` from `retrieve_reports.rs` under `cfg(test)` so
//! the retrieval module keeps the file size gate.

use std::fs;
use std::path::Path;

use super::{MAX_STAGED_REPORT_BYTES, expected_artifact_ids, retrieve_args, retrieve_reports_to};

#[test]
fn retrieve_argv_names_exact_artifact() {
    let args = retrieve_args(
        7,
        "velnor-matrix-r7-a2-m-0123456789abcdef",
        Path::new("/tmp/x"),
    )
    .expect("argv");
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
            "velnor-matrix-r7-a2-m-0123456789abcdef",
            "--dir",
            "/tmp/x"
        ]
    );
    assert!(retrieve_args(7, "velnor-matrix-*", Path::new("/tmp/x")).is_err());
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
            {"artifact_id": "velnor-matrix-r7-a2-m-0000000000000002"},
            {"artifact_id": "velnor-matrix-r7-a2-m-0000000000000001"},
        ]}
    });
    assert_eq!(
        expected_artifact_ids(&plan),
        [
            "velnor-matrix-r7-a2-m-0000000000000002",
            "velnor-matrix-r7-a2-m-0000000000000001",
        ]
    );
    assert!(expected_artifact_ids(&serde_json::json!({})).is_empty());
}

/// Write `plan.json` under a fresh `run_dir` and return the temp dir.
fn plan_dir(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let run = dir.path().join("r7-a2");
    fs::create_dir_all(&run).expect("run dir");
    fs::write(run.join("plan.json"), body).expect("plan write");
    (dir, run)
}

#[test]
fn malformed_ids_skip_before_join_and_mkdir() {
    let (_dir, run) = plan_dir(
        r#"{"matrix": {"include": [
            {"artifact_id": "../escape"},
            {"artifact_id": "velnor-matrix-*"},
            {"artifact_id": ""}
        ]}}"#,
    );
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(
        !run.join("reports").exists(),
        "no directory for malformed IDs"
    );
    assert!(
        !run.parent().expect("parent").join("escape").exists(),
        "no traversal write"
    );
}

#[test]
fn oversize_and_duplicate_key_plans_retrieve_zero() {
    let big = "x".repeat(usize::try_from(MAX_STAGED_REPORT_BYTES + 1).expect("bound fits"));
    let (_dir, run) = plan_dir(&big);
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
    let (_dir, run) = plan_dir(r#"{"matrix": {"include": []}, "matrix": {}}"#);
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}
