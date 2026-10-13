//! Orchestrator core rows: merge accounting, status tokens, budgets.

use std::{collections::BTreeSet, fs};

use tempfile::TempDir;
use velnor_actions_contract::{ExecuteTaskRef, FinalStatus, MatrixStatus, Plan, TaskStatus};
use velnor_actions_orchestrator::{assemble_merge_request, plan_internal};

use crate::impl_common::{
    TestResult, err_of, git, git_line, passing_reports, plan_for_source_change,
    without_ambient_ci_env,
};
use crate::impl_merge::task_reports_for;
use crate::impl_orch_core::{
    committed_repo, merge, merge_request, plan_for_partial_change, push_request, set_task,
    success_jobs, wide_repo,
};
use crate::impl_orch_core_cover::covered_plan;

#[test]
fn orch_core_merge_counts_cover_five_states() -> TestResult {
    let repo = wide_repo(2)?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "wide"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let response = plan_internal(&push_request(root, &head).to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    let mut reports = passing_reports(&plan)?;
    assert!(reports.len() >= 5, "fixture needs five entries");
    // Reused claims need restore proof and fail closed separately.
    let cases = [
        (TaskStatus::Executed, MatrixStatus::Passed),
        (TaskStatus::EmptyPartition, MatrixStatus::Passed),
        (TaskStatus::NotSelected, MatrixStatus::NotRun),
        (TaskStatus::Failed, MatrixStatus::Failed),
        (TaskStatus::Cancelled, MatrixStatus::Cancelled),
    ];
    for (report, (status, aggregate)) in reports.iter_mut().zip(cases) {
        set_task(report, status, aggregate)?;
    }
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &serde_json::to_value(&plan)?,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    let counts = &final_report.counts;
    let extra = u32::try_from(reports.len() - cases.len()).unwrap_or(u32::MAX);
    assert_eq!(counts.reused, 0);
    assert_eq!(counts.executed, 1 + extra);
    assert_eq!(counts.empty_partition, 1);
    assert_eq!(counts.blocked, 1);
    assert_eq!(counts.failed, 1);
    assert_eq!(counts.cancelled, 1);
    assert_eq!(counts.not_run, 0);
    assert_eq!(
        counts.reused
            + counts.executed
            + counts.empty_partition
            + counts.blocked
            + counts.failed
            + counts.cancelled,
        u32::try_from(reports.len()).unwrap_or(u32::MAX),
        "each obligation ends in exactly one state"
    );
    assert_eq!(final_report.status, FinalStatus::Failed);
    Ok(())
}

#[test]
fn orch_core_all_covered_merges_passed() -> TestResult {
    without_ambient_ci_env("orch_core_all_covered_merges_passed", || {
        let (_repo, plan) = plan_for_source_change()?;
        assert!(!plan.task_ids.is_empty(), "fixture must select work");
        let (plan_json, manifest) = covered_plan(&plan)?;
        let matrix = plan_json["matrix"].clone();
        let mut request =
            merge_request(&plan_json, &matrix, &serde_json::json!([]), &success_jobs());
        request["baseline_manifest"] = manifest;
        let final_report = merge(&request)?;
        assert_eq!(final_report.status, FinalStatus::Passed);
        assert_eq!(final_report.counts.covered as usize, plan.task_ids.len());
        assert_eq!(final_report.counts.not_run, 0);
        Ok(())
    })
}

#[test]
fn orch_core_covered_claim_binds_original_proof_run() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let (mut plan_json, manifest) = covered_plan(&plan)?;
    plan_json["obligations"][0]["baseline_proof"]["run_id"] = serde_json::json!(8);
    let matrix = plan_json["matrix"].clone();
    let mut request = merge_request(&plan_json, &matrix, &serde_json::json!([]), &success_jobs());
    request["baseline_manifest"] = manifest;
    assert_eq!(merge(&request)?.status, FinalStatus::PlanningFailed);
    Ok(())
}

