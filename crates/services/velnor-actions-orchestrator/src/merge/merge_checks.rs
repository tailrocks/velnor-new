//! `merge-v1` plan checks: matrix agreement, plan shape, evidence
//! revalidation, Execute inventory, and shard-proof validation.
//!
//! Declared from `merge.rs` (`#[path]`, no `lib.rs` edit) beside the
//! inventory checks in `required_evidence.rs`.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{canonical_json_bytes, task_report_id_for_task};
use velnor_actions_contract_workflow::{
    ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanMatrix, TaskReport,
};

use super::MergeRequest;
use crate::cover::Signals;
use crate::cover::revalidate_coverage;
use crate::cover::shard::{check_entry_shards, validate_budgets};
use crate::internal::SCHEMA;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal_contract;

/// Check 1: `matrix.json` agrees with the plan matrix (WF-4.16).
pub(crate) fn check_agreement(
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
    if velnor_actions_contract_workflow::check_matrix_agreement(&plan.matrix, &matrix_bytes)
        .is_err()
    {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    Ok(())
}

/// The no-work decision needs obligations and task IDs to agree.
pub(crate) fn check_plan_shape(
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if plan.obligations.is_empty() != plan.task_ids.is_empty() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
}

/// Plan event/trust must match the merge-time actual event.
///
/// Trust was stamped at plan time and only checked for self-consistency,
/// so a forged plan artifact could claim `Push` with `Trusted` scope on
/// PR content and pass. The plan's event must equal the actual event
/// captured at merge assembly (same run, runner ground truth), and the
/// plan's trust must equal the canonical scope for that actual event;
/// every task file's event/trust pair must still equal the plan's.
/// Anything else fails closed with a scope token, never silently.
pub(crate) fn check_trust_coherence(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let coherent = match request.actual_event {
        Some(actual) => {
            plan.event == actual
                && plan.trust == velnor_actions_contract_workflow::trust_for_event(actual)
        }
        None => false,
    };
    if !coherent {
        signals.planning_failed = true;
        miss_reasons.insert("trust_scope_mismatch".to_owned());
    }
    for report in &request.task_reports {
        if report.event != plan.event || report.trust != plan.trust {
            signals.planning_failed = true;
            miss_reasons.insert("trust_scope_mismatch".to_owned());
            return;
        }
    }
}

/// Head-bound candidate attestation written by the candidate job.
///
/// The candidate job observes the plan head from its downloaded plan
/// artifact and embeds it as `commit`; the merge re-checks equality
/// against its own plan head, so a stale or cross-plan candidate
/// artifact fails closed instead of qualifying the wrong commit.
/// Tokens reuse the closed miss set: absent is `source_missing`,
/// mismatched is `trust_scope_mismatch`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct CandidateAttestation {
    /// Attestation schema; must be 1.
    schema: u32,
    /// Plan head observed by the candidate job.
    commit: String,
}

/// In candidate mode the attestation must bind the candidate to the plan head.
///
/// Candidate mode is the candidate job's presence in the required
/// inventory (the final gate needs it exactly when the workflow was
/// generated with candidate validation). Outside candidate mode there
/// is no attestation to check.
pub(crate) fn check_candidate_binding(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    use velnor_actions_workflow_jobs::context::CANDIDATE_JOB_ID;
    if !request
        .required_job_ids
        .iter()
        .any(|id| id == CANDIDATE_JOB_ID)
    {
        return;
    }
    let bound = request.candidate_attestation.as_ref().is_some_and(|att| {
        att.schema == SCHEMA && !att.commit.trim().is_empty() && att.commit == plan.head
    });
    if !bound {
        signals.planning_failed = true;
        let token = if request.candidate_attestation.is_none() {
            "source_missing"
        } else {
            "trust_scope_mismatch"
        };
        miss_reasons.insert(token.to_owned());
    }
}

