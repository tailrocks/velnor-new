//! Shared fixtures for internal integration tests, copied from the hub.
//!
//! Subset of the hub `impl_common` used by the colocated
//! trust/cover/envelope suites; the hub keeps its own copy for
//! the remaining suites.

use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use tempfile::TempDir;
use velnor_actions_contract_workflow::{MatrixReport, Plan};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_internal::internal::plan_internal;

/// Test error shortcut.
pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Canonical-schema fixture for positive consumer-generation tests.
///
/// Its placeholder source and digests are serialization inputs, not
/// release provenance or qualification evidence.
pub(crate) fn fixture_manifest_json() -> String {
    include_str!("../../../../fixtures/consumer-release-manifest.json").to_owned()
}

/// Build a git fixture: config plus one root crate (uncommitted).
pub(crate) fn make_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Minimal valid config with an explicit branch (no git branch lookup).
pub(crate) fn config_with_branch() -> &'static str {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n"
}

/// Run git with inherited failure context.
pub(crate) fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}

/// Single git stdout line.
pub(crate) fn git_line(args: &[&str], cwd: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .output()?;
    assert!(output.status.success(), "git {args:?} failed");
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Unwrap the error side or fail the test.
pub(crate) fn err_of<T>(
    result: Result<T, OrchestratorError>,
    what: &str,
) -> Result<OrchestratorError, Box<dyn std::error::Error>> {
    result.err().ok_or_else(|| {
        Box::new(std::io::Error::other(format!("{what}: expected error")))
            as Box<dyn std::error::Error>
    })
}

/// Plan for a two-commit repo whose second commit touches crate sources.
pub(crate) fn plan_for_source_change() -> Result<(TempDir, Plan), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(value["schema"], 1);
    assert_eq!(
        value["matrix"], value["plan"]["matrix"],
        "matrix byte-agreement"
    );
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((repo, plan))
}

/// One passing matrix report per plan entry.
pub(crate) fn passing_reports(
    plan: &Plan,
) -> Result<Vec<MatrixReport>, Box<dyn std::error::Error>> {
    let mut reports: Vec<MatrixReport> = Vec::new();
    for entry in &plan.matrix.include {
        let obligation = plan
            .obligations
            .iter()
            .find(|ob| ob.task_id == entry.task_id)
            .ok_or_else(|| std::io::Error::other("missing obligation"))?;
        let task_report_id = velnor_actions_contract::task_report_id_for_task(
            "local",
            &entry.matrix_key,
            &obligation.task_digest,
        )?;
        reports.push(MatrixReport {
            schema: 1,
            report_id: entry.report_id.clone(),
            run_key: "local".to_owned(),
            matrix_id: entry.id.clone(),
            matrix_key: entry.matrix_key.clone(),
            status: velnor_actions_contract_workflow::MatrixStatus::Passed,
            expected_task_ids: vec![entry.task_id.clone()],
            task_report_ids: vec![task_report_id.clone()],
            tasks: vec![velnor_actions_contract_workflow::MatrixTaskEntry {
                task_report_id,
                task_id: entry.task_id.clone(),
                status: velnor_actions_contract_workflow::TaskStatus::Executed,
                exit_code: 0,
            }],
            selected: 1,
            reused: 0,
            executed: 1,
            empty_partition: 0,
            not_selected: 0,
            failed: 0,
            cancelled: 0,
        });
    }
    for report in &reports {
        report.validate()?;
    }
    Ok(reports)
}
