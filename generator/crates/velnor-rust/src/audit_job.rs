// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, CargoCommand, HelperCommand, Job, JobId, RepoFacts, RootSlug, StepKind,
};

use crate::lane::{Lane, PLAN_JOB};
use crate::plan::{job, setup_units, step};
use crate::units::Unit;

pub(crate) const AUDIT_JOB: JobId = JobId::from_static("rust-inputs-audit");

pub(crate) fn audit_job(facts: &RepoFacts, units: &[Unit], jobs: &[Job]) -> Option<Job> {
    if facts.rust_roots.len() == 1 && !jobs.iter().any(|job| job.needs.contains(&AUDIT_JOB)) {
        return None;
    }
    let when = audit_when(facts)?;
    let lane = Lane {
        slug: RootSlug::from_static("inputs_audit"),
        plan: Some(PLAN_JOB),
        per_root: false,
    };
    let families: Vec<JobId> = units
        .iter()
        .map(|unit| target_family(facts, jobs, unit).unwrap_or(AUDIT_JOB))
        .collect();
    let mut steps = setup_units(facts, units, &lane, &families);
    steps.extend(units.iter().flat_map(|unit| {
        unit.roots.iter().map(|root| {
            let mut build = lane.cargo(CargoCommand::BuildAll, Some(root));
            if unit.grouped {
                build.directory = None;
                build.manifest = Some(root.manifest.clone());
                build.target_dir = Some(unit.target_dir.clone());
            }
            build
        })
    }));
    steps.push(step(StepKind::Helper(HelperCommand::InputsAudit)));
    Some(Job {
        when,
        pr: crate::plan::quick(facts),
        needs: vec![PLAN_JOB],
        steps,
        ..job(facts, AUDIT_JOB)
    })
}

/// The audit builds every target, so it restores, read-only, the target cache
/// of the last lane that saves this root (the one that builds the tests) and
/// starts warm instead of building cold.
fn target_family(facts: &RepoFacts, jobs: &[Job], unit: &Unit) -> Option<JobId> {
    let cache_root = unit.cache_root(facts);
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
