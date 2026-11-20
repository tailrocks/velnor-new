// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, BaseExpiry, CacheKey, CacheRoot, HelperCommand, Host, InputsCli, Job, JobId,
    LockIdentity, PermissionScope, Permissions, RepoFacts, RepoPath, RootSlug, RustRoot, Step,
    PlanFrom, StepKind, TrimFamily, When,
};

use crate::lane::Lane;
use crate::plan::{job, step};
use crate::units::Unit;

const TRIM_JOB: JobId = JobId::from_static("rust-trim");

const SAVING_HELPER: (u64, u64, u64) = (0, 1, 81);
const BATCHED_HELPER: (u64, u64, u64) = (0, 1, 97);
/// The first `velnor-ci` that writes `cold=true` from `prepare` when it cannot read the OS
/// identity, and that exits non-zero when a trusted node-store save fails.
const CHECKED_SAVES_HELPER: (u64, u64, u64) = (0, 1, 100);

pub(crate) fn checks_saves(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(CHECKED_SAVES_HELPER))
}

pub(crate) fn inputs_cli(facts: &RepoFacts) -> InputsCli {
    let batched = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(BATCHED_HELPER));
    if batched {
        InputsCli::Batched
    } else {
        InputsCli::PerRoot
    }
}

const SOURCES_FETCH_HELPER: (u64, u64, u64) = (0, 1, 88);
// helper-gate: first in tools-v0.1.88 via 1a97547af5
const MEMBER_LOCK_HELPER: (u64, u64, u64) = (0, 1, 88);

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

/// The one lookup of the declared public git dependencies for the whole run, in the job that plans
/// it. On the same events as the base save, so a pull request never makes the lookup.
pub(crate) fn public_sources_step(facts: &RepoFacts) -> Option<Step> {
    (facts.config.public_sources.is_some() && writes_base(facts)).then(|| Step {
        when: When::SavingRun,
        ..step(StepKind::PublicSources {
            manifests: facts
                .rust_roots
                .iter()
                .map(|root| root.manifest.clone())
                .collect(),
        })
    })
}

pub const MAX_SOURCE_ROOTS: usize = 16;

#[must_use]
pub fn source_roots_over_limit(facts: &RepoFacts) -> Option<usize> {
    (writes_base(facts)
        && !fetches_all_roots_at_once(facts)
        && facts.rust_roots.len() > MAX_SOURCE_ROOTS)
        .then_some(facts.rust_roots.len())
}

fn fetches_all_roots_at_once(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(SOURCES_FETCH_HELPER))
}

fn sources_fetches(facts: &RepoFacts, lane: &RootSlug) -> Vec<Step> {
    let fetch = |kind, directory| Step {
        directory,
        when: When::FetchSources,
        ..step(kind)
    };
    if fetches_all_roots_at_once(facts) {
        return vec![fetch(
            StepKind::SourcesFetchAll {
                lane: lane.clone(),
                manifests: facts
                    .rust_roots
                    .iter()
                    .map(|root| root.manifest.clone())
                    .collect(),
            },
            None,
        )];
    }
    facts
        .rust_roots
        .iter()
        .enumerate()
        .map(|(position, other)| {
            fetch(
                StepKind::SourcesFetch {
                    lane: lane.clone(),
                    position,
                },
                other.manifest.parent(),
            )
        })
        .collect()
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
            targets: crate::units::units(facts)
                .iter()
                .map(|unit| unit.target_dir.clone())
                .collect(),
            cli: inputs_cli(facts),
        }
    } else {
        StepKind::Helper(HelperCommand::Restored(expiry))
    };
    std::iter::once(step(restore)).chain(replan).collect()
}

pub(crate) fn sources_locks(facts: &RepoFacts) -> Step {
    let mut locks: Vec<RepoPath> = facts
        .rust_roots
        .iter()
        .map(|root| root.manifest.sibling(&RepoPath::from_static("Cargo.lock")))
        .collect();
    locks.sort();
    locks.dedup();
    step(StepKind::SourcesLocks(locks))
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
    unit: &Unit,
    lane: &Lane,
    job: &JobId,
    archive: Option<RepoPath>,
    sources: bool,
) -> Vec<Step> {
    if !writes_base(facts) {
        return Vec::new();
    }
    let cache_root = unit.cache_root(facts);
    let target = CacheKey::Target {
        job: job.clone(),
        root: cache_root.clone(),
        host: Host,
    };
    let mut steps = vec![
        Step {
            directory: unit.directory(),
            when: When::SaveBase,
            ..step(StepKind::InputsSave {
                target: unit.target_dir.clone(),
                cli: inputs_cli(facts),
            })
        },
        Step {
            when: When::SaveBase,
            ..step(StepKind::CacheDecide {
                manifests: facts
                    .rust_roots
                    .iter()
                    .map(|root| root.manifest.clone())
                    .collect(),
                target: unit.target_dir.clone(),
                archive,
                cli: inputs_cli(facts),
                budget_bytes: facts.config.cache_budget_bytes,
                verdicts: facts
                    .config
                    .public_sources
                    .as_ref()
                    .map(|_| lane.plan.clone().map_or(PlanFrom::ThisJob, PlanFrom::Job)),
            })
        },
        Step {
            when: When::SaveBaseDecided,
            skip_when_cold: checks_saves(facts),
            ..step(StepKind::CacheSave(target))
        },
    ];
    if sources {
        steps.extend(sources_fetches(facts, cache_root.slug()));
        steps.push(Step {
            when: When::SaveSources,
            ..step(StepKind::CacheSave(CacheKey::Sources {
                host: Host,
                lane: cache_root,
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

pub(crate) fn lane_permissions(facts: &RepoFacts) -> Permissions {
    let mut permissions = BTreeMap::from([(PermissionScope::Contents, Access::Read)]);
    if writes_base(facts) {
        permissions.insert(PermissionScope::Actions, Access::Read);
    }
    Permissions(permissions)
}
