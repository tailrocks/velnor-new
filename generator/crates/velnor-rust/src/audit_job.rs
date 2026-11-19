// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, CacheRoot, CargoCommand, HelperCommand, Job, JobId, RepoFacts, RootSlug, RustRoot,
    StepKind,
};

use crate::lane::{Lane, PLAN_JOB};
use crate::plan::{job, setup_roots, step};

pub(crate) const AUDIT_JOB: JobId = JobId::from_static("rust-inputs-audit");

pub(crate) fn audit_job(facts: &RepoFacts, jobs: &[Job]) -> Option<Job> {
    if facts.rust_roots.len() == 1 && !jobs.iter().any(|job| job.needs.contains(&AUDIT_JOB)) {
        return None;
    }
    let when = audit_when(facts);
    let lane = Lane {
        slug: RootSlug::from_static("inputs_audit"),
        plan: Some(PLAN_JOB),
    };
    let families: Vec<JobId> = facts
        .rust_roots
        .iter()
        .map(|root| target_family(jobs, root).unwrap_or(AUDIT_JOB))
        .collect();
    let mut steps = setup_roots(facts, &facts.rust_roots, &lane, &families);
    steps.extend(
        facts
            .rust_roots
            .iter()
            .map(|root| lane.cargo(CargoCommand::BuildAll, Some(root))),
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

/// The audit builds every target, so it restores, read-only, the target cache
/// of the last lane that saves this root (the one that builds the tests) and
/// starts warm instead of building cold.
fn target_family(jobs: &[Job], root: &RustRoot) -> Option<JobId> {
    let cache_root = CacheRoot::new(&root.manifest, root.target_dir.clone());
    jobs.iter()
        .flat_map(|job| &job.steps)
        .filter_map(|step| match &step.kind {
            StepKind::CacheSave(CacheKey::Target { job, root, .. }) if *root == cache_root => {
                Some(job.clone())
            }
            _ => None,
        })
        .next_back()
}

#[derive(PartialEq, Eq)]
pub(crate) enum AuditLocation {
    Inline,
    Separate,
}

pub(crate) fn audit_when(facts: &RepoFacts) -> velnor_model::When {
    use velnor_model::{OnMain, When};
    match facts.config.on_main {
        OnMain::Everything => When::FullAuditOrPush,
        OnMain::Affected => When::FullAudit,
    }
}
