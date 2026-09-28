//! Event-time `merge-v1` JSON entrypoint (schema 1).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_actions_contract::{
    FinalCounts, FinalReport, FinalStatus, MatrixReport, MatrixStatus, Plan, RequiredJobResult,
    TaskStatus, final_report_id_for_run, validate_run_key,
};

use crate::OrchestratorError;
use crate::internal::{SCHEMA, check_schema, internal, internal_contract};

/// `merge-v1` request: plan plus matrix reports and required jobs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Run key.
    run_key: String,
    /// Validated plan.
    plan: Plan,
    /// Matrix reports to aggregate.
    matrix_reports: Vec<MatrixReport>,
    /// Required non-matrix job results.
    #[serde(default)]
    required_jobs: Vec<RequiredJobResult>,
}

/// Aggregate matrix reports into the final gate report (schema-1 JSON).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests and
/// plan, report, or final-report validation failures.
pub fn merge_internal(request_json: &str) -> Result<String, OrchestratorError> {
    let mut request: MergeRequest =
        serde_json::from_str(request_json).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
    check_schema(request.schema)?;
    validate_run_key(&request.run_key).map_err(internal_contract)?;
    request.plan.validate().map_err(internal_contract)?;
    if request.plan.run_key != request.run_key {
        return Err(internal("plan_run_mismatch"));
    }
    for report in &request.matrix_reports {
        report.validate().map_err(internal_contract)?;
        if report.run_key != request.run_key {
            return Err(internal("report_run_mismatch"));
        }
    }
    request
        .required_jobs
        .sort_by(|left, right| left.job_id.cmp(&right.job_id));
    for job in &request.required_jobs {
        if job.job_id.trim().is_empty() || job.conclusion.trim().is_empty() {
            return Err(internal("malformed_required_job"));
        }
    }
    let final_report = build_final(&request)?;
    final_report.validate().map_err(internal_contract)?;
    serde_json::to_string(&final_report).map_err(|err| OrchestratorError::Internal {
        problem: format!("response_encode:{err}"),
    })
}

/// Aggregate one final report from validated plan plus reports.
fn build_final(request: &MergeRequest) -> Result<FinalReport, OrchestratorError> {
    let expected: BTreeSet<&str> = request
        .plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.report_id.as_str())
        .collect();
    let present: BTreeMap<&str, &MatrixReport> = request
        .matrix_reports
        .iter()
        .map(|report| (report.report_id.as_str(), report))
        .collect();
    let mut downloaded: Vec<String> = request
        .plan
        .matrix
        .include
        .iter()
        .filter(|entry| present.contains_key(entry.report_id.as_str()))
        .map(|entry| entry.artifact_id.clone())
        .collect();
    downloaded.sort();
    let mut counts = FinalCounts {
        selected: u32::try_from(request.plan.task_ids.len()).unwrap_or(u32::MAX),
        reused: 0,
        executed: 0,
        empty_partition: 0,
        covered: 0,
        failed: 0,
        cancelled: 0,
        blocked: 0,
        not_run: 0,
    };
    let mut failed = false;
    let mut cancelled = false;
    for report in &request.matrix_reports {
        fold_report(report, &mut counts, &mut failed, &mut cancelled);
    }
    for job in &request.required_jobs {
        match job.conclusion.as_str() {
            "success" | "skipped" => {}
            "cancelled" => cancelled = true,
            _ => failed = true,
        }
    }
    let missing = expected.iter().any(|id| !present.contains_key(*id));
    let status = if failed {
        FinalStatus::Failed
    } else if cancelled {
        FinalStatus::Cancelled
    } else if missing {
        FinalStatus::NotRun
    } else if request.plan.task_ids.is_empty() {
        FinalStatus::NoWork
    } else {
        FinalStatus::Passed
    };
    Ok(FinalReport {
        schema: SCHEMA,
        report_id: final_report_id_for_run(&request.run_key).map_err(internal_contract)?,
        run_key: request.run_key.clone(),
        plan_id: request.plan.plan_id.clone(),
        expected_report_ids: expected.into_iter().map(str::to_owned).collect(),
        downloaded_artifact_ids: downloaded,
        required_job_results: request.required_jobs.clone(),
        status,
        counts,
    })
}

/// Fold one matrix report into counts and failure flags.
fn fold_report(
    report: &MatrixReport,
    counts: &mut FinalCounts,
    failed: &mut bool,
    cancelled: &mut bool,
) {
    match report.status {
        MatrixStatus::Failed => *failed = true,
        MatrixStatus::Cancelled => *cancelled = true,
        MatrixStatus::Passed | MatrixStatus::NotRun => {}
    }
    for task in &report.tasks {
        match task.status {
            TaskStatus::Reused => counts.reused += 1,
            TaskStatus::Executed => counts.executed += 1,
            TaskStatus::EmptyPartition => counts.empty_partition += 1,
            TaskStatus::Failed => {
                counts.failed += 1;
                *failed = true;
            }
            TaskStatus::Cancelled => {
                counts.cancelled += 1;
                *cancelled = true;
            }
            TaskStatus::NotSelected => {}
        }
    }
}
