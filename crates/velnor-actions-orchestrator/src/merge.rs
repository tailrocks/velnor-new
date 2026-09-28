//! Event-time `merge-v1` JSON entrypoint (schema 1).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    CandidateReport, FinalCounts, FinalReport, FinalStatus, MatrixEntry, MatrixReport,
    ObligationDecision, Plan, PlanMatrix, RequiredJobResult, canonical_json_bytes, digest_b3,
    final_report_id_for_run, validate_run_key,
};

use crate::OrchestratorError;
use crate::cover::shard::{ResourceLimits, ShardProof, check_entry_shards, validate_budgets};
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

/// One trusted-baseline task proof: identities plus provenance run IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BaselineTaskEntry {
    /// Covered task ID.
    pub(crate) task_id: String,
    /// Covered task digest.
    pub(crate) task_digest: String,
    /// Covered input digest.
    pub(crate) input_digest: String,
    /// Original direct-execution proof run.
    pub(crate) proof_run_id: u64,
    /// Carrying run that revalidated the proof.
    pub(crate) observed_run_id: u64,
}

/// Trusted `baseline.json`: minimum shape plus artifact binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BaselineManifest {
    /// Manifest schema; must be 1.
    pub(crate) schema: u32,
    /// Repository identity digest.
    pub(crate) repository_id: String,
    /// Exact trusted source commit.
    pub(crate) source_commit: String,
    /// Protected ref under test.
    #[serde(rename = "ref")]
    pub ref_: String,
    /// Protected event; must be `push`.
    pub(crate) event: String,
    /// Protected workflow ref.
    pub(crate) workflow_ref: String,
    /// Proof run ID.
    pub(crate) run_id: u64,
    /// Proof run attempt.
    pub(crate) run_attempt: u64,
    /// Final result; must be `passed`.
    pub(crate) final_status: String,
    /// Generator version.
    pub(crate) generator_version: String,
    /// Generator SHA-256.
    pub generator_sha256: String,
    /// Compatibility identity.
    pub(crate) compatibility_id: String,
    /// Numeric baseline artifact ID.
    pub(crate) artifact_id: u64,
    /// Derived baseline artifact name.
    pub(crate) artifact_name: String,
    /// Per-task proofs.
    pub(crate) tasks: Vec<BaselineTaskEntry>,
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
    check_plan_evidence(request, &mut signals);
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
        if shards_failed(entry, report.empty_partition, request) {
            signals.planning_failed = true;
            uncovered += 1;
            continue;
        }
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

/// Revalidate planner coverage claims against the trusted manifest.
pub(crate) fn revalidate_coverage(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
    signals: &mut Signals,
) {
    let covered: Vec<&velnor_actions_contract::PlanObligation> = plan
        .obligations
        .iter()
        .filter(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline)
        .collect();
    if covered.is_empty() {
        return;
    }
    let Some(manifest) = manifest else {
        signals.planning_failed = true;
        return;
    };
    for obligation in covered {
        let Some(proof) = &obligation.baseline_proof else {
            signals.planning_failed = true;
            continue;
        };
        let hit = manifest
            .tasks
            .iter()
            .find(|task| task.task_id == obligation.task_id);
        match hit {
            Some(task)
                if task.task_digest == obligation.task_digest
                    && task.input_digest == obligation.input_digest
                    && proof.run_id == task.proof_run_id
                    && proof.artifact_name == manifest.artifact_name
                    && proof.manifest_digest
                        == digest_b3(&canonical_json_bytes(manifest).unwrap_or_default()) => {}
            _ => signals.planning_failed = true,
        }
    }
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

/// Compare the planned obligation set with the sequential reference.
fn reference_matches(reference: &[String], planned: &[String]) -> bool {
    let mut left = reference.to_vec();
    let mut right = planned.to_vec();
    left.sort();
    right.sort();
    left == right
}

/// Revalidate planner coverage, limits, and reference obligations.
fn check_plan_evidence(request: &MergeRequest, signals: &mut Signals) {
    revalidate_coverage(&request.plan, request.baseline_manifest.as_ref(), signals);
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
        .is_some_and(|reference| !reference_matches(reference, &request.plan.task_ids))
    {
        signals.planning_failed = true;
    }
}

/// True when an entry's shard proofs fail validation.
fn shards_failed(entry: &MatrixEntry, empty: u32, request: &MergeRequest) -> bool {
    let bases = sharded_bases(entry);
    if bases.is_empty() {
        return false;
    }
    let mut inputs = BTreeMap::new();
    for ob in &request.plan.obligations {
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
