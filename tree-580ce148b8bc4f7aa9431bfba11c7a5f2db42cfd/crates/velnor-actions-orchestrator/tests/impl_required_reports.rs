//! Staged-report verdict tests: assembly tokens reach the merge verdict.

use serde_json::json;
use velnor_actions_contract::FinalStatus;
use velnor_actions_orchestrator::{assemble_merge_request, merge_internal};

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};

#[test]
fn unparsable_report_carries_to_verdict() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
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
    // Stage every leg, but corrupt the first report body: assembly must
    // record the unparsable token and the merge must carry it to a
    // planning_failed verdict with the corruption token.
    for (index, report) in reports.iter().enumerate() {
        let entry = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.report_id == report.report_id)
            .ok_or_else(|| std::io::Error::other("report without entry"))?;
        let leg = run.join("reports").join(&entry.artifact_id);
        std::fs::create_dir_all(&leg)?;
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
    value["required_job_ids"] = json!(["plan"]);
    value["required_jobs"] = json!([{"job_id": "plan", "conclusion": "success"}]);
    value["assembly_errors"] = serde_json::Value::Array(
        errors
            .into_iter()
            .filter(|e| e.as_str() != Some("missing_needs_channel"))
            .collect(),
    );
    let report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&value.to_string())?)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"cache_corrupt".to_owned()),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}
