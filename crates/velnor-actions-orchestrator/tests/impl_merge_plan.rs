//! Merge plan-shape cases: missing/hollow plans, agreement tokens,
//! uncovered legs, and coverage revalidation.

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, merge_fixture,
    passing_reports, plan_for_source_change,
};
use crate::impl_merge::{merge, merge_request, success_jobs};
use velnor_actions_contract::{FinalStatus, JobDisposition, ObligationDecision, Plan};

#[path = "impl_merge_plan_native.rs"]
mod native;

/// Plan an empty pull-request diff; returns the plan and its skipped jobs.
pub(crate) fn empty_diff_plan()
-> Result<(tempfile::TempDir, Plan, Vec<String>), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let plan_request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": head,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = velnor_actions_orchestrator::plan_internal(&plan_request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let skipped = plan
        .jobs
        .iter()
        .filter(|job| job.disposition == JobDisposition::NotAffected)
        .map(|job| job.job_id.clone())
        .collect();
    Ok((repo, plan, skipped))
}

#[test]
fn empty_diff_skips_every_crate_job() -> TestResult {
    let (_repo, plan, crate_jobs) = empty_diff_plan()?;
    assert!(!plan.obligations.is_empty(), "empty diff keeps universe");
    assert!(!plan.task_ids.is_empty(), "empty diff keeps universe");
    assert!(
        plan.matrix.include.is_empty(),
        "nothing executes: {:?}",
        plan.matrix
    );
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::NotAffected),
        "nothing affected: {:?}",
        plan.obligations
    );
    assert!(!crate_jobs.is_empty(), "{:?}", plan.jobs);
    let jobs = |conclusion: &str| {
        let mut jobs = vec![serde_json::json!({"job_id": "plan", "conclusion": "success"})];
        jobs.extend(
            crate_jobs
                .iter()
                .map(|id| serde_json::json!({"job_id": id, "conclusion": conclusion})),
        );
        serde_json::Value::Array(jobs)
    };
    let matrix = serde_json::to_value(&plan.matrix)?;
    let no_reports = serde_json::json!([]);

    // Plan skip + job skipped: passes, never no-work.
    let report = merge(&merge_request(
        &plan,
        &matrix,
        &no_reports,
        &jobs("skipped"),
    ))?;
    assert_eq!(
        report.status,
        FinalStatus::Passed,
        "{:?}",
        report.miss_reasons
    );
    assert_eq!(report.counts.not_affected as usize, plan.obligations.len());

    // Plan skip + job ran: the gate disagreed with the plan.
    let report = merge(&merge_request(
        &plan,
        &matrix,
        &no_reports,
        &jobs("success"),
    ))?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(report.miss_reasons.contains(&"gate_mismatch".to_owned()));

    // Plan skip + job failed or cancelled stays failed or cancelled.
    let report = merge(&merge_request(
        &plan,
        &matrix,
        &no_reports,
        &jobs("failure"),
    ))?;
    assert_eq!(report.status, FinalStatus::Failed);
    let report = merge(&merge_request(
        &plan,
        &matrix,
        &no_reports,
        &jobs("cancelled"),
    ))?;
    assert_eq!(report.status, FinalStatus::Cancelled);

    // A failed plan fails Required whatever the crate jobs did.
    let failed_plan = serde_json::json!([
        {"job_id": "plan", "conclusion": "failure"},
        {"job_id": crate_jobs[0], "conclusion": "skipped"},
    ]);
    let report = merge(&merge_request(&plan, &matrix, &no_reports, &failed_plan))?;
    assert_eq!(report.status, FinalStatus::Failed);

    // Malformed merge requests stay hard errors, not reports.
    let err = err_of(
        merge_fixture(r#"{"schema":1,"run_key":"local"}"#),
        "truncated merge rejected",
    )?;
    assert!(matches!(
        err,
        velnor_actions_orchestrator::OrchestratorError::Internal { .. }
    ));
    Ok(())
}

#[test]
fn planned_run_job_skipped_fails_required() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let run_job = plan
        .jobs
        .iter()
        .find(|job| job.disposition == JobDisposition::Run && job.job_id.starts_with("rust-"))
        .ok_or("fixture needs a running crate job")?;
    let reports = passing_reports(&plan)?;
    let jobs = serde_json::json!([
        {"job_id": "plan", "conclusion": "success"},
        {"job_id": run_job.job_id, "conclusion": "skipped"},
    ]);
    let report = merge(&merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &jobs,
    ))?;
    assert_eq!(report.status, FinalStatus::NotRun);
    assert!(
        report
            .miss_reasons
            .contains(&"planned_job_skipped".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

#[test]
fn missing_plan_merges_to_planning_failed() -> TestResult {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "plan": null,
        "matrix": null,
        "matrix_reports": [],
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "failure"}],
    });
    let final_report = merge(&request)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert_eq!(final_report.report_id, "final-local");
    assert_eq!(final_report.expected_report_ids, [] as [String; 0]);
    assert_eq!(final_report.required_job_results.len(), 1);

    // A missing matrix file with a present plan is also planning_failed.
    let (_repo, plan) = plan_for_source_change()?;
    let request = serde_json::json!({
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

#[test]
fn hollow_plan_with_zero_entries_fails_closed() -> TestResult {
    let (_repo, mut plan) = plan_for_source_change()?;
    assert!(
        !plan.obligations.is_empty()
            && plan
                .obligations
                .iter()
                .all(|ob| ob.decision == ObligationDecision::Execute),
        "fixture must be all-Execute: {:?}",
        plan.obligations
    );
    plan.matrix.include.clear();
    assert!(
        plan.validate().is_err(),
        "a run job without entries is refused"
    );
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(&plan, &matrix, &serde_json::json!([]), &success_jobs());
    let final_report = merge(&request)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"cache_corrupt".to_owned()),
        "hollow plan diagnosed: {:?}",
        final_report.miss_reasons
    );
    Ok(())
}

