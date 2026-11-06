// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<PairedJob>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedJob {
    pub logical_job_id: String,
    pub lane_variant: super::lanes::LaneVariant,
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
    let mut jobs = BTreeMap::new();
    let mut pairs = BTreeMap::<&str, (Option<&PlanJob>, Option<&PlanJob>)>::new();
    for job in &plan.jobs {
        if jobs.insert(job.job_id.as_str(), job).is_some() {
            return Err(ContractError::Collision(format!("job {}", job.job_id)));
        }
        if let Some(lane) = &job.lane {
            let pair = pairs.entry(lane.logical_job_id.as_str()).or_default();
            let slot = match lane.lane_variant {
                super::lanes::LaneVariant::Hosted => &mut pair.0,
                super::lanes::LaneVariant::ScaleSet => &mut pair.1,
            };
            if slot.replace(job).is_some() {
                return Err(ContractError::identity(
                    "jobs.lane",
                    "duplicate_lane_variant",
                ));
            }
        }
    }
    validate_pairs(&pairs, &jobs)?;
    let mut owning = BTreeSet::new();
    for obligation in &plan.obligations {
        let logical = obligation.job_id.as_str();
        let disposition = if let Some((Some(hosted), _)) = pairs.get(logical) {
            hosted.disposition
        } else {
            jobs.get(logical)
                .ok_or_else(|| ContractError::identity("obligations.job_id", "unplanned_job"))?
                .disposition
        };
        owning.insert(logical);
        if (obligation.decision == ObligationDecision::NotAffected)
            != (disposition == JobDisposition::NotAffected)
        {
            return Err(ContractError::identity(
                "obligations.decision",
                "disposition_mismatch",
            ));
        }
    }
    let mut entries = BTreeSet::new();
    for entry in &plan.matrix.include {
        let job = jobs.get(entry.job_id.as_str()).ok_or_else(|| {
            ContractError::identity("matrix.job_id", format!("unplanned_job:{}", entry.job_id))
        })?;
        if job.disposition != JobDisposition::Run {
            return Err(ContractError::identity(
                "matrix.job_id",
                "not_affected_job_entry",
            ));
        }
        let obligation = plan
            .obligations
            .iter()
            .find(|ob| ob.task_id == entry.task_id)
            .ok_or_else(|| ContractError::identity("matrix.task_id", "missing_obligation"))?;
        match (&job.lane, entry.lane_variant) {
            (Some(lane), Some(variant))
                if lane.lane_variant == variant && lane.logical_job_id == obligation.job_id => {}
            (None, None) if job.job_id == obligation.job_id => {}
            _ => {
                return Err(ContractError::identity(
                    "matrix.job_id",
                    "lane_ownership_mismatch",
                ));
            }
        }
        entries.insert(entry.job_id.as_str());
    }
    validate_job_work(&jobs, &owning, &entries)?;
    Ok(())
}

fn validate_pairs(
    pairs: &BTreeMap<&str, (Option<&PlanJob>, Option<&PlanJob>)>,
    jobs: &BTreeMap<&str, &PlanJob>,
) -> Result<(), ContractError> {
    for (logical, (hosted, local)) in pairs {
        let (Some(hosted), Some(local)) = (hosted, local) else {
            return Err(ContractError::identity("jobs.lane", "incomplete_lane_pair"));
        };
        if hosted.disposition != local.disposition || jobs.contains_key(logical) {
            return Err(ContractError::identity(
                "jobs.lane",
                "lane_disposition_mismatch",
            ));
        }
    }
    Ok(())
}

fn validate_job_work(
    jobs: &BTreeMap<&str, &PlanJob>,
    owning: &BTreeSet<&str>,
    entries: &BTreeSet<&str>,
) -> Result<(), ContractError> {
    for (job_id, job) in jobs {
        let logical = job
            .lane
            .as_ref()
            .map_or(*job_id, |lane| lane.logical_job_id.as_str());
        let ownerless = [
            (IMAGE_JOB_PREFIX, "image_job_owns_work"),
            (BUN_JOB_PREFIX, "bun_job_owns_work"),
            (DBT_JOB_PREFIX, "dbt_job_owns_work"),
            (ANSIBLE_JOB_PREFIX, "ansible_job_owns_work"),
        ]
        .into_iter()
        .find(|(prefix, _)| job_id.starts_with(prefix));
        if let Some((_, code)) = ownerless {
            if owning.contains(logical) || entries.contains(job_id) {
                return Err(ContractError::identity("jobs", code));
            }
            continue;
        }
        let owns = match job.disposition {
            JobDisposition::Run => entries.contains(job_id),
            JobDisposition::NotAffected => owning.contains(logical),
        };
        if !owns {
            return Err(ContractError::identity("jobs", "job_without_work"));
        }
    }
    Ok(())
}

pub(crate) fn validate_lane_inventory(plan: &Plan) -> bool {
    let expected = plan.lane_inventory.as_ref();
    let mut required = BTreeMap::new();
    if let Some(inventory) = expected {
        for route in &inventory.routes {
            let mut tasks: Vec<_> = plan
                .obligations
                .iter()
                .filter(|item| item.job_id == route.logical_job_id)
                .map(|item| item.task_id.clone())
                .collect();
            tasks.sort();
            if tasks != route.task_ids || tasks.is_empty() {
                return false;
            }
            let Some(job) = plan.jobs.iter().find(|job| job.job_id == route.job_id) else {
                return false;
            };
            match (route.variant, &job.lane) {
                (Some(variant), Some(lane))
                    if lane.logical_job_id == route.logical_job_id
                        && lane.lane_variant == variant =>
                {
                    if required
                        .insert(
                            route.job_id.as_str(),
                            (route.logical_job_id.as_str(), variant),
                        )
                        .is_some()
                    {
                        return false;
                    }
                }
                (None, None) if route.job_id == route.logical_job_id => {}
                _ => return false,
            }
            let entries: Vec<_> = plan
                .matrix
                .include
                .iter()
                .filter(|entry| entry.job_id == route.job_id)
                .collect();
            let mut entry_tasks: Vec<_> =
                entries.iter().map(|entry| entry.task_id.clone()).collect();
            entry_tasks.sort();
            if match job.disposition {
                JobDisposition::Run => entry_tasks != route.task_ids,
                JobDisposition::NotAffected => !entry_tasks.is_empty(),
            } {
                return false;
            }
            for entry in entries {
                if entry.lane_variant != route.variant
                    || entry.planned_platform.runs_on != route.runner
                    || route.target.as_deref() != Some(entry.planned_platform.target.as_str())
                    || !route.task_ids.contains(&entry.task_id)
                {
                    return false;
                }
            }
        }
    }
    let actual: BTreeMap<_, _> = plan
        .jobs
        .iter()
        .filter_map(|job| {
            job.lane.as_ref().map(|lane| {
                (
                    job.job_id.as_str(),
                    (lane.logical_job_id.as_str(), lane.lane_variant),
                )
            })
        })
        .collect();
    actual == required
}
