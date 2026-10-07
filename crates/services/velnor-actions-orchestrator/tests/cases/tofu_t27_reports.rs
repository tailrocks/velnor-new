//! T27 tofu report-integrity cases: hostile report sets fail Required.
//!
//! Every tamper dimension the neutral partition owns — duplicate,
//! unknown, forged-digest, stale-run, and wrong-source reports —
//! fails a tofu Required verdict closed, mirroring the rust pins:
//! duplicates and stale runs are `not_run`, unknown and forged
//! bodies are `planning_failed`.
use std::fs;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::{
    matrix_id_for_task_group, matrix_key_for_id, report_id_for_matrix, task_report_id_for_task,
};
use velnor_actions_contract_workflow::{FinalStatus, MatrixReport, MatrixStatus, Plan, TaskStatus};
use velnor_actions_orchestrator::plan_internal;

use crate::cases::orch_core::{merge, merge_request, set_task, success_jobs};
use crate::support::{
    TestResult, git, git_line, install_fixture_release_manifest, passing_reports,
};

/// Git-initialized pure-tofu repo: `config` plus `files`, no Cargo.
fn make_pure_tofu_repo(
    config: &str,
    files: &[(&str, &str)],
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    install_fixture_release_manifest(root)?;
    for (relative, content) in files {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
    }
    Ok(dir)
}

/// Two-root tofu config over `stacks/a` and `stacks/b`.
fn two_root_config() -> String {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\", \"stacks/b\"]\n"
        .to_owned()
}

fn two_root_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("stacks/a/main.tf", "variable \"a\" {}\n"),
        ("stacks/b/main.tf", "variable \"b\" {}\n"),
    ]
}

/// Plan for a two-commit pure-tofu repo whose second commit rewrites `file`.
fn plan_for_tofu_change(extra: &str) -> Result<(TempDir, Plan), Box<dyn std::error::Error>> {
    let dir = make_pure_tofu_repo(&two_root_config(), &two_root_files())?;
    let root = dir.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    fs::write(root.join(extra), "variable \"bump\" {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((dir, plan))
}

/// Merge one tofu plan with explicit reports and required jobs.
fn merge_reports(
    plan: &Plan,
    reports: &serde_json::Value,
    jobs: &serde_json::Value,
) -> Result<velnor_actions_contract_workflow::FinalReport, Box<dyn std::error::Error>> {
    let plan_value = serde_json::to_value(plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    merge(&merge_request(&plan_value, &matrix, reports, jobs))
}

/// A duplicated tofu leg is `not_run`, never success.
#[test]
fn tofu_duplicate_report_fails_required_not_run() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let mut reports = passing_reports(&plan)?;
    reports.push(reports[0].clone());
    let report = merge_reports(&plan, &serde_json::to_value(&reports)?, &success_jobs())?;
    assert_eq!(report.status, FinalStatus::NotRun);
    assert!(
        report.miss_reasons.contains(&"cache_corrupt".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

/// A structurally valid report outside the plan corrupts the set.
#[test]
fn tofu_unknown_report_fails_required_planning_failed() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let mut reports = passing_reports(&plan)?;
    let ghost_task = "stack/tofu/stacks/a/fmt/zz";
    let matrix_id = matrix_id_for_task_group("tofu", ghost_task)?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    let digest = velnor_actions_contract::digest_b3(b"t27-ghost");
    let task_report_id = task_report_id_for_task("local", &matrix_key, &digest)?;
    let ghost = MatrixReport {
        schema: 1,
        report_id: report_id_for_matrix("local", &matrix_key)?,
        run_key: "local".to_owned(),
        matrix_id,
        matrix_key,
        status: MatrixStatus::Passed,
        expected_task_ids: vec![ghost_task.to_owned()],
        task_report_ids: vec![task_report_id.clone()],
        tasks: vec![velnor_actions_contract_workflow::MatrixTaskEntry {
            task_report_id,
            task_id: ghost_task.to_owned(),
            status: TaskStatus::Executed,
            exit_code: 0,
        }],
        selected: 1,
        reused: 0,
        executed: 1,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    };
    ghost.validate()?;
    reports.push(ghost);
    let report = merge_reports(&plan, &serde_json::to_value(&reports)?, &success_jobs())?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"cache_corrupt".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

/// A task-report ID bound to the wrong digest fails planning.
#[test]
fn tofu_forged_task_digest_fails_required_planning_failed() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let mut reports = passing_reports(&plan)?;
    let wrong_digest = velnor_actions_contract::digest_b3(b"tampered-tofu-task");
    let forged = task_report_id_for_task("local", &reports[0].matrix_key, &wrong_digest)?;
    reports[0].tasks[0].task_report_id = forged.clone();
    reports[0].task_report_ids = vec![forged];
    let report = merge_reports(&plan, &serde_json::to_value(&reports)?, &success_jobs())?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"cache_corrupt".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

/// A tofu leg bound to another run is stale evidence, never success.
#[test]
fn tofu_stale_run_report_fails_required_not_run() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let mut reports = passing_reports(&plan)?;
    reports[0].run_key = "r1-a1".to_owned();
    let report = merge_reports(&plan, &serde_json::to_value(&reports)?, &success_jobs())?;
    assert_eq!(report.status, FinalStatus::NotRun);
    assert!(
        report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

/// A task file from the wrong event source fails trust coherence.
#[test]
fn tofu_wrong_source_task_file_fails_required_planning_failed() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let mut request = merge_request(
        &plan_value,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    request["task_reports"][0]["event"] = json!("push");
    let report = merge(&request)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

/// A skipped tofu validator is `not_run`, never success.
#[test]
fn tofu_skipped_validator_fails_required_not_run() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let reports = passing_reports(&plan)?;
    let jobs = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
        {"job_id": "tofu-stacks-a", "conclusion": "skipped"},
    ]);
    let plan_value = serde_json::to_value(&plan)?;
    let mut request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &jobs,
    );
    request["required_job_ids"] = json!(["plan", "actionlint", "tofu-stacks-a"]);
    assert_eq!(merge(&request)?.status, FinalStatus::NotRun);
    Ok(())
}

/// A cancelled tofu validator cancels Required, never success.
#[test]
fn tofu_cancelled_validator_fails_required_cancelled() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let reports = passing_reports(&plan)?;
    let jobs = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
        {"job_id": "tofu-stacks-a", "conclusion": "cancelled"},
    ]);
    let plan_value = serde_json::to_value(&plan)?;
    let mut request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &jobs,
    );
    request["required_job_ids"] = json!(["plan", "actionlint", "tofu-stacks-a"]);
    assert_eq!(merge(&request)?.status, FinalStatus::Cancelled);
    Ok(())
}

/// A cancelled tofu task cancels Required, never success.
#[test]
fn tofu_cancelled_task_fails_required_cancelled() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let mut reports = passing_reports(&plan)?;
    set_task(
        &mut reports[0],
        TaskStatus::Cancelled,
        MatrixStatus::Cancelled,
    )?;
    let report = merge_reports(&plan, &serde_json::to_value(&reports)?, &success_jobs())?;
    assert_eq!(report.status, FinalStatus::Cancelled);
    Ok(())
}
