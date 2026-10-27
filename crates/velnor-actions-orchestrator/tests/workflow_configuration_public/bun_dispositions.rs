// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::process::Command;

use serde_json::{Value, json};
use velnor_actions_orchestrator::{
    MergeContext, merge_internal_with_context, migrate_config, plan_internal, prepare,
    render_staged_tree_with,
};

use super::bun_routing::make_bun_repo;
use velnor_actions_contract::ExecutionMode;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

#[test]
fn both_mode_required_results_follow_logical_bun_dispositions() -> Result<(), Box<dyn Error>> {
    let (_repo, response) = plan_bun_both_change()?;
    let plan = response
        .get("plan")
        .ok_or("public plan response has no plan")?;
    let planned_jobs = plan
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or("plan has no required job dispositions")?;
    let ui = planned_jobs
        .iter()
        .find(|job| job["job_id"] == "bun-fixture-ui")
        .ok_or("unaffected UI member is absent from logical plan")?;
    let web = planned_jobs
        .iter()
        .find(|job| job["job_id"] == "bun-fixture-web")
        .ok_or("affected web member is absent from logical plan")?;
    assert_eq!(ui["disposition"], "not_affected");
    assert_eq!(web["disposition"], "run");
    assert!(planned_jobs.iter().all(|job| {
        !job["job_id"]
            .as_str()
            .unwrap_or_default()
            .ends_with("__hosted")
            && !job["job_id"]
                .as_str()
                .unwrap_or_default()
                .ends_with("__local")
    }));

    let results = results_for(planned_jobs);
    let unaffected_skip = merge(plan, planned_jobs, &results)?;
    assert_eq!(unaffected_skip["status"], "no_work");
    assert!(!has_reason(&unaffected_skip, "gate_mismatch"));
    assert!(!has_reason(&unaffected_skip, "planned_job_skipped"));

    let mut selected_skip_results = results.clone();
    set_result(&mut selected_skip_results, "bun-fixture-web", "skipped")?;
    let selected_skip = merge(plan, planned_jobs, &selected_skip_results)?;
    assert_eq!(selected_skip["status"], "not_run");
    assert!(has_reason(&selected_skip, "planned_job_skipped"));

    let missing_results: Vec<Value> = results
        .iter()
        .filter(|job| job["job_id"] != "bun-fixture-web")
        .cloned()
        .collect();
    let missing = merge(plan, planned_jobs, &missing_results)?;
    assert_ne!(missing["status"], "passed");
    assert!(
        missing["required_job_results"]
            .as_array()
            .is_some_and(|jobs| {
                jobs.iter()
                    .any(|job| job["job_id"] == "bun-fixture-web" && job["conclusion"] == "missing")
            })
    );

    let mut failed_results = results;
    set_result(&mut failed_results, "bun-fixture-web", "failure")?;
    let failed = merge(plan, planned_jobs, &failed_results)?;
    assert_eq!(failed["status"], "failed");
    Ok(())
}

fn plan_bun_both_change() -> Result<(tempfile::TempDir, Value), Box<dyn Error>> {
    let repo = make_bun_repo()?;
    let config = migrate_config(repo.path(), 2, true)?;
    fs::write(repo.path().join(".velnor/config.toml"), config)?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree_with(&prepared, Some(ExecutionMode::Both))?;
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered CI workflow is missing")?;
    assert!(workflow.contains("bun-fixture-web:"));
    assert!(!workflow.contains("bun-fixture-web__hosted:"));

    git(repo.path(), &["branch", "-m", "main"])?;
    git(repo.path(), &["add", "."])?;
    git(repo.path(), &["commit", "-m", "fixture base"])?;
    let base = git_output(repo.path(), &["rev-parse", "HEAD"])?;
    let app_manifest = repo.path().join("apps/web/package.json");
    let mut contents = fs::read_to_string(&app_manifest)?;
    contents.push('\n');
    fs::write(&app_manifest, contents)?;
    git(repo.path(), &["add", "apps/web/package.json"])?;
    git(repo.path(), &["commit", "-m", "change web member"])?;
    let head = git_output(repo.path(), &["rev-parse", "HEAD"])?;
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "local",
        "base": base,
        "head": head,
        "event": "pull_request",
        "root": repo.path(),
    });
    let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    Ok((repo, response))
}

fn results_for(jobs: &[Value]) -> Vec<Value> {
    jobs.iter()
        .map(|job| {
            json!({
                "job_id": job["job_id"],
                "conclusion": if job["disposition"] == "not_affected" { "skipped" } else { "success" },
            })
        })
        .collect()
}

fn set_result(jobs: &mut [Value], id: &str, conclusion: &str) -> Result<(), Box<dyn Error>> {
    let job = jobs
        .iter_mut()
        .find(|job| job["job_id"] == id)
        .ok_or_else(|| format!("required result is missing: {id}"))?;
    job["conclusion"] = json!(conclusion);
    Ok(())
}

fn merge(plan: &Value, jobs: &[Value], results: &[Value]) -> Result<Value, Box<dyn Error>> {
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": "pull_request",
        "plan": plan,
        "matrix": plan["matrix"],
        "matrix_reports": [],
        "task_reports": [],
        "required_job_ids": jobs.iter().map(|job| job["job_id"].clone()).collect::<Vec<_>>(),
        "required_jobs": results,
    });
    let report = merge_internal_with_context(&request.to_string(), &MergeContext::detached())?;
    Ok(serde_json::from_str(&report)?)
}

fn has_reason(report: &Value, reason: &str) -> bool {
    report["miss_reasons"]
        .as_array()
        .is_some_and(|reasons| reasons.iter().any(|value| value == reason))
}

fn git(root: &std::path::Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = git_command(root, args).output()?;
    if !output.status.success() {
        return Err(format!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(())
}

fn git_output(root: &std::path::Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = git_command(root, args).output()?;
    if !output.status.success() {
        return Err(format!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_command(root: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args);
    command
}
