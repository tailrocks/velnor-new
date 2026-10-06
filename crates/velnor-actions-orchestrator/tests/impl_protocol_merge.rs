//! Merge assembly, artifact publication, and verdict protocol checks.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::canonical_json_str;
use velnor_actions_orchestrator::{
    assemble_merge_request, merge_internal, merge_passed, plan_internal, publish_plan_files,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, passing_reports,
    plan_for_source_change,
};
use crate::impl_merge::task_reports_for;

/// Producer/consumer agreement: assembled files feed the merge unchanged.
#[test]
fn merge_assembled_request_roundtrips_to_passed() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    assert!(!reports.is_empty(), "fixture must carry reports");
    let dir = TempDir::new()?;
    let run = dir.path().join("run");
    fs::create_dir_all(run.join("reports"))?;
    fs::write(run.join("plan.json"), serde_json::to_string(&plan)?)?;
    fs::write(
        run.join("matrix.json"),
        serde_json::to_string(&plan.matrix)?,
    )?;
    let plan_value = serde_json::to_value(&plan)?;
    let task_files = task_reports_for(&plan_value, &serde_json::to_value(&reports)?);
    for report in &reports {
        let entry = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.report_id == report.report_id)
            .ok_or_else(|| std::io::Error::other("report without entry"))?;
        let dir = run
            .join("reports")
            .join(&entry.artifact_id)
            .join(&entry.matrix_key);
        fs::create_dir_all(dir.join("tasks"))?;
        fs::write(
            dir.join("matrix-report.json"),
            serde_json::to_string(report)?,
        )?;
        for task in &report.tasks {
            let want = Some(task.task_report_id.as_str());
            let file = task_files
                .as_array()
                .and_then(|files| {
                    files
                        .iter()
                        .find(|file| file["task_report_id"].as_str() == want)
                })
                .ok_or_else(|| std::io::Error::other("task without file"))?;
            let path = dir
                .join("tasks")
                .join(format!("{}.json", task.task_report_id));
            fs::write(path, serde_json::to_string(file)?)?;
        }
    }
    let request = assemble_merge_request("local", &run)?;
    let mut value: serde_json::Value = serde_json::from_str(&request)?;
    assert!(value.get("base").is_none(), "merge shape: {request}");
    value["required_job_ids"] = serde_json::json!(["plan"]);
    value["required_jobs"] = serde_json::json!([{"job_id": "plan", "conclusion": "success"}]);
    value["assembly_errors"] = serde_json::json!([]);
    value["actual_event"] = value["plan"]["event"].clone();
    let final_report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&value.to_string())?)?;
    assert_eq!(
        final_report.status,
        velnor_actions_contract::FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn merge_assembly_nulls_missing_plan_to_planning_failed() -> TestResult {
    let dir = TempDir::new()?;
    let request = assemble_merge_request("local", dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&request)?;
    assert!(value["plan"].is_null(), "null plan: {request}");
    assert!(
        value["assembly_errors"]
            .as_array()
            .is_some_and(|errors| errors.len() >= 3),
        "gaps recorded: {request}"
    );
    let final_report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&request)?)?;
    final_report.validate()?;
    assert_eq!(
        final_report.status,
        velnor_actions_contract::FinalStatus::PlanningFailed
    );
    for token in ["source_missing", "no_entry"] {
        assert!(
            final_report.miss_reasons.contains(&token.to_owned()),
            "diagnosed: {:?}",
            final_report.miss_reasons
        );
    }
    Ok(())
}

/// The response publishes the exact plan/matrix pair used by consumers.
#[test]
fn publish_plan_files_writes_artifact_pair() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let dir = TempDir::new()?;
    let run = publish_plan_files(&response, &dir.path().join("velnor"))?;
    assert_eq!(run, dir.path().join("velnor").join("local"));
    let plan_text = fs::read_to_string(run.join("plan.json"))?;
    let matrix_text = fs::read_to_string(run.join("matrix.json"))?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(plan_text, canonical_json_str(&value["plan"])?);
    assert_eq!(matrix_text, canonical_json_str(&value["matrix"])?);
    assert!(matrix_text.starts_with("{\"include\":"), "{matrix_text}");
    let err = err_of(
        publish_plan_files(&response, &dir.path().join("velnor")),
        "second publish refused",
    )?;
    assert!(err.to_string().contains("plan_artifact_exists"), "{err}");
    assert!(
        err_of(
            publish_plan_files("not json", dir.path()),
            "garbage refused"
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn merge_verdict_mapping() -> TestResult {
    for (status, passed) in [
        ("passed", true),
        ("no_work", true),
        ("failed", false),
        ("cancelled", false),
        ("blocked", false),
        ("not_run", false),
        ("planning_failed", false),
    ] {
        let response = format!(r#"{{"schema":1,"status":"{status}"}}"#);
        assert_eq!(merge_passed(&response)?, passed, "{status}");
    }
    assert!(err_of(merge_passed("not json"), "verdict rejects garbage").is_ok());
    Ok(())
}
