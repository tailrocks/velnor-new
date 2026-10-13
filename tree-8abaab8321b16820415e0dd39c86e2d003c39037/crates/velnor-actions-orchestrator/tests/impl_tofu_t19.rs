//! T19 tofu exact-set end to end: identity, inventory, merge ride-through.
//!
//! Tofu obligations flow through plan dispatch, IR, reports, and the
//! Required exact-set check with the same fail-closed posture as rust:
//! `tofu-` job identity, union package inventory, and neutral merge,
//! cover, report, and retrieve links proven with tofu cases.
use std::fs;

use serde_json::json;
use tempfile::TempDir;
use velnor_actions_contract::{FinalStatus, JobConclusion, Plan};
use velnor_actions_orchestrator::{
    assemble_merge_request, finalized_jobs, merge_internal, plan_internal, prepare,
};

use super::impl_common::{
    TestResult, git, git_line, install_fixture_release_manifest, passing_reports, plan_for,
};
use super::impl_orch_core::{merge, merge_request, success_jobs};

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

/// Tofu jobs count on their own plan line, never under the rust label.
#[test]
fn plan_counts_tofu_roots_on_their_own_line() -> TestResult {
    let dir = make_pure_tofu_repo(&two_root_config(), &two_root_files())?;
    let text = plan_for(&prepare(dir.path())?)?;
    assert!(
        text.contains("2 OpenTofu root jobs"),
        "tofu count line:\n{text}"
    );
    assert!(!text.contains("Rust crate job"), "no rust label:\n{text}");
    Ok(())
}

/// The plan-job format slot stays rust-workspace-scoped (G8 inherit).
#[test]
fn plan_job_carries_no_tofu_format_scope() -> TestResult {
    let dir = make_pure_tofu_repo(&two_root_config(), &two_root_files())?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let plan = jobs.get("plan").ok_or("plan job")?;
    assert!(
        plan.steps.iter().all(|step| step.name != "Format"),
        "no plan Format step: {:?}",
        plan.steps.iter().map(|step| &step.name).collect::<Vec<_>>()
    );
    Ok(())
}

/// Tofu roots join the package inventory keyed by root key (G4).
#[test]
fn tofu_roots_join_the_package_inventory() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let rows: Vec<(&str, bool, Vec<&str>)> = plan
        .packages
        .iter()
        .map(|row| {
            (
                row.package_id.as_str(),
                row.selected,
                row.reasons.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    // The plan carries the full universe: both roots selected, each
    // with its triple listed; changed-work marks obligations, not rows.
    assert!(
        rows.contains(&("stacks/a", true, vec!["selected"])),
        "affected root selected: {rows:?}"
    );
    assert!(
        rows.contains(&("stacks/b", true, vec!["selected"])),
        "full universe keeps every root: {rows:?}"
    );
    let ids: Vec<&str> = plan
        .packages
        .iter()
        .map(|row| row.package_id.as_str())
        .collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] <= pair[1]),
        "union inventory stays sorted: {ids:?}"
    );
    let tasks: Vec<&str> = plan
        .packages
        .iter()
        .find(|row| row.package_id == "stacks/a")
        .ok_or("stacks/a row")?
        .tasks
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(tasks.len(), 3, "affected triple listed: {tasks:?}");
    assert!(
        tasks
            .iter()
            .all(|id| id.starts_with("stack/tofu/stacks/a/")),
        "{tasks:?}"
    );
    Ok(())
}

/// Docs-only changes mark every tofu obligation unchanged, never silent.
#[test]
fn docs_only_marks_tofu_obligations_unchanged() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("README.md")?;
    assert_eq!(plan.obligations.len(), 6, "full universe still planned");
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.reason != "affected_by_change"),
        "no affected tofu work: {:?}",
        plan.obligations
            .iter()
            .map(|ob| (&ob.task_id, &ob.reason))
            .collect::<Vec<_>>()
    );
    for root in ["stacks/a", "stacks/b"] {
        assert!(
            plan.packages.iter().any(|row| row.package_id == root),
            "{root} row present"
        );
    }
    Ok(())
}

/// A change inside one root marks exactly its triple affected.
#[test]
fn affected_tofu_root_marks_its_triple_changed() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    for ob in &plan.obligations {
        let affected = ob.task_id.starts_with("stack/tofu/stacks/a/");
        assert_eq!(
            ob.reason == "affected_by_change",
            affected,
            "{}: {}",
            ob.task_id,
            ob.reason
        );
    }
    Ok(())
}

