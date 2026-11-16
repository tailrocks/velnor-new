// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::plan::{head, job, step};
use velnor_model::{
    Access, HelperVersion, Job, JobId, PermissionScope, RepoFacts, Step, StepKind, When,
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
    let mut needs = Vec::new();
    for job in jobs {
        if !job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Cargo(_)))
        {
            continue;
        }
        if job.id.as_str() == "rust-helper" && !released {
            continue;
        }
        job.permissions
            .0
            .entry(PermissionScope::Actions)
            .or_insert(Access::Read);
        needs.push(job.id.clone());
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
    let mut steps = head(facts, 1, from_source);
    steps.extend([
        step(StepKind::DownloadMetrics),
        step(StepKind::CheckMetrics),
    ]);
    Some(Job {
        name: Some("Cache".to_owned()),
        needs,
        when: When::Finalizer,
        skip_on_draft: false,
        steps,
        ..job(facts, JobId::from_static("cache"))
    })
}

pub(crate) fn finish(facts: &RepoFacts, jobs: &mut Vec<Job>, draft_guard: Option<Step>) {
    let cache = attach(facts, jobs);
    let mut verdict_needs: Vec<JobId> = jobs.iter().map(|job| job.id.clone()).collect();
    let trim = crate::base_cache::trim_job(facts, jobs, facts.config.helper.is_some());
    jobs.extend(trim);
    if facts.config.cache_check_blocks {
        verdict_needs.extend(cache.iter().map(|job| job.id.clone()));
        jobs.extend(cache.clone());
    }
    jobs.push(Job {
        name: Some(facts.config.required_check.clone()),
        skip_on_draft: false,
        needs: verdict_needs,
        when: When::NotCancelled,
        steps: draft_guard
            .into_iter()
            .chain([step(StepKind::FailWhenANeedFailed)])
            .collect(),
        ..job(facts, JobId::from_static("required"))
    });
    if !facts.config.cache_check_blocks {
        jobs.extend(cache);
    }
}
