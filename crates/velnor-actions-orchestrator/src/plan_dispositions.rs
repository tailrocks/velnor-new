//! Crate-job dispositions from obligation decisions (hard decision 4).
//!
//! A Rust crate job is `not_affected` only when every obligation it owns is
//! and no running job needs it. Anything else runs:
//!
//! - a job owning both `not_affected` and executing obligations runs them
//!   all (`job_runs`): a job is skipped whole or not at all;
//! - a job another running job needs through a cross-job gate runs
//!   (`needed_by_running_job`), so an implicit `needs` skip can never stand
//!   in for a planned run;
//! - `OpenTofu` root jobs, named check jobs, and the plan job always run in
//!   phase 1; their selection has not been audited for skipping.
//!
//! Matrix entries remain only for jobs that run.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CRATE_JOB_ID_PREFIX, JobDisposition, MatrixEntry, ObligationDecision, PlanJob, PlanObligation,
    ProposedTask,
};

use velnor_actions_workflow_renderer::render::{NOT_AFFECTED_JOBS_OUTPUT, PLAN_JOB_ID};

use crate::OrchestratorError;
use crate::crate_job_ids::JobOwners;
use crate::internal::internal_contract;
use crate::internal_plan::wire_w2::ReuseOutcome;

/// Reason for a `not_affected` obligation whose job runs anyway.
pub(crate) const JOB_RUNS_REASON: &str = "job_runs";
/// Reason for an unaffected job that a running job needs.
pub(crate) const NEEDED_REASON: &str = "needed_by_running_job";

/// Disposition of one unchanged Rust obligation (hard decision 4).
///
/// Unchanged work is `not_affected` unless a fail-safe signal says its
/// reads are not bounded by selection: a build script may read anything
/// until in-job read verification exists, so its package always runs.
pub(crate) fn unaffected_rust_outcome(task: &ProposedTask) -> ReuseOutcome {
    if task.identity.undeclared_reads {
        ReuseOutcome::execute("undeclared_reads")
    } else {
        ReuseOutcome::not_affected()
    }
}

/// Cross-job gate dependencies: job ID to the job IDs it needs.
///
/// Mirrors the generated `needs` lift (`crate_jobs::gates`): a runnable
/// gate owned by another job becomes a dependency of the gated job.
///
/// # Errors
///
/// Returns a contract error for a member with an unknown stack.
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

/// Decide every job's disposition and normalize obligations and entries.
pub(crate) fn finalize_dispositions(
    obligations: &mut [PlanObligation],
    entries: &mut Vec<MatrixEntry>,
    needs: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<PlanJob> {
    let mut skippable: BTreeMap<&str, bool> = BTreeMap::new();
    for obligation in obligations.iter() {
        let unaffected = obligation.decision == ObligationDecision::NotAffected
            && obligation.job_id.starts_with(CRATE_JOB_ID_PREFIX);
        skippable
            .entry(obligation.job_id.as_str())
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
    entries.retain(|entry| !not_affected.contains(&entry.job_id));
    let mut ids: BTreeSet<&str> = obligations.iter().map(|ob| ob.job_id.as_str()).collect();
    ids.extend(entries.iter().map(|entry| entry.job_id.as_str()));
    ids.into_iter()
        .map(|job_id| PlanJob {
            job_id: job_id.to_owned(),
            disposition: if not_affected.contains(job_id) {
                JobDisposition::NotAffected
            } else {
                JobDisposition::Run
            },
        })
        .collect()
}

/// Job-level gate skipping one Rust crate job the plan marks `not_affected`.
///
/// Negative list over the plan's `not_affected_jobs` output (`,a,b,`): an
/// empty or missing output matches nothing and runs the job, and the
/// implicit `success()` skips it when the plan fails (hard decision 4).
pub(crate) fn not_affected_gate(job_id: &str) -> String {
    format!("!contains(needs.{PLAN_JOB_ID}.outputs.{NOT_AFFECTED_JOBS_OUTPUT}, ',{job_id},')")
}

/// Encode `not_affected` job IDs for the plan job's output: `,a,b,` or empty.
///
/// Job IDs never contain commas, so `contains(..., ',<id>,')` matches whole
/// IDs only; the empty encoding matches nothing and runs every job.
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

#[cfg(test)]
#[path = "plan_dispositions_tests.rs"]
mod tests;
