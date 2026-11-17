// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, BaseExpiry, CacheKey, CacheRoot, HelperCommand, Host, Job, JobId, PermissionScope,
    Permissions, RepoFacts, RepoPath, RustRoot, Step, StepKind, TrimFamily, When,
};

use crate::lane::Lane;
use crate::plan::{job, step};

const TRIM_JOB: JobId = JobId::from_static("rust-trim");

const SAVING_HELPER: (u64, u64, u64) = (0, 1, 81);

pub(crate) fn writes_base(facts: &RepoFacts) -> bool {
    facts.private_sources.is_none() && has_saving_helper(facts)
}

pub(crate) fn has_saving_helper(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(SAVING_HELPER))
}

pub(crate) fn restored(facts: &RepoFacts) -> Vec<Step> {
    let expiry = expiry(facts);
    let replan = (expiry != BaseExpiry::Never).then(|| Step {
        when: When::BaseExpired,
        ..step(StepKind::Helper(HelperCommand::PlanFull))
    });
    std::iter::once(step(StepKind::Helper(HelperCommand::Restored(expiry))))
        .chain(replan)
        .collect()
}

pub(crate) fn saving_run(run: Step) -> Step {
    Step {
        when: When::SavingRun,
        ..run
    }
}

fn expiry(facts: &RepoFacts) -> BaseExpiry {
    if !writes_base(facts) {
        return BaseExpiry::Never;
    }
    BaseExpiry::OnDefaultBranch
}

pub(crate) fn save_steps(
    facts: &RepoFacts,
    root: &RustRoot,
    lane: &Lane,
    job: &JobId,
    archive: Option<RepoPath>,
    sources: bool,
) -> Vec<Step> {
    if !writes_base(facts) {
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
            ..step(StepKind::InputsSave(root.target_dir.clone()))
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
            when: When::SaveBaseDecided,
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

pub(crate) fn trim_job(facts: &RepoFacts, jobs: &[Job]) -> Option<Job> {
    let release = facts.helper.as_ref().filter(|_| has_saving_helper(facts))?;
    let trims: Vec<Step> = jobs.iter().flat_map(lane_trims).collect();
    if trims.is_empty() {
        return None;
    }
    let lanes: Vec<JobId> = jobs
        .iter()
        .filter(|lane| lane.steps.iter().any(is_decide))
        .map(|lane| lane.id.clone())
        .collect();
    let steps = std::iter::once(step(StepKind::InstallHelper(release.clone())))
        .chain(trims)
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

fn is_decide(step: &Step) -> bool {
    matches!(step.kind, StepKind::CacheDecide { .. })
}

fn lane_trims(lane: &Job) -> Vec<Step> {
    let family = |family| {
        step(StepKind::CacheTrim {
            lane: lane.id.clone(),
            family,
        })
    };
    let saves_sources = lane
        .steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::CacheSave(CacheKey::Sources { .. })));
    lane.steps
        .iter()
        .any(is_decide)
        .then(|| family(TrimFamily::Target))
        .into_iter()
        .chain(saves_sources.then(|| family(TrimFamily::Sources)))
        .collect()
}
