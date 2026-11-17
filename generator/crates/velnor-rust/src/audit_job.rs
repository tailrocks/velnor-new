// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, HelperCommand, Job, JobId, RepoFacts, RootSlug, StepKind};

use crate::lane::{Lane, PLAN_JOB};
use crate::plan::{HELPER_JOB, job, setup_roots, step};

pub(crate) const AUDIT_JOB: JobId = JobId::from_static("rust-inputs-audit");

pub(crate) fn audit_job(facts: &RepoFacts, jobs: &[Job], from_source: bool) -> Option<Job> {
    if facts.rust_roots.len() == 1 && !jobs.iter().any(|job| job.needs.contains(&AUDIT_JOB)) {
        return None;
    }
    let when = audit_when(facts)?;
    let lane = Lane {
        slug: RootSlug::from_static("inputs_audit"),
        plan: Some(PLAN_JOB),
        from_source,
    };
    let mut steps = setup_roots(facts, &facts.rust_roots, &lane, &AUDIT_JOB);
    steps.extend(
        facts
            .rust_roots
            .iter()
            .map(|root| lane.cargo(CargoCommand::BuildAll, root.manifest.parent())),
    );
    steps.push(step(StepKind::Helper(HelperCommand::InputsAudit)));
    Some(Job {
        when,
        skip_on_draft: false,
        needs: [PLAN_JOB]
            .into_iter()
            .chain(from_source.then_some(HELPER_JOB))
            .collect(),
        steps,
        ..job(facts, AUDIT_JOB)
    })
}

#[derive(PartialEq, Eq)]
pub(crate) enum AuditLocation {
    Inline,
    Separate,
}

pub(crate) fn audit_when(facts: &RepoFacts) -> Option<velnor_model::When> {
    use velnor_model::{FullRun, OnMain, When};
    if !crate::base_cache::has_saving_helper(facts) {
        return match (facts.config.on_main, &facts.config.full_run) {
            (OnMain::Everything, _) => Some(When::ScheduledOrPush),
            (OnMain::Affected, FullRun::Off) => None,
            (OnMain::Affected, _) => Some(When::Scheduled),
        };
    }
    Some(match facts.config.on_main {
        OnMain::Everything => When::FullAuditOrPush,
        OnMain::Affected => When::FullAudit,
    })
}
