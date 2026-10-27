// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::{ExecuteTaskRef, ObligationDecision, Plan};

use super::super::MergeRequest;
use crate::OrchestratorError;
use crate::cover::Signals;
use crate::internal::{SCHEMA, internal_contract};

pub(crate) fn check_agreement(
    matrix: Option<&velnor_actions_contract::PlanMatrix>,
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    let Some(matrix) = matrix else {
        signals.planning_failed = true;
        miss_reasons.insert("source_missing".to_owned());
        return Ok(());
    };
    let matrix_bytes =
        velnor_actions_contract::canonical_json_bytes(matrix).map_err(internal_contract)?;
    if velnor_actions_contract::check_matrix_agreement(&plan.matrix, &matrix_bytes).is_err() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    Ok(())
}

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

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CandidateAttestation {
    schema: u32,
    commit: String,
}

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
            ObligationDecision::ReusedFromTaskCache | ObligationDecision::NotAffected => {}
        }
    }
    if hollow {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
    }
}
