use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::plan::{ObligationDecision, Plan, WorkflowEvent};
use crate::config::{ANSIBLE_JOB_PREFIX, BUN_JOB_PREFIX, DBT_JOB_PREFIX, IMAGE_JOB_PREFIX};
use crate::errors::ContractError;
use crate::graph::check_sorted_by;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanJob {
    pub job_id: String,

    pub disposition: JobDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobDisposition {
    Run,

    NotAffected,
}

#[must_use]
pub const fn selects_affected_work(event: WorkflowEvent) -> bool {
    match event {
        WorkflowEvent::PullRequest | WorkflowEvent::Fork | WorkflowEvent::Local => true,
        WorkflowEvent::Push | WorkflowEvent::MergeGroup | WorkflowEvent::Qualification => false,
    }
}

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
