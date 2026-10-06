//! Staged-report verdict tests: assembly tokens reach the merge verdict.

use serde_json::json;
use velnor_actions_contract::FinalStatus;
use velnor_actions_orchestrator::{assemble_merge_request, merge_internal};

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_merge::task_reports_for;

/// Staged run directory with `plan.json` plus `matrix.json` written.
fn stage_run(
    plan: &velnor_actions_contract::Plan,
) -> Result<(tempfile::TempDir, std::path::PathBuf), Box<dyn std::error::Error>> {
    let dir = tempfile::TempDir::new()?;
    let run = dir.path().join("run");
    std::fs::create_dir_all(run.join("reports"))?;
    std::fs::write(run.join("plan.json"), serde_json::to_string(plan)?)?;
    std::fs::write(
        run.join("matrix.json"),
        serde_json::to_string(&plan.matrix)?,
    )?;
    Ok((dir, run))
}

/// Created leg directory for one matrix report.
fn leg_for(
    run: &std::path::Path,
    plan: &velnor_actions_contract::Plan,
    report: &velnor_actions_contract::MatrixReport,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let entry = plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.report_id == report.report_id)
        .ok_or_else(|| std::io::Error::other("report without entry"))?;
    let leg = run
        .join("reports")
        .join(&entry.artifact_id)
        .join(&entry.matrix_key);
    std::fs::create_dir_all(&leg)?;
    Ok(leg)
}

/// Merge one staged request with a passing `plan` validator job.
fn merge_staged(
    value: &mut serde_json::Value,
    errors: Vec<serde_json::Value>,
) -> Result<velnor_actions_contract::FinalReport, Box<dyn std::error::Error>> {
    value["required_job_ids"] = json!(["plan"]);
    value["required_jobs"] = json!([{"job_id": "plan", "conclusion": "success"}]);
    value["assembly_errors"] = serde_json::Value::Array(
        errors
            .into_iter()
            .filter(|e| e.as_str() != Some("missing_needs_channel"))
            .collect(),
    );
    value["actual_event"] = value["plan"]["event"].clone();
    Ok(serde_json::from_str(&merge_internal(&value.to_string())?)?)
}

/// Assert one `planning_failed` verdict carrying the corruption token.
fn assert_corrupt(report: &velnor_actions_contract::FinalReport) -> TestResult {
    report.validate()?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"cache_corrupt".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}

#[test]
fn unparsable_report_carries_to_verdict() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    assert!(reports.len() > 1, "fixture needs two entries");
    let (_dir, run) = stage_run(&plan)?;
    // Stage every leg, but corrupt the first report body: assembly must
    // record the unparsable token and the merge must carry it to a
    // planning_failed verdict with the corruption token.
    for (index, report) in reports.iter().enumerate() {
        let leg = leg_for(&run, &plan, report)?;
        let body = if index == 0 {
            "not json".to_owned()
        } else {
            serde_json::to_string(report)?
        };
        std::fs::write(leg.join("matrix-report.json"), body)?;
    }
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        errors.iter().any(|e| e
            .as_str()
            .is_some_and(|s| s.starts_with("unparsable_report:"))),
        "{errors:?}"
    );
    assert_corrupt(&merge_staged(&mut value, errors)?)
}

/// Shape-malformed report diagnoses: valid JSON, wrong shape.
#[test]
fn shape_malformed_report_diagnoses_planning_failed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    assert!(reports.len() > 1, "fixture needs two entries");
    let (_dir, run) = stage_run(&plan)?;
    // Corrupt the first report body as valid JSON missing its typed ID:
    // assembly validates syntax only, so it passes the value through
    // with no error and the merge must still diagnose it.
    for (index, report) in reports.iter().enumerate() {
        let leg = leg_for(&run, &plan, report)?;
        let mut body = serde_json::to_value(report)?;
        if index == 0 {
            body.as_object_mut()
                .ok_or("report object")?
                .remove("report_id");
        }
        std::fs::write(leg.join("matrix-report.json"), body.to_string())?;
    }
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        !errors.iter().any(|e| e.as_str().is_some_and(
            |s| s.starts_with("unparsable_report:") || s.starts_with("missing_report:")
        )),
        "syntax-valid body passes assembly: {errors:?}"
    );
    assert_corrupt(&merge_staged(&mut value, errors)?)
}

