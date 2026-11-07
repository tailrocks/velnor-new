// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, JobDisposition, MatrixEntry, ObligationDecision, PlanJob, PlanObligation,
    ProposedTask,
};

use crate::OrchestratorError;
use crate::crate_job_ids::JobOwners;
use crate::internal::internal_contract;

pub(crate) const JOB_RUNS_REASON: &str = "job_runs";
pub(crate) const NEEDED_REASON: &str = "needed_by_running_job";

pub(crate) fn job_needs(
    tasks: &[&ProposedTask],
    owners: &JobOwners<'_>,
) -> Result<BTreeMap<String, BTreeSet<String>>, OrchestratorError> {
    let mut needs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for task in tasks {
        let Some(job) = owners.for_member(task).map_err(internal_contract)? else {
            continue;
        };
        for gate in task.gated_by.iter().chain(&task.depends_on) {
            let Some(member) = owners.runnable_member(gate) else {
                continue;
            };
            let Some(owner) = owners.for_member(member).map_err(internal_contract)? else {
                continue;
            };
            if owner != job {
                needs.entry(job.clone()).or_default().insert(owner);
            }
        }
    }
    Ok(needs)
}

pub(crate) fn finalize_dispositions(
    obligations: &mut [PlanObligation],
    entries: &mut Vec<MatrixEntry>,
    needs: &BTreeMap<String, BTreeSet<String>>,
    routes: &[velnor_actions_contract::LaneRoute],
) -> Vec<PlanJob> {
    let mut skippable: BTreeMap<&str, bool> = BTreeMap::new();
    for obligation in obligations.iter() {
        let unaffected = obligation.decision == ObligationDecision::NotAffected
            && (obligation.job_id.starts_with(CRATE_JOB_ID_PREFIX)
                || obligation.job_id.starts_with("check-"));
        skippable
            .entry(&obligation.job_id)
            .and_modify(|all| *all &= unaffected)
            .or_insert(unaffected);
    }
    let mut not_affected: BTreeSet<String> = skippable
        .into_iter()
        .filter(|(_, all)| *all)
        .map(|(job, _)| job.to_owned())
        .collect();
    let mut needed = BTreeSet::new();
    loop {
        let promote: BTreeSet<String> = needs
            .iter()
            .filter(|(job, _)| !not_affected.contains(*job))
            .flat_map(|(_, deps)| deps.iter())
            .filter(|dep| not_affected.contains(*dep))
            .cloned()
            .collect();
        if promote.is_empty() {
            break;
        }
        for job in promote {
            not_affected.remove(&job);
            needed.insert(job);
        }
    }
    for obligation in obligations.iter_mut() {
        if obligation.decision == ObligationDecision::NotAffected
            && !not_affected.contains(&obligation.job_id)
        {
            obligation.decision = ObligationDecision::Execute;
            let reason = if needed.contains(&obligation.job_id) {
                NEEDED_REASON
            } else {
                JOB_RUNS_REASON
            };
            reason.clone_into(&mut obligation.reason);
        }
    }
    let by_output: BTreeMap<&str, &velnor_actions_contract::LaneRoute> = routes
        .iter()
        .map(|route| (route.job_id.as_str(), route))
        .collect();
    entries.retain(|entry| {
        let logical = by_output
            .get(entry.job_id.as_str())
            .map_or(entry.job_id.as_str(), |route| route.logical_job_id.as_str());
        !not_affected.contains(logical)
    });
    project_jobs(obligations, routes, &not_affected)
}

fn project_jobs(
    obligations: &[PlanObligation],
    routes: &[velnor_actions_contract::LaneRoute],
    not_affected: &BTreeSet<String>,
) -> Vec<PlanJob> {
    let mut jobs = BTreeMap::<String, PlanJob>::new();
    for obligation in obligations {
        let matching: Vec<_> = routes
            .iter()
            .filter(|route| route.logical_job_id == obligation.job_id)
            .collect();
        let disposition = if not_affected.contains(&obligation.job_id) {
            JobDisposition::NotAffected
        } else {
            JobDisposition::Run
        };
        if matching.is_empty() {
            jobs.entry(obligation.job_id.clone())
                .or_insert_with(|| PlanJob {
                    job_id: obligation.job_id.clone(),
                    disposition,
                    lane: None,
                });
        } else {
            for route in matching {
                jobs.entry(route.job_id.clone()).or_insert_with(|| PlanJob {
                    job_id: route.job_id.clone(),
                    disposition,
                    lane: route
                        .variant
                        .map(|lane_variant| velnor_actions_contract::PairedJob {
                            logical_job_id: route.logical_job_id.clone(),
                            lane_variant,
                        }),
                });
            }
        }
    }
    jobs.into_values().collect()
}

pub(crate) fn encode_not_affected_jobs(jobs: &[PlanJob]) -> String {
    let ids: Vec<&str> = jobs
        .iter()
        .filter(|job| job.disposition == JobDisposition::NotAffected)
        .map(|job| job.job_id.as_str())
        .collect();
    if ids.is_empty() {
        String::new()
    } else {
        format!(",{},", ids.join(","))
    }
}
