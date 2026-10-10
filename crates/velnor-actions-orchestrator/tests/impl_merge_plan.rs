//! Merge plan-shape cases: missing/hollow plans, agreement tokens,
//! uncovered legs, and coverage revalidation.

use velnor_actions_contract::{FinalStatus, ObligationDecision, Plan};
use velnor_actions_orchestrator::merge_internal;

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, passing_reports,
    plan_for_source_change, without_ambient_ci_env,
};
use crate::impl_merge::{merge, merge_request, success_jobs};
use crate::impl_orch_core::{merge as core_merge, merge_request as core_merge_request};
use crate::impl_orch_core_cover::covered_plan;

#[test]
fn empty_diff_no_baseline_executes_all() -> TestResult {
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
    assert!(!plan.obligations.is_empty(), "empty diff keeps universe");
    assert!(!plan.task_ids.is_empty(), "empty diff keeps universe");
    assert!(!plan.matrix.include.is_empty(), "empty diff executes");
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.reason == "forced_uncached"),
        "nothing proven: {:?}",
        plan.obligations
    );

    let reports = passing_reports(&plan)?;
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);

    // Without reports the forced-uncached legs are not-run, never no-work.
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::json!([]),
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::NotRun);

    // Malformed merge requests stay hard errors, not reports.
    let err = err_of(
        merge_internal(r#"{"schema":1,"run_key":"local"}"#),
        "truncated merge rejected",
    )?;
    assert!(matches!(
        err,
        velnor_actions_orchestrator::OrchestratorError::Internal { .. }
    ));
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
    plan.validate()?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(&plan, &matrix, &serde_json::json!([]), &success_jobs());
    let final_report = merge(&request)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report.miss_reasons.contains(&"no_entry".to_owned()),
        "hollow legs diagnosed: {:?}",
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
fn covered_claim_binds_numeric_artifact_id() -> TestResult {
    without_ambient_ci_env("covered_claim_binds_numeric_artifact_id", || {
        let (_repo, plan) = plan_for_source_change()?;
        let (plan_json, mut manifest) = covered_plan(&plan)?;
        manifest["artifact_id"] = serde_json::json!(10);
        let matrix = plan_json["matrix"].clone();
        let mut request =
            core_merge_request(&plan_json, &matrix, &serde_json::json!([]), &success_jobs());
        request["baseline_manifest"] = manifest;
        let final_report = core_merge(&request)?;
        assert_eq!(final_report.status, FinalStatus::PlanningFailed);
        assert!(
            final_report
                .miss_reasons
                .contains(&"cache_corrupt".to_owned())
        );
        Ok(())
    })
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

#[test]
fn revalidate_failures_carry_miss_tokens() -> TestResult {
    without_ambient_ci_env("revalidate_failures_carry_miss_tokens", || {
        let (_repo, plan) = plan_for_source_change()?;

        // Covered claims without a manifest have no proof source: source_missing.
        let (plan_json, _manifest) = covered_plan(&plan)?;
        let covered_matrix = plan_json["matrix"].clone();
        let request = core_merge_request(
            &plan_json,
            &covered_matrix,
            &serde_json::json!([]),
            &success_jobs(),
        );
        let final_report = core_merge(&request)?;
        assert_eq!(final_report.status, FinalStatus::PlanningFailed);
        assert!(
            final_report
                .miss_reasons
                .contains(&"source_missing".to_owned()),
            "{:?}",
            final_report.miss_reasons
        );

        // Covered claims bound to a tampered proof run: cache_corrupt.
        let (mut plan_json, manifest) = covered_plan(&plan)?;
        plan_json["obligations"][0]["baseline_proof"]["run_id"] = serde_json::json!(8);
        let covered_matrix = plan_json["matrix"].clone();
        let mut request = core_merge_request(
            &plan_json,
            &covered_matrix,
            &serde_json::json!([]),
            &success_jobs(),
        );
        request["baseline_manifest"] = manifest;
        let final_report = core_merge(&request)?;
        assert_eq!(final_report.status, FinalStatus::PlanningFailed);
        assert!(
            final_report
                .miss_reasons
                .contains(&"cache_corrupt".to_owned()),
            "{:?}",
            final_report.miss_reasons
        );
        Ok(())
    })
}