/// Shape-malformed task file diagnoses: valid JSON, wrong shape.
#[test]
fn shape_malformed_task_file_diagnoses_planning_failed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let reports_value = serde_json::to_value(&reports)?;
    let tasks = task_reports_for(&plan_value, &reports_value);
    let tasks = tasks.as_array().ok_or("task array")?;
    assert!(tasks.len() > 1, "fixture needs two task files");
    let (_dir, run) = stage_run(&plan)?;
    for report in &reports {
        let leg = leg_for(&run, &plan, report)?;
        std::fs::create_dir_all(leg.join("tasks"))?;
        std::fs::write(
            leg.join("matrix-report.json"),
            serde_json::to_string(report)?,
        )?;
    }
    stage_task_files(&run, &plan, tasks)?;
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        !errors.iter().any(|e| e
            .as_str()
            .is_some_and(|s| s.starts_with("unparsable_task:") || s.starts_with("missing_task:"))),
        "syntax-valid file passes assembly: {errors:?}"
    );
    assert_corrupt(&merge_staged(&mut value, errors)?)
}

/// Shape-malformed baseline diagnoses: valid JSON, wrong shape.
#[test]
fn shape_malformed_baseline_diagnoses_planning_failed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let (_dir, run) = stage_run(&plan)?;
    for report in &reports {
        let leg = leg_for(&run, &plan, report)?;
        std::fs::write(
            leg.join("matrix-report.json"),
            serde_json::to_string(report)?,
        )?;
    }
    // Valid JSON with no manifest shape: assembly validates syntax
    // only, so it passes the value through and the merge must
    // diagnose it instead of hard-erroring the request.
    std::fs::write(run.join("baseline.json"), r#"{"bogus":1}"#)?;
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert!(
        !errors.iter().any(|e| e
            .as_str()
            .is_some_and(|s| s.starts_with("unparsable_baseline"))),
        "syntax-valid body passes assembly: {errors:?}"
    );
    assert_corrupt(&merge_staged(&mut value, errors)?)
}

/// Shape-malformed attestation diagnoses: valid JSON, wrong shape.
#[test]
fn shape_malformed_attestation_diagnoses_planning_failed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let (_dir, run) = stage_run(&plan)?;
    for report in &reports {
        let leg = leg_for(&run, &plan, report)?;
        std::fs::write(
            leg.join("matrix-report.json"),
            serde_json::to_string(report)?,
        )?;
    }
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    // Inject the syntax-valid shape-wrong value assembly would pass
    // through from the staged attestation file (candidate mode reads
    // it JSON-only): the merge must diagnose, not hard-error.
    value["candidate_attestation"] = json!({"bogus": 1});
    let errors = value["assembly_errors"].as_array().ok_or("errors")?.clone();
    assert_corrupt(&merge_staged(&mut value, errors)?)
}

/// Stage every expected task file; corrupt the first as valid JSON
/// missing its typed ID so assembly passes it through silently.
fn stage_task_files(
    run: &std::path::Path,
    plan: &velnor_actions_contract::Plan,
    tasks: &[serde_json::Value],
) -> TestResult {
    for (index, task) in tasks.iter().enumerate() {
        let task_id = task
            .get("task_report_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("task id")?;
        let matrix_key = task
            .get("matrix_key")
            .and_then(serde_json::Value::as_str)
            .ok_or("matrix key")?;
        let entry = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.matrix_key == matrix_key)
            .ok_or_else(|| std::io::Error::other("task without entry"))?;
        let mut body = task.clone();
        if index == 0 {
            body.as_object_mut()
                .ok_or("task object")?
                .remove("task_report_id");
        }
        std::fs::write(
            run.join("reports")
                .join(&entry.artifact_id)
                .join(&entry.matrix_key)
                .join("tasks")
                .join(format!("{task_id}.json")),
            body.to_string(),
        )?;
    }
    Ok(())
}