/// Every Execute obligation needs a matrix leg; hollow plans fail closed.
///
/// Coverage walks `matrix.include`, so an Execute obligation without a
/// leg would pass with zero task evidence. Baseline-covered and
/// cache-reused dispositions need no leg. The match stays exhaustive so
/// a future decision variant fails to compile here instead of slipping
/// through unchecked.
pub(crate) fn check_execute_inventory(
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
pub(crate) fn plan_entries(plan: &Plan) -> BTreeMap<&str, &MatrixEntry> {
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
pub(crate) fn check_plan_evidence(
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
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    if request
        .reference_task_ids
        .as_ref()
        .is_some_and(|reference| !reference_matches(reference, &plan.task_ids))
    {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
}

/// True when an entry's shard proofs fail validation.
pub(crate) fn shards_failed(
    entry: &MatrixEntry,
    empty: u32,
    plan: &Plan,
    request: &MergeRequest,
) -> bool {
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
            velnor_actions_contract_workflow::ExecuteTaskRef::Single(id) => {
                std::slice::from_ref(id)
            }
            velnor_actions_contract_workflow::ExecuteTaskRef::Shards(ids) => ids.as_slice(),
        };
        for id in ids {
            if let Some((base, _, _)) = velnor_actions_contract::split_shard_suffix(id) {
                bases.insert(base.to_owned());
            }
        }
    }
    bases
}

/// Obligation task digests keyed by task ID.
pub(crate) fn plan_digests(plan: &Plan) -> BTreeMap<&str, &str> {
    plan.obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect()
}

/// Plan-derived per-task file expectation: report ID to task identity.
///
/// The expectation derives from the plan alone (leg task IDs plus
/// obligation digests), never from an aggregate summary, so a lying
/// `matrix-report.json` cannot shrink or redirect the file set. Legs
/// naming tasks without an obligation digest stay underivable here;
/// per-entry coverage fails them closed.
pub(crate) fn expected_task_reports(
    plan: &Plan,
    run_key: &str,
) -> BTreeMap<String, (String, String)> {
    let digests = plan_digests(plan);
    let mut expected = BTreeMap::new();
    for entry in &plan.matrix.include {
        for task_ref in entry.execute_task_ids.tasks.values() {
            let ids: &[String] = match task_ref {
                ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
                ExecuteTaskRef::Shards(ids) => ids.as_slice(),
            };
            for id in ids {
                let derived = digests.get(id.as_str()).and_then(|digest| {
                    task_report_id_for_task(run_key, &entry.matrix_key, digest).ok()
                });
                if let Some(report_id) = derived {
                    expected.insert(report_id, (id.clone(), entry.matrix_key.clone()));
                }
            }
        }
    }
    expected
}

/// Check-2 partition of embedded per-task reports.
pub(crate) struct TaskPartition<'a> {
    /// First valid report per expected task-report ID.
    pub(crate) valid: BTreeMap<&'a str, &'a TaskReport>,
    /// Reports failing validation or bound to another run.
    pub(crate) malformed: u32,
    /// Extra reports beyond the first per task-report ID.
    pub(crate) duplicates: u32,
}

/// Check 2 for task files: first valid report per expected ID exactly.
///
/// Mirrors the matrix partition: malformed and duplicate files are
/// `not_run` (never success); valid files outside the plan-derived
/// expectation corrupt the set. Missing files surface per entry.
pub(crate) fn partition_task_reports<'a>(
    request: &'a MergeRequest,
    expected: &BTreeMap<String, (String, String)>,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> TaskPartition<'a> {
    let mut valid = BTreeMap::new();
    let mut malformed = 0u32;
    let mut duplicates = 0u32;
    for report in &request.task_reports {
        if report.run_key != request.run_key {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("trust_scope_mismatch".to_owned());
            continue;
        }
        if report.validate().is_err() {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        if valid.contains_key(report.task_report_id.as_str()) {
            duplicates += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        if !expected.contains_key(&report.task_report_id) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        valid.insert(report.task_report_id.as_str(), report);
    }
    TaskPartition {
        valid,
        malformed,
        duplicates,
    }
}
