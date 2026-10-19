//! Planned-job verdicts: observed job results against plan dispositions.
//!
//! Hard decision 4: a crate job the plan marks `not_affected` is skipped by
//! its generated job-level gate. `Required` never trusts a conclusion
//! alone; it pairs every observed job result with the plan's disposition:
//!
//! | Plan           | Observed          | Verdict                         |
//! |----------------|-------------------|---------------------------------|
//! | `run`          | success           | pass (reports still required)   |
//! | `run`          | skipped / neutral | fail, `planned_job_skipped`     |
//! | `run`          | failure / missing | fail                            |
//! | `run`          | cancelled         | cancelled                       |
//! | `not_affected` | skipped           | pass, no report expected        |
//! | `not_affected` | success / neutral | fail closed, `gate_mismatch`    |
//! | `not_affected` | failure / missing | fail                            |
//! | `not_affected` | cancelled         | cancelled                       |
//!
//! Jobs the plan does not list (plan, actionlint, named checks, `OpenTofu`
//! roots) keep the generic rule: skipped is never success. A missing,
//! invalid, or failed plan never reaches this fold (`planning_failed`).

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{JobConclusion, JobDisposition, PlanJob, RequiredJobResult};

use crate::cover::Signals;

/// Miss reason for a planned job that was skipped.
pub(crate) const PLANNED_JOB_SKIPPED: &str = "planned_job_skipped";
/// Miss reason for a `not_affected` job that ran anyway.
pub(crate) const GATE_MISMATCH: &str = "gate_mismatch";

/// Fold every observed job result against the plan's job dispositions.
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

/// A job the plan scheduled must run and succeed.
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

/// A job the plan marked `not_affected` must be skipped by its gate.
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

/// Jobs outside the plan's crate-job inventory: skipped is never success.
fn fold_unplanned(conclusion: JobConclusion, signals: &mut Signals) {
    match conclusion {
        JobConclusion::Success => {}
        JobConclusion::Cancelled => signals.cancelled = true,
        JobConclusion::Skipped | JobConclusion::Neutral => signals.not_run = true,
        JobConclusion::Failure | JobConclusion::Missing => signals.failed = true,
    }
}

#[cfg(test)]
#[path = "required_dispositions_tests.rs"]
mod tests;
