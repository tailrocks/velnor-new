// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    Access, BaseExpiry, BaseScope, CacheKey, CacheRoot, HelperCommand, HelperRelease, Host,
    InputsCli, JobId, LockIdentity, PermissionScope, Permissions, PlanFrom, PrivateVerdict,
    RepoFacts, RepoPath, RootSlug, RustRoot, Step, StepKind, When,
};

use crate::lane::Lane;
use crate::plan::step;
use crate::units::Unit;

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

// helper-gate: first in tools-v0.1.111 via 193adc8c56
const PRIVATE_BASE_HELPER: (u64, u64, u64) = (0, 1, 111);
// helper-gate: first in tools-v0.1.112 via e34b20da63
const PRIVATE_PREFIX_HELPER: (u64, u64, u64) = (0, 1, 112);
// helper-gate: first in tools-v0.1.113 via f5ced2b48c
const SOURCES_RESTORED_ENV_HELPER: (u64, u64, u64) = (0, 1, 113);
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
    has_saving_helper(facts) && (facts.private_sources.is_none() || private_base(facts))
}

fn has_helper_at_least(facts: &RepoFacts, gate: (u64, u64, u64)) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(gate))
}

fn has_private_base_helper(facts: &RepoFacts) -> bool {
    has_helper_at_least(facts, PRIVATE_BASE_HELPER)
}

pub(crate) fn private_base(facts: &RepoFacts) -> bool {
    facts.private_sources.is_some() && facts.private_base_caches && has_private_base_helper(facts)
}

pub(crate) fn base_scope(facts: &RepoFacts) -> BaseScope {
    let prefixed = facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(PRIVATE_PREFIX_HELPER));
    if private_base(facts) && prefixed {
        BaseScope::PrivateRepo
    } else {
        BaseScope::Shared
    }
}

fn private_verdict(facts: &RepoFacts) -> PrivateVerdict {
    if private_base(facts) {
        PrivateVerdict::Checked
    } else if has_private_base_helper(facts) {
        PrivateVerdict::Cleared
    } else {
        PrivateVerdict::Unset
    }
}

pub(crate) fn private_only(facts: &RepoFacts, step: Step) -> Step {
    Step {
        private_repo: private_base(facts),
        ..step
    }
}

pub(crate) fn privacy_check(facts: &RepoFacts) -> Vec<Step> {
    if private_base(facts) {
        vec![
            step(StepKind::RepositoryPrivacy),
            step(StepKind::PrivateCheck { cached: Vec::new() }),
        ]
    } else {
        Vec::new()
    }
}

pub(crate) fn public_sources_step(facts: &RepoFacts) -> Option<Step> {
    (facts.config.public_sources.is_some() && writes_base(facts)).then(|| Step {
        when: When::SavingRun,
        ..step(StepKind::PublicSources {
            manifests: facts
                .rust_roots
                .iter()
                .map(|root| root.manifest.clone())
                .collect(),
            selected: crate::units::units(facts)
                .iter()
                .map(|unit| unit.gate.clone())
                .collect(),
            fails_on_private: facts
                .helper
                .as_ref()
                .is_some_and(|release| release.version().at_least(PUBLIC_PROBE_FAILS_HELPER)),
        })
    })
}

// helper-gate: first in tools-v0.1.114 via c9dfb11d50b1cbbf55273621878dd949d4a7c5f9
const PUBLIC_PROBE_FAILS_HELPER: (u64, u64, u64) = (0, 1, 114);

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
    saving_helper(facts.helper.as_ref())
}

pub(crate) fn saving_helper(helper: Option<&HelperRelease>) -> bool {
    helper.is_some_and(|helper| helper.version().at_least(SAVING_HELPER))
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
        scope: base_scope(facts),
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
                private: private_verdict(facts),
                sources_restore: has_helper_at_least(facts, SOURCES_RESTORED_ENV_HELPER)
                    .then(|| cache_root.slug().clone()),
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
                scope: base_scope(facts),
            }))
        });
    }
    steps
        .into_iter()
        .map(|saved| private_only(facts, saved))
        .map(|saved| Step {
            gate: lane.steps(),
            ..saved
        })
        .collect()
}

pub(crate) fn lane_permissions(facts: &RepoFacts) -> Permissions {
    let mut permissions = BTreeMap::from([(PermissionScope::Contents, Access::Read)]);
    if writes_base(facts) {
        permissions.insert(PermissionScope::Actions, Access::Read);
    }
    Permissions(permissions)
}
