// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, BaseExpiry, CacheKey, CacheRoot, HelperCommand, Host, Job, JobId, LockIdentity,
    PermissionScope, Permissions, RepoFacts, RepoPath, RustRoot, Step, StepKind, TrimFamily, When,
};

use crate::lane::Lane;
use crate::plan::{job, step};

const TRIM_JOB: JobId = JobId::from_static("rust-trim");

const SAVING_HELPER: (u64, u64, u64) = (0, 1, 81);
const MEMBER_LOCK_HELPER: (u64, u64, u64) = (0, 1, 85);

pub(crate) fn cache_root(facts: &RepoFacts, root: &RustRoot) -> CacheRoot {
    let lock = if facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(MEMBER_LOCK_HELPER))
    {
        LockIdentity::Members
    } else {
        LockIdentity::File
    };
    CacheRoot::new(&root.manifest, root.target_dir.clone()).with_lock_identity(lock)
}

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
    let restore = if has_saving_helper(facts) {
        StepKind::RestoredTargets {
            expiry,
            targets: facts
                .rust_roots
                .iter()
                .map(|root| root.target_dir.clone())
                .collect(),
        }
    } else {
        StepKind::Helper(HelperCommand::Restored(expiry))
    };
    std::iter::once(step(restore)).chain(replan).collect()
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
    let cache_root = cache_root(facts, root);
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
                manifests: facts
                    .rust_roots
                    .iter()
                    .map(|root| root.manifest.clone())
                    .collect(),
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
            when: When::SaveSources,
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
    let steps = std::iter::once(step(StepKind::InstallHelper(release.clone())))
        .chain(trims)
        .chain(verifies)
        .chain(result)
        .chain([step(StepKind::CacheBudget(facts.config.cache_budget_bytes))])
        .collect();
    Some(Job {
        name: Some("Cache maintenance".to_owned()),
        needs,
        when: When::SaveBase,
        skip_on_draft: false,
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

pub(crate) fn lane_permissions(facts: &RepoFacts) -> Permissions {
    let mut permissions = BTreeMap::from([(PermissionScope::Contents, Access::Read)]);
    if writes_base(facts) {
        permissions.insert(PermissionScope::Actions, Access::Read);
    }
    Permissions(permissions)
}
