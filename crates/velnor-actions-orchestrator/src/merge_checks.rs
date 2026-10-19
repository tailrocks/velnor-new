//! `merge-v1` plan checks: matrix agreement, plan shape, evidence
//! revalidation, Execute inventory, and shard-proof validation.
//!
//! Declared from `merge.rs` (`#[path]`, no `lib.rs` edit) beside the
//! inventory checks in `required_evidence.rs`.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    ExecuteTaskRef, ObligationDecision, Plan, PlanMatrix, canonical_json_bytes,
};

use super::MergeRequest;
use crate::OrchestratorError;
use crate::cover::Signals;
use crate::internal::{SCHEMA, internal_contract};

#[path = "merge_checks_evidence.rs"]
mod evidence;
pub(crate) use self::evidence::{
    check_plan_evidence, expected_task_reports, partition_task_reports, plan_digests, plan_entries,
    shards_failed,
};

#[cfg(test)]
#[path = "merge_qualification_tests.rs"]
mod qualification_tests;

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
    if velnor_actions_contract::check_matrix_agreement(&plan.matrix, &matrix_bytes).is_err() {
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
            plan.event == actual && plan.trust == velnor_actions_contract::trust_for_event(actual)
        }
        None => false,
    };
    let qualification_coherent = match plan.event {
        velnor_actions_contract::WorkflowEvent::Qualification => {
            plan.qualification.as_ref() == request.actual_qualification.as_ref()
                && request
                    .actual_qualification
                    .as_ref()
                    .is_some_and(|context| context.source_sha == plan.head)
        }
        _ => plan.qualification.is_none() && request.actual_qualification.is_none(),
    };
    if !coherent || !qualification_coherent {
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
    use velnor_actions_workflow_renderer::render::CANDIDATE_JOB_ID;
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

/// Every planned matrix entry must name a required gate job.
///
/// Qualification and ordinary CI render separate executor sets. The
/// event-selected plan must stay inside the corresponding final-gate
/// inventory after the other static set is removed.
pub(crate) fn check_matrix_job_inventory(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if !matrix_jobs_are_required(
        plan.matrix
            .include
            .iter()
            .map(|entry| entry.job_id.as_str()),
        &request.required_job_ids,
    ) {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
    }
}

/// Check that each matrix owner occurs in the final-gate inventory.
///
/// Matrix entries may share one physical owner. Keep every entry in the check
/// so a missing owner still fails, while repeated owner IDs remain valid.
fn matrix_jobs_are_required<'a>(
    matrix_jobs: impl IntoIterator<Item = &'a str>,
    required_job_ids: &[String],
) -> bool {
    let required: BTreeSet<&str> = required_job_ids.iter().map(String::as_str).collect();
    matrix_jobs
        .into_iter()
        .all(|job_id| required.contains(job_id))
}

#[test]
fn matrix_job_inventory_requires_every_owner() {
    assert!(matrix_jobs_are_required(
        ["plan", "rust-demo"],
        &["plan".to_owned(), "rust-demo".to_owned()],
    ));
    assert!(!matrix_jobs_are_required(
        ["plan", "rust-demo"],
        &["plan".to_owned()],
    ));
}
