//! Event-time `merge-v1` JSON entrypoint (schema 1).

// Inventory and plan checks live beside the merge entrypoint.
pub mod merge_checks;
pub mod merge_lenient;
pub mod required_evidence;

use std::collections::BTreeSet;

use velnor_actions_contract::{parse_strict_json, validate_run_key};
use velnor_actions_contract_workflow::{
    FinalCounts, FinalReport, FinalStatus, ObligationDecision, Plan, final_report_id_for_run,
};

use self::merge_checks::{
    check_agreement, check_candidate_binding, check_execute_inventory, check_plan_evidence,
    check_plan_shape, check_trust_coherence, expected_task_reports, partition_task_reports,
    plan_digests, plan_entries, shards_failed,
};
pub use self::required_evidence::BaselineManifest;
use self::required_evidence::{
    check_required_evidence, diagnostic_without_plan, fold_jobs, reported_job_results,
};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal_contract;
use velnor_actions_orchestrator_merge_ports::{
    CoverPort, CoverSinks, Fold, SCHEMA, Signals, check_schema,
};

pub use velnor_actions_orchestrator_merge_ports::MergeRequest;

/// Aggregate matrix reports into the final gate report (schema-1 JSON).
///
/// Only the request envelope (JSON shape, schema, run key) fails
/// outright; every evidence failure yields a diagnostic `planning_failed`
/// verdict with closed failure tokens. A missing plan still yields a
/// verdict instead of an error. Consumes the emitted plan only and never
/// rediscovers repository state.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests and
/// response-encoding failures.
pub fn merge_internal_with(
    cover: &dyn CoverPort,
    request_json: &str,
) -> Result<String, OrchestratorError> {
    let envelope = parse_strict_json(request_json).map_err(internal_contract)?;
    let mut request: MergeRequest = match serde_json::from_value(envelope.clone()) {
        Ok(request) => request,
        Err(strict_err) => merge_lenient::lenient_request(&envelope).ok_or_else(|| {
            OrchestratorError::Internal {
                problem: format!("malformed_request:{strict_err}"),
            }
        })?,
    };
    check_schema(request.schema)?;
    validate_run_key(&request.run_key).map_err(internal_contract)?;
    request
        .required_jobs
        .sort_by(|left, right| left.job_id.cmp(&right.job_id));
    if let Some(report) = evidence_diagnostic(&request)? {
        return encode_report(&report);
    }
    let Some(plan) = request.plan.as_ref() else {
        let mut tokens = BTreeSet::new();
        if request.assembly_errors.is_empty() {
            tokens.insert("source_missing".to_owned());
        }
        return encode_report(&diagnostic_without_plan(&request, tokens)?);
    };
    let final_report = build_final(cover, &request, plan)?;
    final_report.validate().map_err(internal_contract)?;
    encode_report(&final_report)
}

/// Encode one final report as JSON.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures.
fn encode_report(report: &FinalReport) -> Result<String, OrchestratorError> {
    serde_json::to_string(report).map_err(|err| OrchestratorError::Internal {
        problem: format!("response_encode:{err}"),
    })
}

/// Diagnostic verdict for corrupt evidence; `None` when usable.
fn evidence_diagnostic(request: &MergeRequest) -> Result<Option<FinalReport>, OrchestratorError> {
    let Some(token) = evidence_failure(request) else {
        return Ok(None);
    };
    diagnostic_without_plan(request, BTreeSet::from([token])).map(Some)
}

/// First evidence failure token, if any.
///
/// Shape failures corrupt the set; wrong-run evidence mistrusts scope.
/// `None` means the evidence is usable, not that it passes.
fn evidence_failure(request: &MergeRequest) -> Option<String> {
    let jobs_ok = request
        .required_jobs
        .iter()
        .all(|job| !job.job_id.trim().is_empty());
    let inventory_ok = request
        .required_job_ids
        .iter()
        .all(|id| !id.trim().is_empty());
    if !jobs_ok || !inventory_ok {
        return Some("cache_corrupt".to_owned());
    }
    if let Some(plan) = request.plan.as_ref() {
        if plan.validate().is_err() {
            return Some("cache_corrupt".to_owned());
        }
        if plan.run_key != request.run_key {
            return Some("trust_scope_mismatch".to_owned());
        }
    }
    None
}

