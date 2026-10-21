//! Per-job plan dispositions (hard decision 4).
//!
//! Every generated job that owns obligations carries exactly one
//! disposition. A `not_affected` job is skipped as a whole job by its
//! generated job-level gate; `Required` passes it only as skipped. The
//! pairing with obligations and matrix entries is closed: a `not_affected`
//! job owns at least one obligation, only `not_affected` ones, and no matrix
//! entry; a `run` job owns no `not_affected` obligation and at least one
//! matrix entry; every obligation and entry names a planned job, and every
//! planned job owns an obligation or an entry (a paired named-check lane
//! owns only entries; a container image build job, id prefix `image-`, owns
//! neither, and is planned by image selection alone). Only affected-selection events may carry
//! `not_affected` work: a full-run event (push, merge group,
//! qualification) never skips anything.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::plan::{ObligationDecision, Plan, WorkflowEvent};
use crate::config::{ANSIBLE_JOB_PREFIX, BUN_JOB_PREFIX, DBT_JOB_PREFIX, IMAGE_JOB_PREFIX};
use crate::errors::ContractError;
use crate::graph::check_sorted_by;

/// One generated job's plan disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanJob {
    /// Generated job ID.
    pub job_id: String,
    /// Whether the job runs.
    pub disposition: JobDisposition,
}

/// Whether one generated job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobDisposition {
    /// The job runs and must succeed with a report for every obligation.
    Run,
    /// The job is skipped; every obligation it owns is `not_affected`.
    NotAffected,
}

/// True when `event` selects only affected work (hard decision 4).
///
/// Pull requests, fork pull requests included, and local pre-push runs
/// test what they affect. Pushes, merge groups, and dispatched
/// qualification runs test everything: the merge queue and the default
/// branch are the full backstop. Exhaustive on purpose: a new event must
/// choose here, and plan validation plus `Required` both enforce it.
#[must_use]
pub const fn selects_affected_work(event: WorkflowEvent) -> bool {
    match event {
        WorkflowEvent::PullRequest | WorkflowEvent::Fork | WorkflowEvent::Local => true,
        WorkflowEvent::Push | WorkflowEvent::MergeGroup | WorkflowEvent::Qualification => false,
    }
}

/// True when the plan skips any job or obligation.
#[must_use]
pub fn plan_skips_work(plan: &Plan) -> bool {
    plan.jobs
        .iter()
        .any(|job| job.disposition == JobDisposition::NotAffected)
        || plan
            .obligations
            .iter()
            .any(|obligation| obligation.decision == ObligationDecision::NotAffected)
}

/// Validate job dispositions against obligations and matrix entries.
///
/// # Errors
///
/// Returns an identity error for any `not_affected` work on a full-run
/// event, unsorted or duplicate jobs, an
/// obligation naming an unplanned job, a job without obligations, or any
/// disposition that disagrees with its obligations or matrix entries.
pub(crate) fn validate_plan_jobs(plan: &Plan) -> Result<(), ContractError> {
    if !selects_affected_work(plan.event) && plan_skips_work(plan) {
        return Err(ContractError::identity(
            "jobs",
            "not_affected_on_full_event",
        ));
    }
    check_sorted_by(&plan.jobs, "jobs", |job| job.job_id.as_str())?;
    let mut jobs: BTreeMap<&str, JobDisposition> = BTreeMap::new();
    for job in &plan.jobs {
        if jobs.insert(job.job_id.as_str(), job.disposition).is_some() {
            return Err(ContractError::Collision(format!("job {}", job.job_id)));
        }
    }
    let mut owning = BTreeSet::new();
    for obligation in &plan.obligations {
        let Some(disposition) = jobs.get(obligation.job_id.as_str()) else {
            return Err(ContractError::identity(
                "obligations.job_id",
                "unplanned_job",
            ));
        };
        owning.insert(obligation.job_id.as_str());
        let not_affected = obligation.decision == ObligationDecision::NotAffected;
        if not_affected != (*disposition == JobDisposition::NotAffected) {
            return Err(ContractError::identity(
                "obligations.decision",
                "disposition_mismatch",
            ));
        }
    }
    let mut entries = BTreeSet::new();
    for entry in &plan.matrix.include {
        match jobs.get(entry.job_id.as_str()) {
            Some(JobDisposition::Run) => {
                entries.insert(entry.job_id.as_str());
            }
            Some(JobDisposition::NotAffected) => {
                return Err(ContractError::identity(
                    "matrix.job_id",
                    "not_affected_job_entry",
                ));
            }
            None => return Err(ContractError::identity("matrix.job_id", "unplanned_job")),
        }
    }
    for (job_id, disposition) in &jobs {
        let ownerless = [
            (IMAGE_JOB_PREFIX, "image_job_owns_work"),
            (BUN_JOB_PREFIX, "bun_job_owns_work"),
            (DBT_JOB_PREFIX, "dbt_job_owns_work"),
            (ANSIBLE_JOB_PREFIX, "ansible_job_owns_work"),
        ]
        .into_iter()
        .find(|(prefix, _)| job_id.starts_with(prefix));
        if let Some((_, code)) = ownerless {
            if owning.contains(job_id) || entries.contains(job_id) {
                return Err(ContractError::identity("jobs", code));
            }
            continue;
        }
        let owns = match disposition {
            JobDisposition::Run => entries.contains(job_id),
            JobDisposition::NotAffected => owning.contains(job_id),
        };
        if !owns {
            return Err(ContractError::identity("jobs", "job_without_work"));
        }
    }
    Ok(())
}