#[test]
fn orch_core_status_tokens_stay_distinct() -> TestResult {
    let tokens: BTreeSet<String> = [
        TaskStatus::Reused,
        TaskStatus::Executed,
        TaskStatus::EmptyPartition,
        TaskStatus::NotSelected,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<_, _>>()?;
    assert_eq!(tokens.len(), 6);
    let (_repo, plan) = plan_for_source_change()?;
    let text = serde_json::to_string(&plan)?;
    assert!(!text.contains("unaffected"), "explanation only");
    Ok(())
}

#[test]
fn orch_core_entries_emit_single_only() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.matrix.include.is_empty(), "fixture must select work");
    for entry in &plan.matrix.include {
        for task_ref in entry.execute_task_ids.tasks.values() {
            assert!(
                matches!(task_ref, ExecuteTaskRef::Single(_)),
                "no inferred fan-out"
            );
        }
        assert!(entry.test_run.is_empty(), "no test_run refs");
    }
    Ok(())
}

#[test]
fn orch_core_plan_is_deterministic() -> TestResult {
    let (repo, head) = committed_repo()?;
    let request = push_request(repo.path(), &head).to_string();
    assert_eq!(plan_internal(&request)?, plan_internal(&request)?);
    Ok(())
}

#[test]
fn orch_core_matrix_budget_guides_broaden_or_reduce() -> TestResult {
    let repo = wide_repo(60)?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "wide"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = push_request(root, &head);
    let err = err_of(plan_internal(&request.to_string()), "budget exceeded")?;
    let text = err.to_string();
    assert!(text.contains("matrix_budget_exceeded"), "{text}");
    assert!(text.contains("broaden"), "{text}");
    assert!(text.contains("reduce"), "{text}");
    Ok(())
}

#[test]
fn orch_core_plan_retains_all_package_tasks() -> TestResult {
    let (_repo, plan) = plan_for_partial_change()?;
    let selected: BTreeSet<&str> = plan.task_ids.iter().map(String::as_str).collect();
    for package in &plan.packages {
        assert!(!package.tasks.is_empty(), "{}", package.package_id);
        if package.selected {
            for task in &package.tasks {
                assert!(selected.contains(task.as_str()), "{task}");
            }
        }
    }
    Ok(())
}

#[test]
fn obligation_universe_matches_independent_oracle() -> TestResult {
    use crate::impl_orch_plansel::{BUMP, commit, make_ws, plan_at, put};
    use velnor_actions_contract::ObligationDecision::Execute;
    let repo = make_ws(&["alpha", "beta"], &[])?;
    let root = repo.path();
    let base = commit(root, "one")?;
    put(root, "beta/src/lib.rs", BUMP)?;
    let head = commit(root, "two")?;
    // Oracle inputs, independent of the planner: raw cargo metadata, fs checks, documented kind rules.
    assert!(!root.join("rustfmt.toml").exists() && !root.join(".rustfmt.toml").exists());
    let root = std::fs::canonicalize(root).map_err(|_| "canon")?;
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()?;
    assert!(output.status.success(), "cargo metadata failed");
    let meta: serde_json::Value = serde_json::from_str(&String::from_utf8(output.stdout)?)?;
    let members: BTreeSet<&str> = meta["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let mut expected = BTreeSet::new();
    for package in meta["packages"].as_array().into_iter().flatten() {
        if !members.contains(package["id"].as_str().unwrap_or_default()) {
            continue;
        }
        let manifest = package["manifest_path"].as_str().ok_or("manifest")?;
        let rel = std::path::Path::new(manifest)
            .strip_prefix(&root)
            .map_err(|_| "rel")?;
        let key = rel
            .parent()
            .and_then(|dir| dir.to_str())
            .filter(|dir| !dir.is_empty())
            .unwrap_or("root");
        for kind in ["clippy", "test", "doctest", "doc"] {
            expected.insert(format!("stack/rust/{key}/{kind}/default"));
        }
    }
    let (plan, _) = plan_at(&root, Some(&base), &head, None)?;
    let planned: BTreeSet<String> = plan.task_ids.iter().cloned().collect();
    assert_eq!(planned, expected, "oracle set equals plan output");
    assert!(plan.obligations.iter().all(|ob| ob.decision == Execute));
    Ok(())
}

#[test]
fn blocked_tasks_resolve_below_cancelled() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut reports = passing_reports(&plan)?;
    for report in &mut reports {
        set_task(report, TaskStatus::NotSelected, MatrixStatus::NotRun)?;
    }
    let run = |reports: &[velnor_actions_contract::MatrixReport]| -> Result<velnor_actions_contract::FinalReport, Box<dyn std::error::Error>> {
        let request = merge_request(
            &serde_json::to_value(&plan)?,
            &serde_json::to_value(&plan.matrix)?,
            &serde_json::to_value(reports)?,
            &success_jobs(),
        );
        merge(&request)
    };
    let final_report = run(&reports)?;
    assert_eq!(final_report.status, FinalStatus::Blocked);
    assert_eq!(final_report.counts.blocked as usize, reports.len());
    assert_eq!(final_report.counts.not_run, 0);
    set_task(
        &mut reports[0],
        TaskStatus::Cancelled,
        MatrixStatus::Cancelled,
    )?;
    assert_eq!(run(&reports)?.status, FinalStatus::Cancelled);
    set_task(&mut reports[0], TaskStatus::Failed, MatrixStatus::Failed)?;
    assert_eq!(run(&reports)?.status, FinalStatus::Failed);
    Ok(())
}