/// Aggregate one final report from validated plan plus reports.
fn build_final(
    cover: &dyn CoverPort,
    request: &MergeRequest,
    plan: &Plan,
) -> Result<FinalReport, OrchestratorError> {
    let mut signals = Signals::default();
    let mut miss_reasons = BTreeSet::new();
    check_final_inputs(cover, request, plan, &mut signals, &mut miss_reasons)?;
    let entries = plan_entries(plan);
    let obligations = plan_digests(plan);
    let partition = cover.partition_reports(request, &entries, &mut signals, &mut miss_reasons);
    let expected = expected_task_reports(plan, &request.run_key);
    let tasks = partition_task_reports(request, &expected, &mut signals, &mut miss_reasons);
    let mut fold = Fold::default();
    let mut seen_task_reports = BTreeSet::new();
    let mut downloaded = Vec::new();
    let mut uncovered = 0u32;
    check_required_evidence(plan, request, &mut signals, &mut miss_reasons);
    for entry in &plan.matrix.include {
        let Some(report) = partition.valid.get(entry.report_id.as_str()) else {
            uncovered += 1;
            signals.not_run = true;
            miss_reasons.insert("no_entry".to_owned());
            continue;
        };
        if shards_failed(cover, entry, report.empty_partition, plan, request) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            uncovered += 1;
            continue;
        }
        let mut sinks = CoverSinks {
            seen_task_reports: &mut seen_task_reports,
            task_files: &tasks.valid,
            fold: &mut fold,
            signals: &mut signals,
            miss_reasons: &mut miss_reasons,
        };
        if cover.cover_entry(request, entry, report, &obligations, &mut sinks)? {
            downloaded.push(entry.artifact_id.clone());
        } else {
            uncovered += 1;
        }
    }
    fold_jobs(&request.required_jobs, &mut signals);
    downloaded.sort();
    downloaded.dedup();
    let status = decide(&signals, plan);
    Ok(FinalReport {
        schema: SCHEMA,
        report_id: final_report_id_for_run(&request.run_key).map_err(internal_contract)?,
        run_key: request.run_key.clone(),
        plan_id: plan.plan_id.clone(),
        expected_report_ids: entries.into_keys().map(str::to_owned).collect(),
        downloaded_artifact_ids: downloaded,
        required_job_results: reported_job_results(request),
        status,
        counts: FinalCounts {
            selected: u32::try_from(plan.task_ids.len()).unwrap_or(u32::MAX),
            reused: fold.reused,
            executed: fold.executed,
            empty_partition: fold.empty_partition,
            covered: covered_count(plan),
            failed: fold.failed,
            cancelled: fold.cancelled,
            blocked: fold.blocked,
            not_run: uncovered
                + partition.malformed
                + partition.duplicates
                + tasks.malformed
                + tasks.duplicates,
        },
        miss_reasons: miss_reasons.into_iter().collect(),
    })
}

/// Validate final-fold inputs before collecting obligation evidence.
fn check_final_inputs(
    cover: &dyn CoverPort,
    request: &MergeRequest,
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    check_agreement(request.matrix.as_ref(), plan, signals, miss_reasons)?;
    check_plan_shape(plan, signals, miss_reasons);
    check_trust_coherence(plan, request, signals, miss_reasons);
    check_candidate_binding(plan, request, signals, miss_reasons);
    check_plan_evidence(cover, plan, request, signals, miss_reasons);
    check_execute_inventory(plan, signals, miss_reasons);
    if !velnor_actions_orchestrator_check_evidence::gate::validate_proofs(
        plan,
        &request.task_reports,
        &request.check_proofs,
    ) {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    Ok(())
}

/// Check 5: precedence over collected signals, then pass or no-work.
///
/// Documented precedence: `planning_failed` beats `failed` beats
/// `cancelled` beats `blocked` beats `not_run`; an obligation-free
/// plan is `no_work`, otherwise every signal clear is `passed`.
/// Blocked (`not_selected`) stays distinct from missing evidence:
/// collapsing it into `not_run` would hide a decided outcome behind
/// an evidence gap.
fn decide(signals: &Signals, plan: &Plan) -> FinalStatus {
    if signals.planning_failed {
        FinalStatus::PlanningFailed
    } else if signals.failed {
        FinalStatus::Failed
    } else if signals.cancelled {
        FinalStatus::Cancelled
    } else if signals.blocked {
        FinalStatus::Blocked
    } else if signals.not_run {
        FinalStatus::NotRun
    } else if plan.task_ids.is_empty() && plan.obligations.is_empty() {
        FinalStatus::NoWork
    } else {
        FinalStatus::Passed
    }
}

/// Obligations covered without execution.
///
/// The match stays exhaustive so a future decision variant fails to
/// compile here instead of silently joining one side of the count.
fn covered_count(plan: &Plan) -> u32 {
    let mut covered = 0usize;
    for obligation in &plan.obligations {
        match obligation.decision {
            ObligationDecision::Execute => {}
            ObligationDecision::ReusedFromTaskCache
            | ObligationDecision::CoveredByTrustedBaseline => covered += 1,
        }
    }
    u32::try_from(covered).unwrap_or(u32::MAX)
}
