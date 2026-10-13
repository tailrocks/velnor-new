//! P13 end-to-end negative pipeline: every stage fails closed.
//!
//! One test per pipeline stage — config, discovery, plan, IR, workflow,
//! reports, Required — each driving the real chain up to the failing stage
//! plus a positive control proving the chain itself is green. New file by
//! policy: existing test files are untouched.

use std::fs;

use velnor_actions_contract_workflow::{FinalStatus, MatrixReport, MatrixStatus, TaskStatus};
use velnor_actions_orchestrator::{
    GenerateOptions, OrchestratorError, generate, merge_internal, merge_passed, plan_internal,
    prepare, publish_final_report,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, passing_reports,
    plan_for_source_change,
};
use crate::impl_merge::{merge_request, success_jobs};
use crate::impl_perf_p13::perf_fixtures_p13::workspace_repo;

/// Missing `.velnor/config.toml` fails `prepare` before any discovery.
#[test]
fn negative_config_missing_fails_prepare() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::remove_file(repo.path().join(".velnor/config.toml"))?;
    let err = err_of(prepare(repo.path()).map(|_| ()), "missing config")?;
    assert!(
        matches!(err, OrchestratorError::ConfigMissing { .. }),
        "got {err:?}"
    );
    Ok(())
}

/// Unparseable config fails `prepare` with a config error, never a guess.
#[test]
fn negative_config_invalid_fails_prepare() -> TestResult {
    let repo = make_repo("[[[ not toml\n")?;
    let err = err_of(prepare(repo.path()).map(|_| ()), "invalid config")?;
    assert!(
        matches!(err, OrchestratorError::Config { .. }),
        "got {err:?}"
    );
    Ok(())
}

/// A malformed member manifest fails discovery naming detection.
#[test]
fn negative_discovery_member_malformed_fails_plan() -> TestResult {
    let repo = workspace_repo(2)?;
    fs::write(repo.path().join("crates/c001/Cargo.toml"), "[[[ not toml\n")?;
    let err = err_of(prepare(repo.path()).map(|_| ()), "malformed member")?;
    assert!(
        matches!(err, OrchestratorError::Detection { .. }),
        "got {err:?}"
    );
    Ok(())
}

/// A plan request with an empty head fails instead of planning nothing.
///
/// Unknown or malformed bases are deliberately NOT errors: selection
/// broadens fail-open (`comparison_unavailable:*:all_changed`). Only an
/// absent head is rejected here.
#[test]
fn negative_plan_empty_head_fails() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let base = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": "",
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let err = err_of(
        plan_internal(&request.to_string()).map(|_| ()),
        "empty head",
    )?;
    assert!(
        err.to_string().contains("empty_head"),
        "diagnostic names the missing head: {err}"
    );
    Ok(())
}

/// Dropping an obligation from the plan IR breaks contract validation.
#[test]
fn negative_ir_tampered_plan_rejected() -> TestResult {
    let (_repo, mut plan) = plan_for_source_change()?;
    assert!(!plan.obligations.is_empty(), "fixture selects work");
    plan.obligations.pop();
    let err = plan.validate().err().ok_or_else(|| {
        Box::new(std::io::Error::other("tampered plan validated")) as Box<dyn std::error::Error>
    })?;
    assert!(!err.to_string().is_empty(), "diagnostic present");
    Ok(())
}

/// `generate` fails closed when the output dir cannot be created.
#[test]
fn negative_workflow_unwritable_dir_fails_generate() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let out = tempfile::TempDir::new()?;
    let blocker = out.path().join("blocker");
    fs::write(&blocker, "not a directory\n")?;
    let opts = GenerateOptions {
        output_dir: Some(blocker.join("tree")),
    };
    let err = err_of(generate(&prep, &opts).map(|_| ()), "unwritable output")?;
    assert!(matches!(err, OrchestratorError::Io { .. }), "got {err:?}");
    Ok(())
}

/// A missing matrix leg merges red with the leg counted not-run.
#[test]
fn negative_reports_missing_leg_fails_required() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut reports = passing_reports(&plan)?;
    assert!(reports.len() > 1, "fixture needs several legs");
    reports.pop();
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let response = merge_internal(&request.to_string())?;
    let final_report: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&response)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::NotRun);
    assert!(!merge_passed(&response)?, "missing leg never passes");
    assert!(final_report.counts.not_run > 0, "missing leg counted");
    assert_eq!(
        final_report.counts.executed as usize,
        reports.len(),
        "present legs still prove their work"
    );
    Ok(())
}

/// Flip one report's single task to failed, keeping counts coherent.
fn fail_task(report: &mut MatrixReport) -> TestResult {
    let task = report.tasks.first_mut().ok_or("report without tasks")?;
    task.status = TaskStatus::Failed;
    task.exit_code = 1;
    report.status = MatrixStatus::Failed;
    report.reused = 0;
    report.executed = 0;
    report.empty_partition = 0;
    report.not_selected = 0;
    report.failed = 1;
    report.cancelled = 0;
    report.validate()?;
    Ok(())
}

/// A failed leg publishes a red verdict: `merge_passed` stays false.
#[test]
fn negative_required_failed_leg_reports_red() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut reports = passing_reports(&plan)?;
    let first = reports.first_mut().ok_or("no reports")?;
    fail_task(first)?;
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let response = merge_internal(&request.to_string())?;
    assert!(!merge_passed(&response)?, "failed merge never passes");
    let dir = tempfile::TempDir::new()?;
    let artifact = publish_final_report(&response, dir.path())?;
    let published = fs::read_to_string(artifact.join("final-report.json"))?;
    assert!(!merge_passed(&published)?, "published verdict stays red");
    Ok(())
}

/// Positive control: the unbroken chain merges green and publishes passed.
#[test]
fn pipeline_positive_control_all_green() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let response = merge_internal(&request.to_string())?;
    assert!(merge_passed(&response)?, "green chain passes");
    let dir = tempfile::TempDir::new()?;
    let artifact = publish_final_report(&response, dir.path())?;
    assert!(
        artifact.join("final-report.json").is_file(),
        "verdict file published"
    );
    Ok(())
}
