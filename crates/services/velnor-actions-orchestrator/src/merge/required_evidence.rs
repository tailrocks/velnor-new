//! Closed required-evidence inventory for `merge-v1` (P01).
//!
//! The finalized workflow declares its required validators through the
//! `needs` channel (see `merge_request`); the plan declares its matrix
//! legs and obligation dispositions. Merge proves the full expected set
//! exists instead of judging supplied evidence alone: validator
//! conclusions must cover the declared inventory exactly (the candidate
//! job's conclusion is its qualification evidence when candidate mode
//! gates), and every non-execute disposition must carry verifiable
//! proof. Empty or partial evidence is `planning_failed`, never success.
//!
//! Diagnostics use the contract's closed `miss_reason` tokens only, so
//! every diagnostic report validates. Assembly details stay in the
//! request; the verdict maps each failure class to its token.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use velnor_actions_contract_workflow::{
    FinalReport, JobConclusion, ObligationDecision, Plan, RequiredJobResult,
};

use super::MergeRequest;
use crate::cover::Signals;
use velnor_actions_orchestrator_core::internal_contract;

/// One trusted-baseline task proof: identities plus provenance run IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BaselineTaskEntry {
    /// Covered task ID.
    pub(crate) task_id: String,
    /// Covered task digest.
    pub(crate) task_digest: String,
    /// Covered input digest.
    pub(crate) input_digest: String,
    /// Canonical digest over the task's complete input closure.
    ///
    /// Required with no default: entries recorded before closure binding
    /// (schema 1) fail deserialization instead of covering blindly.
    pub(crate) closure_digest: String,
    /// Original direct-execution proof run.
    pub(crate) proof_run_id: u64,
    /// Carrying run that revalidated the proof.
    pub(crate) observed_run_id: u64,
    /// External-data freshness (required for advisory kinds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) external_data: Option<crate::external_data::ExternalDataFreshness>,
    /// Structured task proof, when the publisher recorded one (PAR-5.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) proof: Option<velnor_actions_contract_workflow::ManifestTaskProof>,
}

/// Trusted `baseline.json`: minimum shape plus artifact binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BaselineManifest {
    /// Manifest schema; must be 2 (closure-bound entries).
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
    /// Manifest-assigned numeric fingerprint of the artifact name.
    pub(crate) artifact_id: u64,
    /// Derived baseline artifact name.
    pub(crate) artifact_name: String,
    /// Per-task proofs.
    pub(crate) tasks: Vec<BaselineTaskEntry>,
    /// Unix expiry; absent means the baseline never expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) expires_at_unix: Option<u64>,
}

/// Enforce the closed inventory: jobs and obligation proofs.
///
/// Assembly failures recorded in the request fail here too, so a
/// diagnostic verdict always explains the failed evidence class.
pub(crate) fn check_required_evidence(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    check_job_inventory(request, signals, miss_reasons);
    check_obligation_proofs(plan, signals, miss_reasons);
    if !request.assembly_errors.is_empty() {
        signals.planning_failed = true;
        miss_reasons.extend(assembly_tokens(&request.assembly_errors));
    }
}

/// Exact-set validator conclusions; an empty inventory never passes.
fn check_job_inventory(
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if request.required_job_ids.is_empty() {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
        return;
    }
    let expected: BTreeSet<&str> = request
        .required_job_ids
        .iter()
        .map(String::as_str)
        .collect();
    if expected.len() != request.required_job_ids.len() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    let mut seen = BTreeSet::new();
    for job in &request.required_jobs {
        if !seen.insert(job.job_id.as_str()) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
        if !expected.contains(job.job_id.as_str()) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
        }
    }
    if expected.iter().any(|id| !seen.contains(id)) {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
    }
}

/// Every non-execute disposition carries verifiable proof.
///
/// Baseline coverage revalidates against the manifest in
/// `revalidate_coverage`; task-cache reuse has no restore-proof channel
/// yet (P04), so any reuse decision fails closed here. The match stays
/// exhaustive so a future decision variant fails to compile here
/// instead of slipping through unchecked.
fn check_obligation_proofs(
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    for obligation in &plan.obligations {
        match obligation.decision {
            ObligationDecision::ReusedFromTaskCache => {
                signals.planning_failed = true;
                miss_reasons.insert("no_entry".to_owned());
            }
            ObligationDecision::Execute | ObligationDecision::CoveredByTrustedBaseline => {}
        }
    }
}

/// Fold required job conclusions; skipped is never success.
pub(crate) fn fold_jobs(jobs: &[RequiredJobResult], signals: &mut Signals) {
    for job in jobs {
        match job.conclusion {
            JobConclusion::Success => {}
            JobConclusion::Cancelled => signals.cancelled = true,
            JobConclusion::Skipped | JobConclusion::Neutral => signals.not_run = true,
            JobConclusion::Failure | JobConclusion::Missing => signals.failed = true,
        }
    }
}

/// Reported validator outcomes: observed results plus `missing` markers.
///
/// The final report mirrors the declared inventory exactly so consumers
/// see which validators never reported, not just the failures.
pub(crate) fn reported_job_results(request: &MergeRequest) -> Vec<RequiredJobResult> {
    let mut reported = request.required_jobs.clone();
    for id in &request.required_job_ids {
        if !reported.iter().any(|job| &job.job_id == id) {
            reported.push(RequiredJobResult {
                job_id: id.clone(),
                conclusion: JobConclusion::Missing,
            });
        }
    }
    reported.sort_by(|left, right| left.job_id.cmp(&right.job_id));
    reported
}

/// Diagnostic verdict when no usable plan exists (P01).
///
/// Missing, unparsable, and mismatched plans all land here with the
/// mapped failure tokens instead of dying without a report.
pub(crate) fn diagnostic_without_plan(
    request: &MergeRequest,
    tokens: BTreeSet<String>,
) -> Result<FinalReport, velnor_actions_orchestrator_core::OrchestratorError> {
    let mut report = FinalReport::without_plan(&request.run_key, reported_job_results(request))
        .map_err(internal_contract)?;
    let mut miss = assembly_tokens(&request.assembly_errors);
    miss.extend(tokens);
    report.miss_reasons = miss.into_iter().collect();
    report.validate().map_err(internal_contract)?;
    Ok(report)
}

/// Map assembly failure details to closed verdict tokens.
///
/// Missing channel declarations are `no_entry`; missing evidence files
/// are `source_missing`; every other assembly failure corrupts the
/// evidence set. Unknown details fail closed as corrupt, never silent.
fn assembly_tokens(errors: &[String]) -> BTreeSet<String> {
    errors
        .iter()
        .map(|error| {
            if error == "empty_needs" || error.starts_with("missing_needs") {
                "no_entry"
            } else if error.starts_with("missing_") {
                "source_missing"
            } else {
                "cache_corrupt"
            }
            .to_owned()
        })
        .collect()
}