#[test]
fn staged_symlinks_and_oversize_reports_reject() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let files = task_reports_for(&plan_value, &serde_json::to_value(&reports)?);
    let entry = plan.matrix.include.first().ok_or("entry")?;
    let report = reports
        .iter()
        .find(|report| report.report_id == entry.report_id)
        .ok_or("report")?;
    let dir = TempDir::new()?;
    let run = dir.path().join("run");
    let home = run
        .join("reports")
        .join(&entry.artifact_id)
        .join(&entry.matrix_key);
    fs::create_dir_all(home.join("tasks"))?;
    fs::write(run.join("plan.json"), serde_json::to_string(&plan)?)?;
    fs::write(
        run.join("matrix.json"),
        serde_json::to_string(&plan.matrix)?,
    )?;
    fs::write(
        home.join("matrix-report.json"),
        serde_json::to_string(report)?,
    )?;
    for task in &report.tasks {
        let want = Some(task.task_report_id.as_str());
        let file = files
            .as_array()
            .and_then(|list| {
                list.iter()
                    .find(|file| file["task_report_id"].as_str() == want)
            })
            .ok_or("file")?;
        fs::write(
            home.join("tasks")
                .join(format!("{}.json", task.task_report_id)),
            serde_json::to_string(file)?,
        )?;
    }
    let target = home
        .join("tasks")
        .join(format!("{}.json", report.tasks[0].task_report_id));
    fs::remove_file(&target)?;
    std::os::unix::fs::symlink(home.join("matrix-report.json"), &target)?;
    let value: serde_json::Value = serde_json::from_str(&assemble_merge_request("local", &run)?)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        errors.iter().any(|error| error
            .as_str()
            .is_some_and(|s| s.starts_with("symlink_task:"))),
        "{errors:?}"
    );
    fs::remove_file(&target)?;
    let want = Some(report.tasks[0].task_report_id.as_str());
    let file = files
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|file| file["task_report_id"].as_str() == want)
        })
        .ok_or("file")?;
    fs::write(&target, serde_json::to_string(file)?)?;
    let big = "x".repeat(1_048_577);
    fs::write(
        home.join("matrix-report.json"),
        format!("{{\"pad\":\"{big}\"}}"),
    )?;
    let value: serde_json::Value = serde_json::from_str(&assemble_merge_request("local", &run)?)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        errors.iter().any(|error| error
            .as_str()
            .is_some_and(|s| s.starts_with("oversize_report:"))),
        "{errors:?}"
    );
    Ok(())
}