#[test]
fn partial_hollow_plan_with_dropped_leg_fails_closed() -> TestResult {
    let (_repo, mut plan) = plan_for_source_change()?;
    assert!(plan.matrix.include.len() > 1, "fixture needs two entries");
    plan.matrix.include.pop();
    plan.validate()?;
    // Reports cover every remaining leg exactly; the dropped Execute
    // obligation has zero task evidence and must fail the verdict.
    let reports = passing_reports(&plan)?;
    assert!(!reports.is_empty(), "remaining legs stay covered");
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(
        &plan,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report.miss_reasons.contains(&"no_entry".to_owned()),
        "dropped leg diagnosed: {:?}",
        final_report.miss_reasons
    );
    Ok(())
}

#[test]
fn agreement_failures_carry_miss_tokens() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let reports = passing_reports(&plan)?;

    // A missing matrix.json never landed: source_missing.
    let request = merge_request(
        &plan,
        &serde_json::Value::Null,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"source_missing".to_owned()),
        "{:?}",
        final_report.miss_reasons
    );

    // A disagreeing matrix.json corrupts the evidence set: cache_corrupt.
    let mut trimmed = matrix.clone();
    trimmed["include"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("matrix shape"))?
        .pop();
    let request = merge_request(
        &plan,
        &trimmed,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"cache_corrupt".to_owned()),
        "{:?}",
        final_report.miss_reasons
    );
    Ok(())
}

#[test]
fn task_file_gaps_fail_closed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let good = passing_reports(&plan)?;
    let jobs = success_jobs();
    let check = |request: &serde_json::Value, token: &str| -> TestResult {
        let final_report = merge(request)?;
        assert_eq!(final_report.status, FinalStatus::PlanningFailed);
        assert!(final_report.miss_reasons.contains(&token.to_owned()));
        Ok(())
    };
    let mut missing = merge_request(&plan, &matrix, &serde_json::to_value(&good)?, &jobs);
    missing["task_reports"].as_array_mut().ok_or("tasks")?.pop();
    check(&missing, "source_missing")?;
    let mut contrary = merge_request(&plan, &matrix, &serde_json::to_value(&good)?, &jobs);
    let files = contrary["task_reports"].as_array_mut().ok_or("tasks")?;
    files[0]["status"] = serde_json::json!("failed");
    files[0]["exit_code"] = serde_json::json!(1);
    check(&contrary, "cache_corrupt")?;
    let mut incoherent = good.clone();
    incoherent[0].tasks[0].exit_code = 1;
    let request = merge_request(&plan, &matrix, &serde_json::to_value(&incoherent)?, &jobs);
    check(&request, "cache_corrupt")?;
    let mut outputs = merge_request(&plan, &matrix, &serde_json::to_value(&good)?, &jobs);
    outputs["task_reports"].as_array_mut().ok_or("tasks")?[0]["outputs"] =
        serde_json::json!(["target/evil"]);
    check(&outputs, "cache_corrupt")?;
    let mut swapped = merge_request(&plan, &matrix, &serde_json::to_value(&good)?, &jobs);
    let files = swapped["task_reports"].as_array_mut().ok_or("tasks")?;
    assert!(files.len() > 1, "fixture needs two tasks");
    let other = files[1]["task_id"].clone();
    files[0]["task_id"] = other;
    check(&swapped, "cache_corrupt")?;
    let mut unexpected = merge_request(&plan, &matrix, &serde_json::to_value(&good)?, &jobs);
    let matrix_id = "stack:rust|task:stack/rust/foreign/build/default";
    let matrix_key = velnor_actions_contract::matrix_key_for_id(matrix_id)?;
    let digest = velnor_actions_contract::digest_b3(b"foreign-task");
    let report_id =
        velnor_actions_contract::task_report_id_for_task("local", &matrix_key, &digest)?;
    let foreign = serde_json::json!({"schema": velnor_actions_contract::TaskReport::SCHEMA,
        "task_report_id": report_id, "run_key": "local",
        "event": "pull_request", "trust": "pr", "matrix_id": matrix_id, "matrix_key": matrix_key,
        "task_id": "stack/rust/foreign/build/default", "task_digest": digest, "status": "executed",
        "cache": {"layer": "task", "key": "", "result": "not_attempted"},
        "platform_binding": {"state": "unavailable",
            "planned_platform_id": velnor_actions_contract::digest_b3(b"planned-platform"),
            "runner_environment": "unknown", "reason": "observation_not_recorded"},
        "exit_code": 0, "duration_ms": 0, "outputs": []});
    let parsed: velnor_actions_contract::TaskReport = serde_json::from_value(foreign.clone())?;
    parsed.validate()?;
    unexpected["task_reports"]
        .as_array_mut()
        .ok_or("tasks")?
        .push(foreign);
    check(&unexpected, "cache_corrupt")?;
    Ok(())
}

#[test]
fn uncovered_leg_carries_miss_token() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let mut reports = passing_reports(&plan)?;
    reports.pop();
    let request = merge_request(
        &plan,
        &matrix,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::NotRun);
    assert!(
        final_report.miss_reasons.contains(&"no_entry".to_owned()),
        "{:?}",
        final_report.miss_reasons
    );
    Ok(())
}
