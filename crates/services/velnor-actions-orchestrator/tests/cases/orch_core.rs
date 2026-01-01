//! Orchestrator core rows: selection, protocol parity.

use std::collections::BTreeSet;
use std::fs;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_contract::{canonical_json_str, validate_digest};
use velnor_actions_contract_workflow::{
    FinalReport, MatrixReport, MatrixStatus, ObligationDecision, Plan, TaskStatus,
};
use velnor_actions_orchestrator::{
    PlanOutputMode, baseline_artifact_numeric_id, merge_internal, plan_internal, plan_outputs,
};

use crate::cases::merge::task_reports_for;
use crate::support::{
    TestResult, config_with_branch, fixture_manifest_json, git, git_line, make_repo,
    plan_for_source_change,
};

/// Merge one request and parse the final report.
pub(crate) fn merge(
    request: &serde_json::Value,
) -> Result<FinalReport, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&merge_internal(
        &request.to_string(),
    )?)?)
}

/// Canonical merge request for plan/matrix values with report/job overrides.
///
/// The declared inventory mirrors the supplied results, so callers testing
/// inventory mismatches must overwrite `required_job_ids` explicitly.
pub(crate) fn merge_request(
    plan: &serde_json::Value,
    matrix: &serde_json::Value,
    reports: &serde_json::Value,
    jobs: &serde_json::Value,
) -> serde_json::Value {
    let ids: Vec<serde_json::Value> = jobs
        .as_array()
        .map(|jobs| jobs.iter().map(|job| job["job_id"].clone()).collect())
        .unwrap_or_default();
    serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": plan.get("event").cloned().unwrap_or(serde_json::Value::Null),
        "plan": plan,
        "matrix": matrix,
        "matrix_reports": reports,
        "task_reports": task_reports_for(plan, reports),
        "required_job_ids": ids,
        "required_jobs": jobs,
    })
}

/// One successful required job.
pub(crate) fn success_jobs() -> serde_json::Value {
    serde_json::json!([{"job_id": "plan", "conclusion": "success"}])
}

/// JSON helper result.
pub(crate) type WireResult = Result<Value, Box<dyn std::error::Error>>;

/// Plan response value for one event/root/revision request.
pub(crate) fn plan_value(
    root: &std::path::Path,
    event: &str,
    base: Option<&str>,
    head: &str,
    m: Option<&Value>,
) -> WireResult {
    let request = json!({"schema": 1, "run_key": "local", "event": event, "base": base,
        "head": head, "root": root.display().to_string(), "baseline_manifest": m});
    Ok(serde_json::from_str(&plan_internal(&request.to_string())?)?)
}

/// Plan response value for a pull-request request.
pub(crate) fn pr_value(
    root: &std::path::Path,
    base: &str,
    head: &str,
    m: Option<&Value>,
) -> WireResult {
    plan_value(root, "pull_request", Some(base), head, m)
}

/// Baseline manifest fixture for one plan.
pub(crate) fn manifest_for(plan: &Plan, base: &str, branch: &str) -> WireResult {
    let first = plan
        .obligations
        .first()
        .ok_or_else(|| std::io::Error::other("obligation"))?;
    let digest = &first.task_digest;
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let name = format!("velnor-baseline-{base}-{digest}");
    let numeric = baseline_artifact_numeric_id(&name);
    Ok(
        json!({"schema": 2, "repository_id": digest, "source_commit": base,
        "ref": format!("refs/heads/{branch}"), "event": "push",
        "workflow_ref": format!("o/r/{workflow}@refs/heads/{branch}"),
        "run_id": 7, "run_attempt": 1, "final_status": "passed",
        "generator_version": plan.generator.version, "generator_sha256": plan.generator.sha256,
        "compatibility_id": digest, "artifact_id": numeric,
        "artifact_name": name,
        "tasks": plan.obligations.iter().map(|ob| json!({"task_id": ob.task_id,
            "task_digest": ob.task_digest, "input_digest": ob.input_digest,
            "closure_digest": ob.closure_digest,
            "proof_run_id": 7, "observed_run_id": 7})).collect::<Vec<_>>()}),
    )
}

/// Merge request for one plan plus its reports.
pub(crate) fn merge_request_for(plan: &Plan, reports: &[MatrixReport]) -> WireResult {
    let plan = serde_json::to_value(plan)?;
    let reports = serde_json::to_value(reports)?;
    Ok(merge_request(
        &plan,
        &plan["matrix"].clone(),
        &reports,
        &success_jobs(),
    ))
}

/// True when the plan carries a warning containing `part`.
pub(crate) fn has_warning(value: &Value, part: &str) -> bool {
    value["plan"]["warnings"].as_array().is_some_and(|list| {
        list.iter()
            .any(|w| w.as_str().is_some_and(|s| s.contains(part)))
    })
}

