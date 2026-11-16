// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CargoCommand, HelperCommand, Job, JobId, RepoFacts, RootSlug, StepKind};

use crate::lane::{Lane, PLAN_JOB};
use crate::plan::{job, setup_roots, step};
use crate::root_jobs::audit_when;

const AUDIT_JOB: JobId = JobId::from_static("rust-inputs-audit");

pub(crate) fn audit_job(facts: &RepoFacts) -> Option<Job> {
    let when = audit_when(facts)?;
    let lane = Lane {
        slug: RootSlug::from_static("inputs_audit"),
        plan: Some(PLAN_JOB),
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
        needs: vec![PLAN_JOB],
        steps,
        ..job(facts, AUDIT_JOB)
    })
}
