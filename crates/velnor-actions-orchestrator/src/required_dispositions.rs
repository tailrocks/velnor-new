// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::crate_job::{GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX};
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
    plan_event: WorkflowEvent,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let dispositions: BTreeMap<&str, JobDisposition> = planned
        .iter()
        .map(|job| (job.job_id.as_str(), job.disposition))
        .collect();
    for job in jobs {
        if let Some((disposition, is_writer)) = branch_disposition(&dispositions, job.job_id.as_str())
        {
            fold_branch(
                disposition,
                is_writer,
                job.conclusion,
                plan_event,
                signals,
                miss_reasons,
            );
            continue;
        }
        match dispositions.get(job.job_id.as_str()) {
            Some(JobDisposition::Run) => fold_run(job.conclusion, signals, miss_reasons),
            Some(JobDisposition::NotAffected) => {
                fold_not_affected(job.conclusion, signals, miss_reasons);
            }
            None => fold_unplanned(job.conclusion, signals),
        }
    }
}

fn branch_disposition(
    dispositions: &BTreeMap<&str, JobDisposition>,
    job_id: &str,
) -> Option<(JobDisposition, bool)> {
    if let Some(group) = job_id.strip_suffix(GROUP_WRITER_SUFFIX) {
        return dispositions.get(group).copied().map(|disposition| (disposition, true));
    }
    if let Some(group) = job_id.strip_suffix(GROUP_READER_SUFFIX) {
        return dispositions
            .get(group)
            .copied()
            .map(|disposition| (disposition, false));
    }
    None
}

fn fold_branch(
    disposition: JobDisposition,
    is_writer: bool,
    conclusion: JobConclusion,
    plan_event: WorkflowEvent,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if disposition == JobDisposition::Run
        && is_writer
        && conclusion == JobConclusion::Skipped
        && plan_event != WorkflowEvent::Push
    {
        return;
    }
    match disposition {
        JobDisposition::Run => fold_run(conclusion, signals, miss_reasons),
        JobDisposition::NotAffected => fold_not_affected(conclusion, signals, miss_reasons),
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
