use std::path::{Path, PathBuf};

use velnor_actions_orchestrator_retrieve::retrieve_reports::{
    MAX_RETRIEVE_PLAN_BYTES, retrieve_args, retrieve_reports_to,
};

/// Write `plan.json` under a fresh `run_dir` and return both.
fn plan_dir(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let run = dir.path().join("r7-a2");
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(run.join("plan.json"), body).expect("plan write");
    (dir, run)
}

#[test]
fn missing_plan_retrieves_zero_without_spawning() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let run = dir.path().join("r7-a2");
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}

#[test]
fn unparsable_plan_retrieves_zero() {
    let (_dir, run) = plan_dir("{not json");
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}

#[test]
fn oversize_plan_retrieves_zero() {
    let big = format!(
        "{{\"matrix\":{{\"include\":[]}},\"pad\":\"{}\"}}",
        "p".repeat(usize::try_from(MAX_RETRIEVE_PLAN_BYTES).expect("bound"))
    );
    let (_dir, run) = plan_dir(&big);
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}

#[test]
fn duplicate_key_plan_retrieves_zero() {
    let (_dir, run) = plan_dir(r#"{"matrix": {"include": []}, "matrix": {}}"#);
    assert_eq!(retrieve_reports_to(7, &run), 0);
    assert!(!run.join("reports").exists(), "no downloads attempted");
}

/// Malformed IDs skip before any join or mkdir, with or without a
/// repository in the environment: unparseable-typed plans never reach
/// the spawn either way, so this pins the public path hermetically.
#[test]
fn malformed_ids_skip_without_mkdir_or_traversal() {
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
fn retrieve_argv_names_exact_artifact() {
    let args = retrieve_args(
        7,
        "velnor-matrix-r7-a2-m-0123456789abcdef",
        Path::new("/tmp/x"),
        "o/r",
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
            "/tmp/x",
            "--repo",
            "o/r"
        ]
    );
    assert!(retrieve_args(7, "velnor-matrix-*", Path::new("/tmp/x"), "o/r").is_err());
    assert!(retrieve_args(7, "../escape", Path::new("/tmp/x"), "o/r").is_err());
    assert!(
        retrieve_args(
            7,
            "velnor-matrix-r7-a2-m-0123456789abcdef",
            Path::new("/tmp/x"),
            "not-a-slug",
        )
        .is_err(),
        "a malformed repo never builds an unscoped command"
    );
}
