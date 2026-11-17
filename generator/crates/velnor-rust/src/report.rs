// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::plan::{job, step};
use velnor_model::{
    Access, HelperVersion, Job, JobId, PermissionScope, RepoFacts, Step, StepKind, When,
};

fn attach(facts: &RepoFacts, jobs: &mut [Job], maintenance: Option<&Job>) -> Option<Job> {
    let released = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least((0, 1, 81)));
    if !released {
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
    for job in jobs {
        let saves = job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::CacheSave(_)));
        if !job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Cargo(_)))
            && !saves
        {
            continue;
        }
        attach_metrics(job, nextest_artifacts);
        producers.push(&*job);
    }
    producers.extend(maintenance);
    let needs = producers.iter().map(|job| job.id.clone()).collect();
    let needs_max_bytes = crate::required_size::needs_max_bytes(facts, producers);
    let mut steps: Vec<_> = facts
        .helper
        .iter()
        .map(|release| step(StepKind::InstallHelper(release.clone())))
        .collect();
    steps.extend([
        step(StepKind::DownloadMetrics),
        step(StepKind::CheckMetrics {
            needs_max_bytes,
            trusted_api: true,
        }),
    ]);
    let mut cache = Job {
        name: Some("Cache".to_owned()),
        needs,
        when: When::Finalizer,
        skip_on_draft: false,
        steps,
        ..job(facts, JobId::from_static("cache"))
    };
    cache
        .permissions
        .0
        .insert(PermissionScope::Actions, Access::Read);
    Some(cache)
}

fn attach_metrics(job: &mut Job, nextest_artifacts: bool) {
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
    let trim = crate::base_cache::trim_job(facts, jobs);
    let cache = attach(facts, jobs, trim.as_ref());
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
