//! Event-time `merge-v1` JSON entrypoint (schema 1).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_actions_contract::{
    CandidateReport, FinalCounts, FinalReport, FinalStatus, MatrixEntry, MatrixReport,
    ObligationDecision, Plan, PlanMatrix, RequiredJobResult, canonical_json_bytes,
    final_report_id_for_run, validate_run_key,
};

use crate::OrchestratorError;
use crate::cover::{Fold, Signals, cover_entry, partition_reports};
use crate::internal::{SCHEMA, check_schema, internal, internal_contract};

/// `merge-v1` request: plan, matrix bytes, reports, jobs, and candidate.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MergeRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Run key.
    pub(crate) run_key: String,
    /// Validated plan.
    plan: Plan,
    /// `matrix.json` content; must agree with the plan matrix.
    matrix: PlanMatrix,
    /// Matrix reports to aggregate.
    pub(crate) matrix_reports: Vec<MatrixReport>,
    /// Required non-matrix job results.
    #[serde(default)]
    required_jobs: Vec<RequiredJobResult>,
    /// Candidate report when candidate validation ran.
    #[serde(default)]
    candidate: Option<CandidateReport>,
}

/// Aggregate matrix reports into the final gate report (schema-1 JSON).
///
/// Structural mismatches yield a `planning_failed` final report; only
/// malformed requests fail outright. Consumes the emitted plan only and
/// never rediscovers repository state.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests and
/// plan, candidate, or final-report validation failures.
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
    request
        .required_jobs
        .sort_by(|left, right| left.job_id.cmp(&right.job_id));
    for job in &request.required_jobs {
        if job.job_id.trim().is_empty() || job.conclusion.trim().is_empty() {
            return Err(internal("malformed_required_job"));
        }
    }
    if let Some(candidate) = &request.candidate {
        candidate.validate().map_err(internal_contract)?;
        if candidate.run_key != request.run_key {
            return Err(internal("candidate_run_mismatch"));
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
    let mut signals = Signals::default();
    check_agreement(request, &mut signals)?;
    check_plan_shape(request, &mut signals);
    let entries = plan_entries(request);
    let obligations = plan_digests(request);
    let partition = partition_reports(request, &entries, &mut signals);
    let mut fold = Fold::default();
    let mut seen_task_reports = BTreeSet::new();
    let mut downloaded = Vec::new();
    let mut uncovered = 0u32;
    for entry in &request.plan.matrix.include {
        let Some(report) = partition.valid.get(entry.report_id.as_str()) else {
            uncovered += 1;
            signals.not_run = true;
            continue;
        };
        if cover_entry(
            request,
            entry,
            report,
            &obligations,
            &mut seen_task_reports,
            &mut fold,
            &mut signals,
        )? {
            downloaded.push(entry.artifact_id.clone());
        } else {
            uncovered += 1;
        }
    }
    fold_jobs(&request.required_jobs, &mut signals);
    fold_candidate(request.candidate.as_ref(), &mut signals);
    downloaded.sort();
    let status = decide(&signals, &request.plan);
    Ok(FinalReport {
        schema: SCHEMA,
        report_id: final_report_id_for_run(&request.run_key).map_err(internal_contract)?,
        run_key: request.run_key.clone(),
        plan_id: request.plan.plan_id.clone(),
        expected_report_ids: entries.into_keys().map(str::to_owned).collect(),
        downloaded_artifact_ids: downloaded,
        required_job_results: request.required_jobs.clone(),
        status,
        counts: FinalCounts {
            selected: u32::try_from(request.plan.task_ids.len()).unwrap_or(u32::MAX),
            reused: fold.reused,
            executed: fold.executed,
            empty_partition: fold.empty_partition,
            covered: covered_count(request),
            failed: fold.failed,
            cancelled: fold.cancelled,
            blocked: fold.blocked,
            not_run: uncovered + partition.malformed + partition.duplicates,
        },
    })
}

/// Check 1: `matrix.json` agrees byte-for-byte with the plan matrix.
fn check_agreement(request: &MergeRequest, signals: &mut Signals) -> Result<(), OrchestratorError> {
    let plan_bytes = canonical_json_bytes(&request.plan.matrix).map_err(internal_contract)?;
    let matrix_bytes = canonical_json_bytes(&request.matrix).map_err(internal_contract)?;
    if plan_bytes != matrix_bytes {
        signals.planning_failed = true;
    }
    Ok(())
}

/// The no-work decision needs obligations and task IDs to agree.
fn check_plan_shape(request: &MergeRequest, signals: &mut Signals) {
    if request.plan.obligations.is_empty() != request.plan.task_ids.is_empty() {
        signals.planning_failed = true;
    }
}

/// Expected entries keyed by report ID, sorted.
fn plan_entries(request: &MergeRequest) -> BTreeMap<&str, &MatrixEntry> {
    request
        .plan
        .matrix
        .include
        .iter()
        .map(|entry| (entry.report_id.as_str(), entry))
        .collect()
}

/// Obligation task digests keyed by task ID.
fn plan_digests(request: &MergeRequest) -> BTreeMap<&str, &str> {
    request
        .plan
        .obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect()
}
/// Fold required job conclusions; skipped is never success.
fn fold_jobs(jobs: &[RequiredJobResult], signals: &mut Signals) {
    for job in jobs {
        match job.conclusion.as_str() {
            "success" => {}
            "cancelled" => signals.cancelled = true,
            "skipped" | "neutral" => signals.not_run = true,
            _ => signals.failed = true,
        }
    }
}

/// Check 4: fold the candidate conclusion when validation ran.
fn fold_candidate(candidate: Option<&CandidateReport>, signals: &mut Signals) {
    match candidate.map(|report| report.status) {
        None | Some(velnor_actions_contract::CandidateStatus::Passed) => {}
        Some(velnor_actions_contract::CandidateStatus::Failed) => signals.failed = true,
        Some(velnor_actions_contract::CandidateStatus::Cancelled) => signals.cancelled = true,
    }
}

/// Check 5: precedence over collected signals, then pass or no-work.
fn decide(signals: &Signals, plan: &Plan) -> FinalStatus {
    if signals.planning_failed {
        FinalStatus::PlanningFailed
    } else if signals.failed {
        FinalStatus::Failed
    } else if signals.cancelled {
        FinalStatus::Cancelled
    } else if signals.not_run {
        FinalStatus::NotRun
    } else if plan.task_ids.is_empty() && plan.obligations.is_empty() {
        FinalStatus::NoWork
    } else {
        FinalStatus::Passed
    }
}

/// Obligations covered without execution.
fn covered_count(request: &MergeRequest) -> u32 {
    u32::try_from(
        request
            .plan
            .obligations
            .iter()
            .filter(|obligation| obligation.decision != ObligationDecision::Execute)
            .count(),
    )
    .unwrap_or(u32::MAX)
}
