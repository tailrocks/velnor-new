// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::audit_job::AUDIT_JOB;
use crate::plan::{HELPER_JOB, job, step};
use crate::plan_tools::TOOLS_SAVED_KEY_HELPER;
use velnor_model::{
    Access, CI_RUN_LABEL, CacheKey, HelperVersion, Job, JobId, PermissionScope, Permissions, PrCi,
    PrRun, PushAfterMergeGroup, RepoFacts, Step, StepKind, ToolsLocalVerb, ToolsTelemetry, When,
};

// helper-gate: first in tools-v0.1.111 via 2c70d48ddc
const NOT_GENERATED_HELPER: (u64, u64, u64) = (0, 1, 111);

fn not_generated(facts: &RepoFacts) -> Option<&'static str> {
    let refused = facts.private_sources.is_some()
        && facts
            .helper
            .as_ref()
            .is_some_and(|helper| helper.version().at_least(NOT_GENERATED_HELPER))
        && !crate::base_cache::writes_base(facts);
    refused.then_some("private dependencies")
}

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
    let not_generated = not_generated(facts);
    let saved_keys = facts
        .helper
        .as_ref()
        .is_some_and(|release| release.version().at_least(TOOLS_SAVED_KEY_HELPER));
    let mut producers = Vec::new();
    for job in jobs {
        let saves = job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::CacheSave(_)));
        if !job.steps.iter().any(|step| step.kind.runs_cargo()) && !saves {
            continue;
        }
        let reason = not_generated.filter(|_| job.id != AUDIT_JOB);
        attach_metrics(job, nextest_artifacts, saved_keys, reason);
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

fn attach_metrics(
    job: &mut Job,
    nextest_artifacts: bool,
    saved_keys: bool,
    not_generated: Option<&'static str>,
) {
    let caches: Vec<CacheKey> = job
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::CacheRestore(key) => Some(key.clone()),
            _ => None,
        })
        .collect();
    let saves_cache = job
        .steps
        .iter()
        .any(|step| matches!(&step.kind, StepKind::CacheSave(key) if key.family() != "tools"));
    let not_generated = not_generated.filter(|_| !saves_cache).map(str::to_owned);
    let unsuppressed: Vec<CacheKey> = caches
        .iter()
        .filter(|key| job.id == HELPER_JOB && matches!(key, CacheKey::Target { .. }))
        .cloned()
        .collect();
    let has = |verb| {
        job.steps
            .iter()
            .any(|step| matches!(&step.kind, StepKind::ToolsLocal(found, _) if *found == verb))
    };
    let node_tools = ToolsTelemetry {
        restore: has(ToolsLocalVerb::RecordRestore),
        save: has(ToolsLocalVerb::RecordSave),
        saved_key: has(ToolsLocalVerb::RecordSavedKey),
    };
    job.steps.extend(
        [
            StepKind::ReportCaches {
                caches,
                nextest_artifacts,
                node_tools,
                saved_keys,
                not_generated,
                unsuppressed,
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
