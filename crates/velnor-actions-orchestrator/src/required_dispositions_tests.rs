//! Every row of the Required verdict table (hard decision 4).

use std::collections::BTreeSet;

use velnor_actions_contract::{JobConclusion, JobDisposition, PlanJob, RequiredJobResult};

use super::{GATE_MISMATCH, PLANNED_JOB_SKIPPED, fold_planned_jobs};
use crate::cover::Signals;

/// Verdict class for one observed conclusion under one disposition.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Failed,
    Cancelled,
    NotRun(&'static str),
    PlanningFailed(&'static str),
}

fn fold(disposition: Option<JobDisposition>, conclusion: JobConclusion) -> Verdict {
    fold_job("rust-a", disposition, conclusion)
}

fn fold_job(id: &str, disposition: Option<JobDisposition>, conclusion: JobConclusion) -> Verdict {
    let planned: Vec<PlanJob> = disposition
        .map(|disposition| PlanJob {
            job_id: id.to_owned(),
            disposition,
        })
        .into_iter()
        .collect();
    let observed = [RequiredJobResult {
        job_id: id.to_owned(),
        conclusion,
    }];
    let mut signals = Signals::default();
    let mut reasons = BTreeSet::new();
    fold_planned_jobs(&planned, &observed, &mut signals, &mut reasons);
    let reason = |token: &'static str| {
        assert!(reasons.contains(token), "{reasons:?}");
        token
    };
    if signals.planning_failed {
        Verdict::PlanningFailed(reason(GATE_MISMATCH))
    } else if signals.failed {
        Verdict::Failed
    } else if signals.cancelled {
        Verdict::Cancelled
    } else if signals.not_run {
        if disposition == Some(JobDisposition::Run) {
            Verdict::NotRun(reason(PLANNED_JOB_SKIPPED))
        } else {
            Verdict::NotRun("unplanned")
        }
    } else {
        assert!(reasons.is_empty(), "{reasons:?}");
        Verdict::Pass
    }
}

#[test]
fn planned_run_jobs_must_run_and_succeed() {
    let run = Some(JobDisposition::Run);
    assert_eq!(fold(run, JobConclusion::Success), Verdict::Pass);
    assert_eq!(
        fold(run, JobConclusion::Skipped),
        Verdict::NotRun(PLANNED_JOB_SKIPPED),
        "plan run + job skipped fails"
    );
    assert_eq!(
        fold(run, JobConclusion::Neutral),
        Verdict::NotRun(PLANNED_JOB_SKIPPED)
    );
    assert_eq!(fold(run, JobConclusion::Failure), Verdict::Failed);
    assert_eq!(fold(run, JobConclusion::Missing), Verdict::Failed);
    assert_eq!(fold(run, JobConclusion::Cancelled), Verdict::Cancelled);
}

#[test]
fn not_affected_jobs_pass_only_when_skipped() {
    let skip = Some(JobDisposition::NotAffected);
    assert_eq!(
        fold(skip, JobConclusion::Skipped),
        Verdict::Pass,
        "plan skip + job skipped passes"
    );
    assert_eq!(
        fold(skip, JobConclusion::Success),
        Verdict::PlanningFailed(GATE_MISMATCH),
        "plan skip + job ran fails closed"
    );
    assert_eq!(
        fold(skip, JobConclusion::Neutral),
        Verdict::PlanningFailed(GATE_MISMATCH)
    );
    assert_eq!(fold(skip, JobConclusion::Failure), Verdict::Failed);
    assert_eq!(fold(skip, JobConclusion::Missing), Verdict::Failed);
    assert_eq!(fold(skip, JobConclusion::Cancelled), Verdict::Cancelled);
}

#[test]
fn unplanned_jobs_never_pass_when_skipped() {
    assert_eq!(fold(None, JobConclusion::Success), Verdict::Pass);
    assert_eq!(
        fold(None, JobConclusion::Skipped),
        Verdict::NotRun("unplanned")
    );
    assert_eq!(fold(None, JobConclusion::Failure), Verdict::Failed);
    assert_eq!(fold(None, JobConclusion::Cancelled), Verdict::Cancelled);
}

/// A Bun suite job (one id for every matrix leg: GitHub reports the
/// aggregate) folds exactly like any planned job (design section 8.3).
#[test]
fn bun_suite_matrix_aggregates_fold_like_any_planned_job() {
    let suite = "bun-chainargos-platform--browser";
    let run = Some(JobDisposition::Run);
    let skip = Some(JobDisposition::NotAffected);
    assert_eq!(fold_job(suite, run, JobConclusion::Success), Verdict::Pass);
    // A leg that timed out or failed makes the aggregate `failure`.
    assert_eq!(
        fold_job(suite, run, JobConclusion::Failure),
        Verdict::Failed
    );
    assert_eq!(
        fold_job(suite, run, JobConclusion::Cancelled),
        Verdict::Cancelled
    );
    assert_eq!(
        fold_job(suite, run, JobConclusion::Skipped),
        Verdict::NotRun(PLANNED_JOB_SKIPPED)
    );
    assert_eq!(fold_job(suite, skip, JobConclusion::Skipped), Verdict::Pass);
    assert_eq!(
        fold_job(suite, skip, JobConclusion::Success),
        Verdict::PlanningFailed(GATE_MISMATCH)
    );
}
