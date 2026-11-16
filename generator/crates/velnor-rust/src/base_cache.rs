// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BaseExpiry, CacheKey, CacheRoot, FullRun, HelperCommand, Host, JobId, OnMain, RepoFacts,
    RepoPath, RustRoot, Step, StepKind, When,
};

use crate::lane::Lane;
use crate::plan::step;

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
            ..step(StepKind::CacheSave(target.clone()))
        },
        Step {
            when: When::SaveBaseDecided,
            ..step(StepKind::CacheTrim(target))
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
