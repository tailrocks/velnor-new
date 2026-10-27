// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "merge_checks.rs"]
pub(crate) mod merge_checks;
#[path = "merge_lenient.rs"]
mod merge_lenient;
#[path = "required_evidence.rs"]
pub(crate) mod required_evidence;

use std::collections::BTreeSet;

use serde::Deserialize;
use velnor_actions_contract::{
    FinalCounts, FinalReport, FinalStatus, MatrixReport, ObligationDecision, Plan, PlanMatrix,
    QualificationDispatch, RequiredJobResult, TaskReport, WorkflowEvent, final_report_id_for_run,
    parse_strict_json, validate_run_key,
};

use self::merge_checks::{
    CandidateAttestation, check_agreement, check_candidate_binding, check_execute_inventory,
    check_group_ledgers, check_plan_evidence, check_plan_shape, check_trust_coherence,
    expected_task_reports, partition_task_reports, plan_digests, plan_entries, shards_failed,
};
use self::required_evidence::{
    check_required_evidence, diagnostic_without_plan, reported_job_results,
};
use crate::OrchestratorError;
use crate::cover::shard::{ResourceLimits, ShardProof};
use crate::cover::{CoverSinks, Fold, Signals, cover_entry, partition_reports};
use crate::internal::{SCHEMA, check_schema, internal_contract};

#[path = "merge_context.rs"]
mod context;
pub use context::MergeContext;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MergeRequest {
    schema: u32,
    pub(crate) run_key: String,
    #[serde(default)]
    actual_event: Option<WorkflowEvent>,
    #[serde(default)]
    actual_qualification: Option<QualificationDispatch>,
    #[serde(default)]
    candidate_attestation: Option<CandidateAttestation>,
    #[serde(default)]
    plan: Option<Plan>,
    #[serde(default)]
    matrix: Option<PlanMatrix>,
    pub(crate) matrix_reports: Vec<MatrixReport>,
    #[serde(default)]
    pub(crate) task_reports: Vec<TaskReport>,
    #[serde(default)]
    pub(crate) check_proofs: Vec<serde_json::Value>,
    pub(crate) required_job_ids: Vec<String>,
    pub(crate) required_jobs: Vec<RequiredJobResult>,
    #[serde(default)]
    pub(crate) group_ledgers: Vec<serde_json::Value>,
    #[serde(default)]
    pub(crate) assembly_errors: Vec<String>,
    #[serde(default)]
    shard_proofs: Vec<ShardProof>,
    #[serde(default)]
    limits: Option<ResourceLimits>,
    #[serde(default)]
    reference_task_ids: Option<Vec<String>>,
}

pub fn merge_internal(request_json: &str) -> Result<String, OrchestratorError> {
    merge_internal_with_context(request_json, &MergeContext::capture_ambient())
}

pub fn merge_internal_with_context(
    request_json: &str,
    context: &MergeContext,
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
    let final_report = build_final(&request, plan, context)?;
    final_report.validate().map_err(internal_contract)?;
    encode_report(&final_report)
}

fn encode_report(report: &FinalReport) -> Result<String, OrchestratorError> {
    serde_json::to_string(report).map_err(|err| OrchestratorError::Internal {
        problem: format!("response_encode:{err}"),
    })
}

fn evidence_diagnostic(request: &MergeRequest) -> Result<Option<FinalReport>, OrchestratorError> {
    let Some(token) = evidence_failure(request) else {
        return Ok(None);
    };
    diagnostic_without_plan(request, BTreeSet::from([token])).map(Some)
}

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

fn build_final(
    request: &MergeRequest,
    plan: &Plan,
    context: &MergeContext,
) -> Result<FinalReport, OrchestratorError> {
    let mut signals = Signals::default();
    let mut miss_reasons = BTreeSet::new();
    check_final_inputs(request, plan, context, &mut signals, &mut miss_reasons)?;
    let entries = plan_entries(plan);
    let obligations = plan_digests(plan);
    let partition = partition_reports(request, &entries, &mut signals, &mut miss_reasons);
    let expected = expected_task_reports(plan, &request.run_key);
    let tasks = partition_task_reports(plan, request, &expected, &mut signals, &mut miss_reasons);
    let mut fold = Fold::default();
    let mut seen_task_reports = BTreeSet::new();
    let mut downloaded = Vec::new();
    let mut uncovered = 0u32;
    check_required_evidence(plan, request, &mut signals, &mut miss_reasons);
    check_group_ledgers(plan, request, &mut signals, &mut miss_reasons);
    for entry in &plan.matrix.include {
        let Some(report) = partition.valid.get(entry.report_id.as_str()) else {
            uncovered += 1;
            signals.not_run = true;
            miss_reasons.insert("no_entry".to_owned());
            continue;
        };
        if shards_failed(entry, report.empty_partition, plan, request) {
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
        if cover_entry(request, entry, report, &obligations, &mut sinks)? {
            downloaded.push(entry.artifact_id.clone());
        } else {
            uncovered += 1;
        }
    }
    crate::required_dispositions::fold_planned_jobs(
        &plan.jobs,
        &request.required_jobs,
        plan.event,
        &mut signals,
        &mut miss_reasons,
    );
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
            covered: decision_count(plan, ObligationDecision::ReusedFromTaskCache),
            not_affected: decision_count(plan, ObligationDecision::NotAffected),
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

fn check_final_inputs(
    request: &MergeRequest,
    plan: &Plan,
    context: &MergeContext,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    check_agreement(request.matrix.as_ref(), plan, signals, miss_reasons)?;
    check_plan_shape(plan, signals, miss_reasons);
    check_trust_coherence(plan, request, signals, miss_reasons);
    crate::required_dispositions::check_full_event_skips(
        plan,
        request.actual_event,
        signals,
        miss_reasons,
    );
    check_candidate_binding(plan, request, signals, miss_reasons);
    check_plan_evidence(plan, request, signals, miss_reasons);
    check_execute_inventory(plan, signals, miss_reasons);
    if workspace_validation_required(plan, context) {
        match context.workspace() {
            Some(root) if !root.as_os_str().is_empty() => {
                if crate::internal_plan::native_homebrew_checks::validate_required_plan(root, plan)
                    .is_err()
                {
                    signals.planning_failed = true;
                    miss_reasons.insert("cache_corrupt".to_owned());
                }
                if !crate::check_evidence::gate::validate_proofs(
                    plan,
                    &request.task_reports,
                    &request.check_proofs,
                    Some(root),
                ) {
                    signals.planning_failed = true;
                    miss_reasons.insert("cache_corrupt".to_owned());
                }
            }
            _ => {
                signals.planning_failed = true;
                miss_reasons.insert("cache_corrupt".to_owned());
            }
        }
    } else if !crate::check_evidence::gate::validate_proofs(
        plan,
        &request.task_reports,
        &request.check_proofs,
        None,
    ) {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    Ok(())
}

pub(super) fn workspace_validation_required(plan: &Plan, context: &MergeContext) -> bool {
    context.repository_present()
        || plan
            .matrix
            .include
            .iter()
            .any(|entry| entry.stack_id == "native")
}

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

fn decision_count(plan: &Plan, decision: ObligationDecision) -> u32 {
    let count = plan
        .obligations
        .iter()
        .filter(|obligation| obligation.decision == decision)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}
