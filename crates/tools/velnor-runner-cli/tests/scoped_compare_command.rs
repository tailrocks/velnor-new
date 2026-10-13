//! End-to-end CLI coverage for versioned scoped comparison inputs.
#![expect(
    clippy::expect_used,
    reason = "fixture construction expectations identify which validated contract input broke"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

#[path = "scoped_compare_command/fixture.rs"]
mod fixture;

use fixture::{ATTEMPT, REPOSITORY, RUN_ID, scoped_input};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-compare-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("unique temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) | Err(_) => {}
        }
    }
}

#[test]
fn scoped_compare_command_reports_bound_only_for_exact_matching_input() {
    let temp = TempDir::new();
    let input = scoped_input();
    let path = write_input(&temp, &input);
    let output = run_scoped(&path, REPOSITORY, RUN_ID, ATTEMPT.into());

    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.starts_with("BOUND_ONLY lanes=2 "), "{stdout}");
    assert!(
        stdout.contains("repository=chainargos/java-monorepo"),
        "{stdout}"
    );
    assert!(stdout.contains("run_id=424242 attempt=2"));
    assert!(!stdout.contains("PROVEN"), "{stdout}");
}

#[test]
fn scope_mismatch_and_unavailable_provider_read_fail_closed() {
    let temp = TempDir::new();
    let mut wrong_run = scoped_input();
    wrong_run["provider_read"]["value"]["workflow_run_id"] = json!(RUN_ID + 1);
    let output = run_scoped(
        &write_input(&temp, &wrong_run),
        REPOSITORY,
        RUN_ID,
        ATTEMPT.into(),
    );
    assert_not_proven(&output);

    let mut unknown_schema = scoped_input();
    unknown_schema["schema"] = json!(2);
    let output = run_scoped(
        &write_input(&temp, &unknown_schema),
        REPOSITORY,
        RUN_ID,
        ATTEMPT.into(),
    );
    assert_not_proven(&output);

    let mut unknown_provider_field = scoped_input();
    unknown_provider_field["provider_read"]["extra"] = json!(true);
    let output = run_scoped(
        &write_input(&temp, &unknown_provider_field),
        REPOSITORY,
        RUN_ID,
        ATTEMPT.into(),
    );
    assert_not_proven(&output);

    let mut unavailable = scoped_input();
    unavailable["provider_read"] = json!({
        "kind": "unavailable",
        "value": "job_page_limit_exceeded"
    });
    let output = run_scoped(
        &write_input(&temp, &unavailable),
        REPOSITORY,
        RUN_ID,
        ATTEMPT.into(),
    );
    assert_not_proven(&output);
}

#[test]
fn duplicate_provider_identity_keys_in_raw_json_fail_closed() {
    let temp = TempDir::new();
    let valid = scoped_input().to_string();
    let duplicate_run_id = valid.replacen(
        "\"workflow_run_id\":424242",
        "\"workflow_run_id\":424243,\"workflow_run_id\":424242",
        1,
    );
    assert_ne!(valid, duplicate_run_id, "fixture run identity is present");
    assert_not_proven(&run_scoped_raw(
        &temp,
        &duplicate_run_id,
        REPOSITORY,
        RUN_ID,
        ATTEMPT.into(),
    ));

    let jobs_offset = valid.find("\"jobs\":").expect("provider jobs array");
    let (before_jobs, jobs_and_rest) = valid.split_at(jobs_offset);
    let duplicate_job_id = format!(
        "{before_jobs}{}",
        jobs_and_rest.replacen("\"id\":10601", "\"id\":10600,\"id\":10601", 1)
    );
    assert_ne!(valid, duplicate_job_id, "fixture job identity is present");
    assert_not_proven(&run_scoped_raw(
        &temp,
        &duplicate_job_id,
        REPOSITORY,
        RUN_ID,
        ATTEMPT.into(),
    ));

    let provider_offset = valid.find("\"provider_read\"").expect("provider read");
    let artifacts_offset = valid[provider_offset..]
        .find("\"artifacts\":")
        .map(|offset| provider_offset + offset)
        .expect("provider artifacts array");
    let (before_artifacts, artifacts_and_rest) = valid.split_at(artifacts_offset);
    let duplicate_artifact_id = format!(
        "{before_artifacts}{}",
        artifacts_and_rest.replacen("\"id\":501", "\"id\":500,\"id\":501", 1)
    );
    assert_ne!(
        valid, duplicate_artifact_id,
        "fixture artifact identity is present"
    );
    assert_not_proven(&run_scoped_raw(
        &temp,
        &duplicate_artifact_id,
        REPOSITORY,
        RUN_ID,
        ATTEMPT.into(),
    ));
}

#[test]
fn legacy_evidence_stays_unscoped_and_input_modes_are_mutually_exclusive() {
    let temp = TempDir::new();
    let legacy = Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .args([
            "compare",
            "--repo",
            REPOSITORY,
            "--run-id",
            "424242",
            "--attempt",
            "2",
            "--evidence",
        ])
        .arg(temp.path())
        .output()
        .expect("run legacy compare");
    assert_not_proven(&legacy);

    let scoped = write_input(&temp, &scoped_input());
    let conflict = Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .args([
            "compare",
            "--repo",
            REPOSITORY,
            "--run-id",
            "424242",
            "--attempt",
            "2",
            "--evidence",
        ])
        .arg(temp.path())
        .args(["--scoped-evidence"])
        .arg(scoped)
        .output()
        .expect("run conflicting compare");
    assert!(!conflict.status.success());
}

fn assert_not_proven(output: &Output) {
    assert!(!output.status.success());
    assert_eq!(stdout(output), "NOT_PROVEN\n");
}

fn run_scoped(path: &Path, repository: &str, run_id: i64, attempt: u64) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .args(["compare", "--repo", repository, "--run-id"])
        .arg(run_id.to_string())
        .args(["--attempt"])
        .arg(attempt.to_string())
        .args(["--scoped-evidence"])
        .arg(path)
        .output()
        .expect("run scoped compare")
}

fn run_scoped_raw(
    temp: &TempDir,
    raw: &str,
    repository: &str,
    run_id: i64,
    attempt: u64,
) -> Output {
    let path = temp.path().join("raw-scoped-evidence.json");
    std::fs::write(&path, raw).expect("write raw JSON with possible duplicate keys");
    run_scoped(&path, repository, run_id, attempt)
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("UTF-8 stdout")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("UTF-8 stderr")
}

fn write_input(temp: &TempDir, input: &Value) -> PathBuf {
    let path = temp.path().join("scoped-evidence.json");
    std::fs::write(&path, input.to_string()).expect("write exact test input");
    path
}
