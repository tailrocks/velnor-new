// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    CacheKey, Host, MbxStore, RepoFacts, RunnerLabel, Step, StepKind, ToolsLocalVerb, When,
};

use crate::plan::{TOOLS_LOCAL_HELPER, step};

pub(crate) fn tools_cache_key() -> CacheKey {
    CacheKey::Tools { host: Host }
}

fn tools_local(facts: &RepoFacts) -> bool {
    facts.config.mbx_store == MbxStore::Shared
        && facts
            .helper
            .as_ref()
            .is_some_and(|release| release.version().at_least(TOOLS_LOCAL_HELPER))
}

pub(crate) fn tools_restore(facts: &RepoFacts) -> Vec<Step> {
    if facts.tools.tools.is_empty() {
        return Vec::new();
    }
    let local = tools_local(facts);
    let network = Step {
        when: if local {
            When::RestoreToolsUnlessLocal
        } else {
            When::Success
        },
        ..step(StepKind::CacheRestore(tools_cache_key()))
    };
    let record = || step(StepKind::ToolsLocal(ToolsLocalVerb::RecordRestore));
    if local {
        vec![
            step(StepKind::ToolsLocal(ToolsLocalVerb::Restore)),
            network,
            record(),
        ]
    } else {
        vec![network]
    }
}

/// On a runner that only runs archived tests, nothing else saves the tools it restored from the
/// network cache to the node store, so the node stays cold for the lanes that would read it.
pub(crate) fn tools_prime(facts: &RepoFacts) -> Option<Step> {
    (!facts.tools.tools.is_empty() && tools_local(facts)).then(|| Step {
        when: When::PrimeToolsFromNetwork,
        ..step(StepKind::ToolsLocal(ToolsLocalVerb::Prime))
    })
}

pub(crate) fn tools_save(facts: &RepoFacts) -> Vec<Step> {
    if facts.tools.tools.is_empty() {
        return Vec::new();
    }
    let local = tools_local(facts);
    let network = Step {
        when: if local {
            When::SaveToolsUnlessLocal
        } else {
            When::SaveToolsOnDefaultBranch
        },
        ..step(StepKind::CacheSave(tools_cache_key()))
    };
    if !local {
        return vec![network];
    }
    let checked = crate::base_cache::checks_saves(facts);
    let saving = |verb| Step {
        when: When::SavingRun,
        ..step(StepKind::ToolsLocal(verb))
    };
    let mut steps = vec![
        saving(if checked {
            ToolsLocalVerb::SaveChecked
        } else {
            ToolsLocalVerb::Save
        }),
        network,
    ];
    steps.extend(checked.then(|| saving(ToolsLocalVerb::RecordSave)));
    steps
}

pub(crate) fn tools_install(facts: &RepoFacts) -> Option<Step> {
    (!facts.tools.tools.is_empty()).then(|| step(StepKind::InstallTools { only: None }))
}

pub(crate) fn mirror_step(facts: &RepoFacts) -> Option<Step> {
    mirror_step_on(facts, &facts.config.runner)
}

pub(crate) fn mirror_step_on(facts: &RepoFacts, runner: &RunnerLabel) -> Option<Step> {
    facts
        .config
        .crates_mirror
        .as_ref()
        .filter(|mirror| mirror.runner == *runner)
        .map(|mirror| step(StepKind::CratesMirror(mirror.clone())))
}
