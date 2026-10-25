use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    JobConclusion, JobDisposition, Plan, PlanJob, RequiredJobResult, WorkflowEvent,
    plan_skips_work, selects_affected_work,
};

use crate::cover::Signals;

pub(crate) const PLANNED_JOB_SKIPPED: &str = "planned_job_skipped";
pub(crate) const GATE_MISMATCH: &str = "gate_mismatch";

pub(crate) const NOT_AFFECTED_ON_FULL_EVENT: &str = "not_affected_on_full_event";

pub(crate) fn check_full_event_skips(
    plan: &Plan,
    actual_event: Option<WorkflowEvent>,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let full = |event: WorkflowEvent| !selects_affected_work(event);
    if (full(plan.event) || actual_event.is_some_and(full)) && plan_skips_work(plan) {
        signals.planning_failed = true;
        miss_reasons.insert(NOT_AFFECTED_ON_FULL_EVENT.to_owned());
    }
}

pub(crate) fn fold_planned_jobs(
    planned: &[PlanJob],
    jobs: &[RequiredJobResult],
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let dispositions: BTreeMap<&str, JobDisposition> = planned
        .iter()
        .map(|job| (job.job_id.as_str(), job.disposition))
        .collect();
    for job in jobs {
        match dispositions.get(job.job_id.as_str()) {
            Some(JobDisposition::Run) => fold_run(job.conclusion, signals, miss_reasons),
            Some(JobDisposition::NotAffected) => {
                fold_not_affected(job.conclusion, signals, miss_reasons);
            }
            None => fold_unplanned(job.conclusion, signals),
        }
    }
}

fn fold_run(conclusion: JobConclusion, signals: &mut Signals, miss_reasons: &mut BTreeSet<String>) {
    match conclusion {
        JobConclusion::Success => {}
        JobConclusion::Cancelled => signals.cancelled = true,
        JobConclusion::Skipped | JobConclusion::Neutral => {
            signals.not_run = true;
            miss_reasons.insert(PLANNED_JOB_SKIPPED.to_owned());
        }
        JobConclusion::Failure | JobConclusion::Missing => signals.failed = true,
    }
}

fn fold_not_affected(
    conclusion: JobConclusion,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    match conclusion {
        JobConclusion::Skipped => {}
        JobConclusion::Success | JobConclusion::Neutral => {
            signals.planning_failed = true;
            miss_reasons.insert(GATE_MISMATCH.to_owned());
        }
        JobConclusion::Cancelled => signals.cancelled = true,
        JobConclusion::Failure | JobConclusion::Missing => signals.failed = true,
    }
}

fn fold_unplanned(conclusion: JobConclusion, signals: &mut Signals) {
    match conclusion {
        JobConclusion::Success => {}
        JobConclusion::Cancelled => signals.cancelled = true,
        JobConclusion::Skipped | JobConclusion::Neutral => signals.not_run = true,
        JobConclusion::Failure | JobConclusion::Missing => signals.failed = true,
    }
}
