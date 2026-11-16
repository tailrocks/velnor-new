// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, BaseExpiry, CacheKey, CacheRoot, FullRun, HelperCommand, Host, Job, JobId, OnMain,
    PermissionScope, Permissions, RepoFacts, RepoPath, RustRoot, Step, StepKind, When,
};

use crate::lane::Lane;
use crate::plan::{job, step};

const TRIM_JOB: JobId = JobId::from_static("rust-trim");

const SAVING_HELPER: (u64, u64, u64) = (0, 1, 81);

pub(crate) fn writes_base(facts: &RepoFacts, lane: &Lane) -> bool {
    lane.from_source || has_saving_helper(facts)
}

pub(crate) fn has_saving_helper(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(SAVING_HELPER))
}

pub(crate) fn expiry(facts: &RepoFacts, lane: &Lane) -> BaseExpiry {
    if !writes_base(facts, lane) {
        return BaseExpiry::Never;
    }
    match (facts.config.on_main, &facts.config.full_run) {
        (OnMain::Affected, FullRun::Off) | (OnMain::Everything, _) => BaseExpiry::OnDefaultBranch,
        (OnMain::Affected, _) => BaseExpiry::OnDefaultBranchExceptPush,
    }
}

pub(crate) fn save_steps(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    job: &JobId,
    archive: Option<RepoPath>,
    sources: bool,
) -> Vec<Step> {
    if !writes_base(facts, lane) {
        return Vec::new();
    }
    let cache_root = CacheRoot::new(&root.manifest, root.target_dir.clone());
    let target = CacheKey::Target {
        job: job.clone(),
        root: cache_root.clone(),
        host: Host,
    };
    let mut steps = vec![
        Step {
            directory: root.manifest.parent(),
            when: When::SaveBase,
            ..step(StepKind::Helper(HelperCommand::InputsSave))
        },
        Step {
            when: When::SaveBase,
            ..step(StepKind::CacheDecide {
                target: root.target_dir.clone(),
                archive,
            })
        },
        Step {
            when: When::SaveBaseDecided,
            ..step(StepKind::CacheSave(target))
        },
    ];
    if sources {
        steps.push(Step {
            when: When::SaveBase,
            ..step(StepKind::CacheSave(CacheKey::Sources {
                host: Host,
                root: cache_root,
            }))
        });
    }
    steps
        .into_iter()
        .map(|saved| Step {
            gate: lane.steps(),
            ..saved
        })
        .collect()
}

fn trim_helper(facts: &RepoFacts, from_source: bool) -> Option<Step> {
    match &facts.helper {
        Some(release) if has_saving_helper(facts) => {
            Some(step(StepKind::InstallHelper(release.clone())))
        }
        _ => from_source.then(|| step(StepKind::HelperFromArtifact)),
    }
}

pub(crate) fn trim_job(facts: &RepoFacts, jobs: &[Job], from_source: bool) -> Option<Job> {
    let lanes: Vec<JobId> = jobs
        .iter()
        .filter(|lane| {
            lane.steps
                .iter()
                .any(|step| matches!(step.kind, StepKind::CacheDecide { .. }))
        })
        .map(|lane| lane.id.clone())
        .collect();
    if lanes.is_empty() {
        return None;
    }
    let steps = std::iter::once(trim_helper(facts, from_source)?)
        .chain(
            lanes
                .iter()
                .map(|lane| step(StepKind::CacheTrim { lane: lane.clone() })),
        )
        .collect();
    Some(Job {
        needs: lanes,
        when: When::SaveBase,
        skip_on_draft: false,
        permissions: Permissions(BTreeMap::from([(PermissionScope::Actions, Access::Write)])),
        steps,
        ..job(facts, TRIM_JOB)
    })
}
