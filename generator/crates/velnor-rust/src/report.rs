// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::plan::{head, job, step};
use velnor_model::{
    Access, CacheLaneOutput, HelperVersion, Job, JobId, PermissionScope, RepoFacts, Step, StepKind,
    When, cache_lane_outputs,
};

pub(crate) fn attach(facts: &RepoFacts, jobs: &mut [Job]) -> Option<Job> {
    let released = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least((0, 1, 81)));
    let from_source = facts.config.helper.is_some();
    if !released && !from_source {
        return None;
    }
    let nextest_artifacts = crate::mise::nextest_tool(&facts.tools)
        .and_then(|name| facts.tools.tools.get(name))
        .is_some_and(|pin| {
            !pin.locked.is_empty()
                && pin.locked.iter().all(|version| {
                    HelperVersion::new(version.clone())
                        .is_ok_and(|version| version.at_least((0, 9, 123)))
                })
        });
    let mut producers = Vec::new();
    let mut verified = Vec::new();
    for job in jobs {
        let saves = cache_lane_outputs(&job.steps)
            .iter()
            .any(|output| matches!(output, CacheLaneOutput::CacheOk(_)));
        if !job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Cargo(_)))
            && !saves
        {
            continue;
        }
        if job.id.as_str() == "rust-helper" && !released {
            continue;
        }
        if saves {
            verified.push(job.id.clone());
        }
        attach_metrics(job, nextest_artifacts);
        producers.push(&*job);
    }
    let needs = producers.iter().map(|job| job.id.clone()).collect();
    let needs_max_bytes = crate::required_size::needs_max_bytes(facts, producers);
    let mut steps = head(facts, 1, from_source);
    steps.extend([
        step(StepKind::DownloadMetrics),
        step(StepKind::CheckMetrics { needs_max_bytes }),
    ]);
    if !verified.is_empty() {
        steps.push(Step {
            when: When::Finalizer,
            ..step(StepKind::CacheResult(verified))
        });
    }
    Some(Job {
        name: Some("Cache".to_owned()),
        needs,
        when: When::Finalizer,
        skip_on_draft: false,
        steps,
        ..job(facts, JobId::from_static("cache"))
    })
}

fn attach_metrics(job: &mut Job, nextest_artifacts: bool) {
    job.permissions
        .0
        .entry(PermissionScope::Actions)
        .or_insert(Access::Read);

    let caches = job
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::CacheRestore(key) => Some(key.clone()),
            _ => None,
        })
        .collect();
    job.steps.extend(
        [
            StepKind::ReportCaches {
                caches,
                nextest_artifacts,
            },
            StepKind::UploadMetrics,
        ]
        .into_iter()
        .map(|kind| Step {
            when: When::Finalizer,
            ..step(kind)
        }),
    );
}

pub(crate) fn finish(
    facts: &RepoFacts,
    jobs: &mut Vec<Job>,
    single: bool,
    draft_guard: Option<Step>,
) {
    let cache = attach(facts, jobs);
    let trim = crate::base_cache::trim_job(facts, jobs);
    if facts.config.cache_check_blocks {
        jobs.extend(cache.clone());
    }
    let required = crate::required_job::verdict_job(facts, jobs, single, draft_guard);
    jobs.extend(trim);
    jobs.push(required);
    if !facts.config.cache_check_blocks {
        jobs.extend(cache);
    }
}