/// Rewrite the single task of a report, keeping counts coherent.
pub(crate) fn set_task(
    report: &mut MatrixReport,
    status: TaskStatus,
    aggregate: MatrixStatus,
) -> Result<(), Box<dyn std::error::Error>> {
    let task = report
        .tasks
        .first_mut()
        .ok_or_else(|| std::io::Error::other("report without tasks"))?;
    task.status = status;
    task.exit_code = i32::from(status == TaskStatus::Failed);
    report.status = aggregate;
    report.reused = 0;
    report.executed = 0;
    report.empty_partition = 0;
    report.not_selected = 0;
    report.failed = 0;
    report.cancelled = 0;
    match status {
        TaskStatus::Reused => report.reused = 1,
        TaskStatus::Executed => report.executed = 1,
        TaskStatus::EmptyPartition => report.empty_partition = 1,
        TaskStatus::NotSelected => report.not_selected = 1,
        TaskStatus::Failed => report.failed = 1,
        TaskStatus::Cancelled => report.cancelled = 1,
    }
    report.validate()?;
    Ok(())
}

/// Committed single-crate repo plus its head commit.
pub(crate) fn committed_repo() -> Result<(TempDir, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    Ok((repo, head))
}

/// Push plan request selecting everything under `root` at `head`.
pub(crate) fn push_request(root: &std::path::Path, head: &str) -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": serde_json::Value::Null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    })
}

/// Wide workspace repo with `members` crates for budget tests.
pub(crate) fn wide_repo(members: u32) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    let configs = ["c1", "c2", "c3", "c4"]
        .iter()
        .map(|name| format!("{{ name = \"{name}\", target = \"host\" }}"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        format!(
            "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\nconfigurations = [{configs}]\n"
        ),
    )?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    let member_list = (0..members)
        .map(|index| format!("\"members/m{index:02}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nmembers = [{member_list}]\n"),
    )?;
    for index in 0..members {
        let member = root.join(format!("members/m{index:02}"));
        fs::create_dir_all(member.join("src"))?;
        fs::write(
            member.join("Cargo.toml"),
            format!("[package]\nname = \"m{index:02}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(member.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(dir)
}

/// Two-member workspace where only member `a` changed since `base`.
pub(crate) fn plan_for_partial_change() -> Result<(TempDir, Plan), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\n",
    )?;
    for member in ["a", "b"] {
        fs::create_dir_all(root.join(member).join("src"))?;
        fs::write(
            root.join(member).join("Cargo.toml"),
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(root.join(member).join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    fs::write(root.join("a/src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
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
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    plan.validate()?;
    Ok((dir, plan))
}

#[test]
fn orch_core_selected_packages_have_execute_obligations() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let execute: BTreeSet<&str> = plan
        .obligations
        .iter()
        .filter(|ob| ob.decision == ObligationDecision::Execute)
        .map(|ob| ob.task_id.as_str())
        .collect();
    assert!(!execute.is_empty(), "fixture must select work");
    for package in &plan.packages {
        let owned = package
            .tasks
            .iter()
            .any(|task| execute.contains(task.as_str()));
        assert_eq!(package.selected, owned, "{}", package.package_id);
    }
    Ok(())
}

#[test]
fn orch_core_universe_packages_carry_reasons() -> TestResult {
    let (_repo, plan) = plan_for_partial_change()?;
    assert!(!plan.packages.is_empty(), "fixture must inventory members");
    for package in &plan.packages {
        assert!(!package.reasons.is_empty(), "{}", package.package_id);
        if package.tasks.is_empty() {
            assert!(!package.selected, "{}", package.package_id);
        } else {
            assert!(package.selected, "{}", package.package_id);
            assert!(package.reasons.contains(&"selected".to_owned()));
        }
    }
    Ok(())
}

#[test]
fn orch_core_plan_matrix_outputs_byte_identical() -> TestResult {
    let (repo, head) = committed_repo()?;
    let request = push_request(repo.path(), &head);
    let response = plan_internal(&request.to_string())?;
    let outputs = plan_outputs(&response, PlanOutputMode::Static)?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    assert_eq!(value["matrix"], value["plan"]["matrix"]);
    assert_eq!(outputs.matrix, canonical_json_str(&plan.matrix)?);
    assert!(!outputs.matrix.contains('\n'), "single-line output");
    Ok(())
}

#[test]
fn orch_core_obligations_carry_valid_digests() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut seen = BTreeSet::new();
    for obligation in &plan.obligations {
        validate_digest(&obligation.task_digest)?;
        validate_digest(&obligation.input_digest)?;
        validate_digest(&obligation.closure_digest)?;
        assert!(seen.insert(obligation.task_digest.clone()), "distinct");
    }
    for entry in &plan.matrix.include {
        validate_digest(&entry.input_digest)?;
    }
    Ok(())
}
