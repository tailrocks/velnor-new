//! Event-time `merge-v1` JSON entrypoint (schema 1).

// Inventory checks live beside the merge so `lib.rs` stays untouched.
#[path = "required_evidence.rs"]
pub(crate) mod required_evidence;

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_actions_contract::{
    CandidateReport, ExecuteTaskRef, FinalCounts, FinalReport, FinalStatus, MatrixEntry,
    MatrixReport, ObligationDecision, Plan, PlanMatrix, RequiredJobResult, canonical_json_bytes,
    final_report_id_for_run, validate_run_key,
};

pub(crate) use self::required_evidence::BaselineManifest;
use self::required_evidence::{
    check_required_evidence, diagnostic_without_plan, fold_candidate, fold_jobs,
    reported_job_results,
};
use crate::OrchestratorError;
use crate::cover::shard::{ResourceLimits, ShardProof, check_entry_shards, validate_budgets};
use crate::cover::{
    CoverSinks, Fold, Signals, cover_entry, partition_reports, revalidate_coverage,
};
use crate::internal::{SCHEMA, check_schema, internal_contract};

/// `merge-v1` request: plan, matrix bytes, reports, jobs, and candidate.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MergeRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Run key.
    pub(crate) run_key: String,
    /// Validated plan; absent when the plan artifact never landed.
    #[serde(default)]
    plan: Option<Plan>,
    /// `matrix.json` content; must agree with the plan matrix.
    #[serde(default)]
    matrix: Option<PlanMatrix>,
    /// Matrix reports to aggregate.
    pub(crate) matrix_reports: Vec<MatrixReport>,
    /// Declared validator inventory from the workflow `needs` channel.
    pub(crate) required_job_ids: Vec<String>,
    /// Observed validator conclusions covering the inventory exactly.
    pub(crate) required_jobs: Vec<RequiredJobResult>,
    /// Assembly failure details; every entry fails the verdict.
    #[serde(default)]
    pub(crate) assembly_errors: Vec<String>,
    /// Candidate report when candidate validation ran.
    #[serde(default)]
    candidate: Option<CandidateReport>,
    /// Trusted baseline manifest for coverage revalidation.
    #[serde(default)]
    baseline_manifest: Option<BaselineManifest>,
    /// Shard proofs for partitioned test entries.
    #[serde(default)]
    shard_proofs: Vec<ShardProof>,
    /// Configured resource limits revalidated here.
    #[serde(default)]
    limits: Option<ResourceLimits>,
    /// Sequential-reference obligation set.
    #[serde(default)]
    reference_task_ids: Option<Vec<String>>,
}

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
pub fn merge_internal(request_json: &str) -> Result<String, OrchestratorError> {
    let mut request: MergeRequest =
        serde_json::from_str(request_json).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
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
    let final_report = build_final(&request, plan)?;
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
        .all(|job| !job.job_id.trim().is_empty() && !job.conclusion.trim().is_empty());
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
    candidate_failure(request)
}

/// Candidate evidence failure token, if any.
///
/// Qualification accepts any event, so a required untrusted PR
/// candidate qualifies here without promotion rights; promotion stays
/// with the protected release job. The proof must bind the planned head.
fn candidate_failure(request: &MergeRequest) -> Option<String> {
    let candidate = request.candidate.as_ref()?;
    if candidate.validate().is_err() {
        return Some("cache_corrupt".to_owned());
    }
    if candidate.run_key != request.run_key {
        return Some("trust_scope_mismatch".to_owned());
    }
    let bound = request
        .plan
        .as_ref()
        .is_none_or(|plan| candidate.source_commit == plan.head);
    (!bound).then_some("input_digest_mismatch".to_owned())
}

/// Aggregate one final report from validated plan plus reports.
fn build_final(request: &MergeRequest, plan: &Plan) -> Result<FinalReport, OrchestratorError> {
    let mut signals = Signals::default();
    let mut miss_reasons = BTreeSet::new();
    check_agreement(
        request.matrix.as_ref(),
        plan,
        &mut signals,
        &mut miss_reasons,
    )?;
    check_plan_shape(plan, &mut signals, &mut miss_reasons);
    check_plan_evidence(plan, request, &mut signals, &mut miss_reasons);
    check_execute_inventory(plan, &mut signals, &mut miss_reasons);
    let entries = plan_entries(plan);
    let obligations = plan_digests(plan);
    let partition = partition_reports(request, &entries, &mut signals, &mut miss_reasons);
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
        if shards_failed(entry, report.empty_partition, plan, request) {
            signals.planning_failed = true;
            uncovered += 1;
            continue;
        }
        let mut sinks = CoverSinks {
            seen_task_reports: &mut seen_task_reports,
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
    fold_jobs(&request.required_jobs, &mut signals);
    fold_candidate(request.candidate.as_ref(), &mut signals);
    downloaded.sort();
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
            not_run: uncovered + partition.malformed + partition.duplicates,
        },
        miss_reasons: miss_reasons.into_iter().collect(),
    })
}

