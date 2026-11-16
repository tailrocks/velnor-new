// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Access, FullRun, Job, JobId, PermissionScope, RepoFacts, Step, StepKind, When};

use crate::base_cache::has_saving_helper;
use crate::plan::{head, job, step};

pub(crate) fn enabled(facts: &RepoFacts, from_source: bool) -> bool {
    from_source || has_saving_helper(facts)
}

pub(crate) fn after_saves(facts: &RepoFacts, jobs: &mut [Job], from_source: bool) {
    if !enabled(facts, from_source) {
        return;
    }
    for job in jobs {
        if !has_saving_helper(facts)
            && !job
                .steps
                .iter()
                .any(|step| matches!(step.kind, StepKind::HelperFromArtifact))
        {
            continue;
        }
        let mut steps = Vec::new();
        for saved in std::mem::take(&mut job.steps) {
            let budget = matches!(saved.kind, StepKind::CacheSave(_)).then(|| Step {
                kind: StepKind::CacheBudget(
                    facts.config.cache_budget_bytes,
                    match &saved.kind {
                        StepKind::CacheSave(key) => Some(key.clone()),
                        _ => None,
                    },
                ),
                ..saved.clone()
            });
            steps.push(saved);
            steps.extend(budget);
        }
        job.steps = steps;
        if job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::CacheBudget(..)))
        {
            permissions(job);
        }
    }
}

pub(crate) fn scheduled(facts: &RepoFacts, jobs: &[Job], from_source: bool) -> Option<Job> {
    if facts.config.full_run == FullRun::Off || !enabled(facts, from_source) {
        return None;
    }
    let mut maintenance = Job {
        when: When::ScheduledMaintenance,
        skip_on_draft: false,
        needs: jobs.iter().map(|job| job.id.clone()).collect(),
        steps: head(facts, 1, from_source),
        ..job(facts, JobId::from_static("rust-cache-budget"))
    };
    maintenance.steps.push(step(StepKind::CacheBudget(
        facts.config.cache_budget_bytes,
        None,
    )));
    permissions(&mut maintenance);
    Some(maintenance)
}

fn permissions(job: &mut Job) {
    job.permissions
        .0
        .insert(PermissionScope::Actions, Access::Write);
    job.permissions
        .0
        .insert(PermissionScope::PullRequests, Access::Read);
}

pub(crate) fn add(facts: &RepoFacts, jobs: &mut Vec<Job>, from_source: bool) {
    after_saves(facts, jobs, from_source);
    if let Some(maintenance) = scheduled(facts, jobs, from_source) {
        jobs.push(maintenance);
    }
}
