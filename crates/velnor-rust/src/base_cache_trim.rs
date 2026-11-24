// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, CacheKey, Job, JobId, PermissionScope, Permissions, RepoFacts, Step, StepKind,
    TrimFamily, When,
};

use crate::base_cache::has_saving_helper;
use crate::plan::{job, step};

const TRIM_JOB: JobId = JobId::from_static("rust-trim");

// helper-gate: first in tools-v0.1.112 via 81eb49accb
const PRIVATE_PURGE_HELPER: (u64, u64, u64) = (0, 1, 112);

pub(crate) fn trim_job(facts: &RepoFacts, jobs: &[Job]) -> Option<Job> {
    let release = facts.helper.as_ref().filter(|_| has_saving_helper(facts))?;
    let trims: Vec<Step> = jobs.iter().flat_map(lane_trims).collect();
    let needs: Vec<JobId> = jobs
        .iter()
        .filter(|job| {
            job.steps
                .iter()
                .any(|step| matches!(step.kind, StepKind::CacheSave(_)))
        })
        .map(|job| job.id.clone())
        .collect();
    let uploads: Vec<(JobId, TrimFamily)> = jobs.iter().flat_map(lane_uploads).collect();
    let verifies = uploads.iter().map(|(lane, family)| {
        step(StepKind::CacheVerify {
            lane: lane.clone(),
            family: *family,
        })
    });
    let result = (!uploads.is_empty()).then(|| step(StepKind::CacheResult(uploads.clone())));
    let purge = release
        .version()
        .at_least(PRIVATE_PURGE_HELPER)
        .then(|| Step {
            when: When::Finalizer,
            ..step(StepKind::CachePurgePrivate)
        });
    let steps = std::iter::once(step(StepKind::InstallHelper(release.clone())))
        .chain(trims)
        .chain(verifies)
        .chain(result)
        .chain([step(StepKind::CacheBudget(facts.config.cache_budget_bytes))])
        .chain(purge)
        .collect();
    Some(Job {
        name: Some("Cache maintenance".to_owned()),
        needs,
        when: When::SaveBase,
        pr: crate::plan::quick(facts),
        permissions: Permissions(BTreeMap::from([
            (PermissionScope::Actions, Access::Write),
            (PermissionScope::PullRequests, Access::Read),
        ])),
        steps,
        ..job(facts, TRIM_JOB)
    })
}

fn is_decide(step: &Step) -> bool {
    matches!(step.kind, StepKind::CacheDecide { .. })
}

fn lane_trims(lane: &Job) -> Vec<Step> {
    lane_uploads(lane)
        .into_iter()
        .map(|(lane, family)| step(StepKind::CacheTrim { lane, family }))
        .collect()
}

fn lane_uploads(lane: &Job) -> Vec<(JobId, TrimFamily)> {
    let saves_sources = lane
        .steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::CacheSave(CacheKey::Sources { .. })));
    lane.steps
        .iter()
        .any(is_decide)
        .then(|| (lane.id.clone(), TrimFamily::Target))
        .into_iter()
        .chain(saves_sources.then(|| (lane.id.clone(), TrimFamily::Sources)))
        .collect()
}
