// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CargoCommand, HelperCommand, Job, PermissionScope, RepoFacts, Reuse, ReusePrVerdict,
    ReuseStep, Step, StepKind, When, WorkflowPlan,
};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::lane::PLAN_JOB;
use crate::plan::step;

fn runs_on_every_event(when: When) -> bool {
    matches!(when, When::Success | When::NotCancelled)
}

fn reusable_audit(step: &Step) -> bool {
    matches!(step.kind, StepKind::InputsAuditRoot(_))
        && matches!(
            step.when,
            When::FullAuditOrPullRequest | When::FullAuditOrPushOrPullRequest
        )
}

fn kind_of(step: &Step) -> Option<&'static str> {
    match &step.kind {
        StepKind::Cargo(CargoCommand::FmtCheck) => Some("format"),
        StepKind::Cargo(command)
            if command.reports_tests()
                && !matches!(command, CargoCommand::NextestRunArchive { .. }) =>
        {
            Some("test")
        }
        StepKind::InputsAuditRoot(_) => Some("inputs-audit"),
        _ => None,
    }
}

fn is_lint_job(job: &Job) -> bool {
    job.steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::Lint(_)))
}

pub(crate) fn apply(facts: &RepoFacts, jobs: &mut [Job]) {
    if facts.config.reuse_pr_verdict != ReusePrVerdict::On
        || !jobs.iter().any(|job| job.id == PLAN_JOB)
    {
        return;
    }
    for job in jobs.iter_mut().filter(|job| job.id != PLAN_JOB) {
        if !runs_on_every_event(job.when) {
            continue;
        }
        if is_lint_job(job) {
            if !job.needs.contains(&PLAN_JOB) {
                job.needs.push(PLAN_JOB);
            }
            job.reuse = Some(Reuse::job(&job.id, &PLAN_JOB));
            continue;
        }
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for step in &mut job.steps {
            let Some(kind) =
                kind_of(step).filter(|_| runs_on_every_event(step.when) || reusable_audit(step))
            else {
                continue;
            };
            let position =
                if let Some((_, count)) = counts.iter_mut().find(|(seen, _)| *seen == kind) {
                    *count += 1;
                    *count
                } else {
                    counts.push((kind, 1));
                    1
                };
            step.reuse = Some(Reuse::step(&job.id, kind, position, &PLAN_JOB));
        }
        if job.steps.iter().any(|step| step.reuse.is_some()) && !job.needs.contains(&PLAN_JOB) {
            job.needs.push(PLAN_JOB);
        }
    }
    if let Some(plan) = jobs.iter_mut().find(|job| job.id == PLAN_JOB)
        && let Some(at) = plan
            .steps
            .iter()
            .position(|step| step.kind == StepKind::Helper(HelperCommand::Plan))
    {
        let required = facts.config.required_check.clone();
        plan.steps.splice(
            at + 1..=at,
            [
                step(StepKind::Reuse(ReuseStep::Record)),
                step(StepKind::Reuse(ReuseStep::Upload)),
                step(StepKind::Reuse(ReuseStep::Decide { required })),
            ],
        );
        plan.permissions
            .0
            .insert(PermissionScope::PullRequests, Access::Read);
        plan.permissions
            .0
            .insert(PermissionScope::Actions, Access::Read);
    }
}

pub fn check_reuse(facts: &RepoFacts, plan: &WorkflowPlan) -> Result<(), DetectError> {
    let planned = plan.jobs.iter().any(|job| {
        job.steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Reuse(ReuseStep::Decide { .. })))
    });
    if facts.config.reuse_pr_verdict == ReusePrVerdict::On && !planned {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::ReuseWithoutPlanJob,
        ));
    }
    Ok(())
}
