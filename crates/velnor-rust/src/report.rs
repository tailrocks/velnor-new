// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::plan::{job, step};
use velnor_model::{
    Access, CI_RUN_LABEL, HelperVersion, Job, JobId, PermissionScope, Permissions, PrCi, PrRun,
    PushAfterMergeGroup, RepoFacts, Step, StepKind, ToolsLocalVerb, ToolsTelemetry, When,
};

fn attach(facts: &RepoFacts, jobs: &mut [Job], maintenance: Option<&Job>) -> Job {
    let nextest_artifacts = crate::mise::nextest_tool(&facts.tools)
        .and_then(|name| facts.tools.tools.get(name))
        .is_some_and(|pin| {
            !pin.locked.is_empty()
                && pin.locked.iter().all(|lock| {
                    HelperVersion::new(lock.version.clone())
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
        pr: crate::plan::quick(facts),
        steps,
        ..job(facts, JobId::from_static("cache"))
    };
    cache
        .permissions
        .0
        .insert(PermissionScope::Actions, Access::Read);
    cache
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
    let has = |verb| {
        job.steps
            .iter()
            .any(|step| step.kind == StepKind::ToolsLocal(verb))
    };
    let node_tools = ToolsTelemetry {
        restore: has(ToolsLocalVerb::RecordRestore),
        save: has(ToolsLocalVerb::RecordSave),
    };
    job.steps.extend(
        [
            StepKind::ReportCaches {
                caches,
                nextest_artifacts,
                node_tools,
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

fn label_reset(facts: &RepoFacts) -> Option<Job> {
    (facts.config.pr_ci == PrCi::Label).then(|| Job {
        name: Some(format!("Remove the {CI_RUN_LABEL} label")),
        needs: vec![crate::required_job::VERDICT_JOB],
        when: When::ClearRunLabel,
        pr: PrRun::Quick,
        permissions: Permissions(BTreeMap::from([(
            PermissionScope::PullRequests,
            Access::Write,
        )])),
        steps: vec![step(StepKind::ClearRunLabel)],
        ..job(facts, JobId::from_static("ci-run-label"))
    })
}

pub(crate) fn finish(
    facts: &RepoFacts,
    jobs: &mut Vec<Job>,
    single: bool,
    draft_guard: Option<Step>,
) {
    if facts.config.push_after_merge_group == PushAfterMergeGroup::SkipWhenQueueRunPassed {
        crate::merge_group_skip::ask(facts, jobs, single);
    }
    crate::merge_queue_check::ask(facts, jobs, single);
    let trim = crate::base_cache::trim_job(facts, jobs);
    let cache = attach(facts, jobs, trim.as_ref());
    if facts.config.cache_check_blocks {
        jobs.push(cache.clone());
    }
    let required = crate::required_job::verdict_job(facts, jobs, single, draft_guard);
    let budget = crate::plan_budget::budget_job(facts, jobs, single, &required.id);
    jobs.extend(trim);
    jobs.push(required);
    jobs.extend(budget);
    jobs.extend(label_reset(facts));
    if !facts.config.cache_check_blocks {
        jobs.push(cache);
    }
}