/// An empty required inventory fails a tofu plan closed.
#[test]
fn empty_inventory_fails_tofu_plan_closed() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &plan_value,
        &matrix,
        &serde_json::to_value(&reports)?,
        &serde_json::json!([]),
    );
    let report = merge(&request)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"no_entry".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

/// A fully-reported tofu plan merges Passed through the neutral links.
#[test]
fn tofu_passed_round_trip() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    assert_eq!(plan.matrix.include.len(), 6, "full universe planned");
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let request = merge_request(
        &plan_value,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);
    Ok(())
}

/// A missing tofu leg fails Required closed with its cause recorded.
#[test]
fn missing_tofu_report_fails_required_closed() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let reports = passing_reports(&plan)?;
    assert!(reports.len() > 1, "fixture needs two entries");
    let dir = tempfile::TempDir::new()?;
    let run = dir.path().join("run");
    std::fs::create_dir_all(run.join("reports"))?;
    std::fs::write(run.join("plan.json"), serde_json::to_string(&plan)?)?;
    std::fs::write(
        run.join("matrix.json"),
        serde_json::to_string(&plan.matrix)?,
    )?;
    // Stage every report but the first: the missing leg must fail closed.
    for report in &reports[1..] {
        let entry = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.report_id == report.report_id)
            .ok_or_else(|| std::io::Error::other("report without entry"))?;
        let leg = run.join("reports").join(&entry.artifact_id);
        let leg = leg.join(&entry.matrix_key);
        std::fs::create_dir_all(&leg)?;
        std::fs::write(
            leg.join("matrix-report.json"),
            serde_json::to_string(report)?,
        )?;
    }
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        errors
            .iter()
            .any(|e| e.as_str().is_some_and(|s| s.starts_with("missing_report:"))),
        "{errors:?}"
    );
    value["required_job_ids"] = json!(["plan"]);
    value["required_jobs"] = json!([{"job_id": "plan", "conclusion": "success"}]);
    value["assembly_errors"] = serde_json::Value::Array(
        errors
            .into_iter()
            .filter(|e| e.as_str() != Some("missing_needs_channel"))
            .collect(),
    );
    value["actual_event"] = value["plan"]["event"].clone();
    let report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&value.to_string())?)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"source_missing".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

/// Tofu validators fold per validator: missing marks, failure fails.
#[test]
fn tofu_validator_fold_marks_exact_set() -> TestResult {
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let plan_value = serde_json::to_value(&plan)?;
    let inventory = json!(["plan", "actionlint", "tofu-stacks-a"]);
    let run = |jobs: serde_json::Value| {
        let mut request = merge_request(
            &plan_value,
            &matrix,
            &serde_json::to_value(&reports)?,
            &jobs,
        );
        request["required_job_ids"] = inventory.clone();
        merge(&request)
    };

    // A missing tofu validator fails, and the report marks it missing.
    let jobs = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
    ]);
    let report = run(jobs)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report
            .required_job_results
            .iter()
            .any(|job| job.job_id == "tofu-stacks-a" && job.conclusion == JobConclusion::Missing),
        "{:?}",
        report.required_job_results
    );

    // A failing tofu validator fails Required with its conclusion.
    let jobs = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
        {"job_id": "tofu-stacks-a", "conclusion": "failure"},
    ]);
    assert_eq!(run(jobs)?.status, FinalStatus::Failed);

    // Exact coverage passes.
    let jobs = json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": "actionlint", "conclusion": "success"},
        {"job_id": "tofu-stacks-a", "conclusion": "success"},
    ]);
    assert_eq!(run(jobs)?.status, FinalStatus::Passed);
    Ok(())
}

/// Missing plan or matrix on a tofu inventory stays `PlanningFailed` (M3).
#[test]
fn missing_plan_on_tofu_inventory_diagnoses_planning_failed() -> TestResult {
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "plan": null,
        "matrix": null,
        "matrix_reports": [],
        "required_job_ids": ["plan", "tofu-stacks-a"],
        "required_jobs": [
            {"job_id": "plan", "conclusion": "success"},
            {"job_id": "tofu-stacks-a", "conclusion": "success"},
        ],
    });
    let report = merge(&request)?;
    report.validate()?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"source_missing".to_owned()),
        "{:?}",
        report.miss_reasons
    );

    // A missing matrix file with a present tofu plan is also planning_failed.
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": "pull_request",
        "plan": plan,
        "matrix": null,
        "matrix_reports": [],
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "success"}],
    });
    assert_eq!(merge(&request)?.status, FinalStatus::PlanningFailed);
    Ok(())
}