/// Check 1: `matrix.json` agrees with the plan matrix (WF-4.16).
fn check_agreement(
    matrix: Option<&PlanMatrix>,
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    let Some(matrix) = matrix else {
        signals.planning_failed = true;
        miss_reasons.insert("source_missing".to_owned());
        return Ok(());
    };
    let matrix_bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    if velnor_actions_contract::check_matrix_agreement(&plan.matrix, &matrix_bytes).is_err() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    Ok(())
}

/// The no-work decision needs obligations and task IDs to agree.
fn check_plan_shape(plan: &Plan, signals: &mut Signals, miss_reasons: &mut BTreeSet<String>) {
    if plan.obligations.is_empty() != plan.task_ids.is_empty() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
}

/// Every Execute obligation needs a matrix leg; hollow plans fail closed.
///
/// Coverage walks `matrix.include`, so an Execute obligation without a
/// leg would pass with zero task evidence. Baseline-covered and
/// cache-reused dispositions need no leg. The match stays exhaustive so
/// a future decision variant fails to compile here instead of slipping
/// through unchecked.
fn check_execute_inventory(
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let mut leg_tasks = BTreeSet::new();
    for entry in &plan.matrix.include {
        for task_ref in entry.execute_task_ids.tasks.values() {
            let ids: &[String] = match task_ref {
                ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
                ExecuteTaskRef::Shards(ids) => ids.as_slice(),
            };
            // Leg IDs match obligation IDs verbatim: sharded obligations
            // carry their full `/shard-N-of-M` IDs, never the bare base.
            for id in ids {
                leg_tasks.insert(id.as_str());
            }
        }
    }
    let mut hollow = false;
    for obligation in &plan.obligations {
        match obligation.decision {
            ObligationDecision::Execute => {
                hollow = hollow || !leg_tasks.contains(obligation.task_id.as_str());
            }
            ObligationDecision::ReusedFromTaskCache
            | ObligationDecision::CoveredByTrustedBaseline => {}
        }
    }
    if hollow {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
    }
}

/// Expected entries keyed by report ID, sorted.
fn plan_entries(plan: &Plan) -> BTreeMap<&str, &MatrixEntry> {
    plan.matrix
        .include
        .iter()
        .map(|entry| (entry.report_id.as_str(), entry))
        .collect()
}

/// Compare the planned obligation set with the sequential reference.
fn reference_matches(reference: &[String], planned: &[String]) -> bool {
    let mut left = reference.to_vec();
    let mut right = planned.to_vec();
    left.sort();
    right.sort();
    left == right
}

/// Revalidate planner coverage, limits, and reference obligations.
fn check_plan_evidence(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    revalidate_coverage(
        plan,
        request.baseline_manifest.as_ref(),
        signals,
        miss_reasons,
    );
    if request
        .limits
        .as_ref()
        .is_some_and(|limits| validate_budgets(limits).is_err())
    {
        signals.planning_failed = true;
    }
    if request
        .reference_task_ids
        .as_ref()
        .is_some_and(|reference| !reference_matches(reference, &plan.task_ids))
    {
        signals.planning_failed = true;
    }
}

/// True when an entry's shard proofs fail validation.
fn shards_failed(entry: &MatrixEntry, empty: u32, plan: &Plan, request: &MergeRequest) -> bool {
    let bases = sharded_bases(entry);
    if bases.is_empty() {
        return false;
    }
    let mut inputs = BTreeMap::new();
    for ob in &plan.obligations {
        inputs.insert(ob.task_id.clone(), ob.input_digest.clone());
    }
    check_entry_shards(&bases, empty, &request.shard_proofs, &inputs).is_err()
}

/// Base task IDs carrying shard suffixes in one entry.
fn sharded_bases(entry: &MatrixEntry) -> BTreeSet<String> {
    let mut bases = BTreeSet::new();
    for task_ref in entry.execute_task_ids.tasks.values() {
        let ids = match task_ref {
            velnor_actions_contract::ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
            velnor_actions_contract::ExecuteTaskRef::Shards(ids) => ids.as_slice(),
        };
        for id in ids {
            if let Some((base, _)) = id.split_once("/shard-") {
                bases.insert(base.to_owned());
            }
        }
    }
    bases
}

/// Obligation task digests keyed by task ID.
fn plan_digests(plan: &Plan) -> BTreeMap<&str, &str> {
    plan.obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect()
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
