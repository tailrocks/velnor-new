// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    AnchorSource, CacheKey, Host, JobId, MbxStore, RepoFacts, RunnerLabel, Step, StepKind,
    ToolsAnchor, ToolsLocalVerb, VerifiedSave, When,
};

use crate::plan::{TOOLS_LOCAL_HELPER, step};

/// The first `velnor-ci` with `tools-verify`, which checks every tools restore against the anchor
/// a trusted run recorded and discards a restore that differs.
// First released in tools-v0.1.109 (not tagged yet when this gate was written).
pub const TOOLS_VERIFY_HELPER: (u64, u64, u64) = (0, 1, 109);

// First released in tools-v0.1.110 (not tagged yet when this gate was written).
const TOOLS_CLEAR_HELPER: (u64, u64, u64) = (0, 1, 110);

// First released in tools-v0.1.110 (not tagged yet when this gate was written).
const TOOLS_ANCHOR_HELPER: (u64, u64, u64) = (0, 1, 110);

pub(crate) fn tools_cache_key(anchor: ToolsAnchor) -> CacheKey {
    CacheKey::Tools { host: Host, anchor }
}

fn tools_anchored(facts: &RepoFacts) -> bool {
    tools_verified(facts)
        && facts
            .helper
            .as_ref()
            .is_some_and(|release| release.version().at_least(TOOLS_ANCHOR_HELPER))
}

fn restored_anchor(facts: &RepoFacts, plan: Option<&JobId>) -> ToolsAnchor {
    if !tools_anchored(facts) {
        return ToolsAnchor::Unanchored;
    }
    ToolsAnchor::Restored(plan.map_or(AnchorSource::ThisJob, |job| AnchorSource::Job(job.clone())))
}

pub(crate) fn tools_anchor_read(facts: &RepoFacts) -> Vec<Step> {
    if facts.tools.tools.is_empty() || !tools_anchored(facts) {
        return Vec::new();
    }
    [ToolsLocalVerb::AnchorFind, ToolsLocalVerb::AnchorRead]
        .map(|verb| step(StepKind::ToolsLocal(verb, ToolsAnchor::Unanchored)))
        .into()
}

#[must_use]
pub fn tools_verified(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|release| release.version().at_least(TOOLS_VERIFY_HELPER))
}

/// The note for a lock whose helper restores the tools without verifying them.
#[must_use]
pub fn unverified_tools_note(facts: &RepoFacts) -> Option<String> {
    (!facts.tools.tools.is_empty() && facts.helper.is_some() && !tools_verified(facts)).then(|| {
        format!(
            "the pinned velnor-ci restores the tools cache (~/.local/share/mise, ~/.rustup) and runs it unverified; pin velnor-ci {}.{}.{} or newer so every restore is checked against the anchor of a trusted run and discarded when it differs",
            TOOLS_VERIFY_HELPER.0, TOOLS_VERIFY_HELPER.1, TOOLS_VERIFY_HELPER.2
        )
    })
}

fn tools_clears(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|release| release.version().at_least(TOOLS_CLEAR_HELPER))
}

fn tools_step(verb: ToolsLocalVerb, anchor: ToolsAnchor, when: When) -> Step {
    Step {
        when,
        ..step(StepKind::ToolsLocal(verb, anchor))
    }
}

fn verified_when(facts: &RepoFacts, plain: When, verified: VerifiedSave) -> When {
    if !tools_verified(facts) {
        return plain;
    }
    When::SaveVerifiedTools(match (tools_anchored(facts), verified) {
        (true, VerifiedSave::OnDefaultBranch) => VerifiedSave::AnchoredOnDefaultBranch,
        (true, VerifiedSave::UnlessLocal) => VerifiedSave::AnchoredUnlessLocal,
        (_, verified) => verified,
    })
}

/// A verified restore for a repository whose runner is self-hosted. Restoring the 590 to 620 MB
/// tools archive from the Actions cache took 31 to 66 s on a scale-set runner, while installing
/// the same tools from scratch took 9 to 18 s, so such a job takes the tools from the node store
/// and installs them on a miss. The Actions cache stays in place for a GitHub-hosted runner, which
/// a job of the same workflow can still land on (a fork pull request), through `hosted_only`.
fn node_first(facts: &RepoFacts) -> bool {
    tools_verified(facts) && !facts.config.hosted_runners.contains(&facts.config.runner)
}

fn tools_local(facts: &RepoFacts) -> bool {
    (facts.config.mbx_store == MbxStore::Shared || node_first(facts))
        && facts
            .helper
            .as_ref()
            .is_some_and(|release| release.version().at_least(TOOLS_LOCAL_HELPER))
}

pub(crate) fn tools_restore(facts: &RepoFacts, plan: Option<&JobId>) -> Vec<Step> {
    if facts.tools.tools.is_empty() {
        return Vec::new();
    }
    let local = tools_local(facts);
    let anchor = restored_anchor(facts, plan);
    let network = Step {
        when: if local {
            When::RestoreToolsUnlessLocal
        } else {
            When::Success
        },
        hosted_only: node_first(facts),
        ..step(StepKind::CacheRestore(tools_cache_key(anchor.clone())))
    };
    let local_step = |verb| step(StepKind::ToolsLocal(verb, anchor.clone()));
    let mut steps: Vec<Step> = plan
        .is_none()
        .then(|| tools_anchor_read(facts))
        .into_iter()
        .flatten()
        .chain(tools_clears(facts).then(|| {
            step(StepKind::ToolsLocal(
                ToolsLocalVerb::Clear,
                ToolsAnchor::Unanchored,
            ))
        }))
        .chain(local.then(|| local_step(ToolsLocalVerb::Restore)))
        .chain([network])
        .collect();
    if tools_verified(facts) {
        if anchor == ToolsAnchor::Unanchored {
            steps.extend([
                tools_step(ToolsLocalVerb::AnchorReset, anchor.clone(), When::Success),
                tools_step(ToolsLocalVerb::AnchorRestore, anchor.clone(), When::Success),
            ]);
        }
        steps.push(tools_step(
            ToolsLocalVerb::Verify,
            anchor.clone(),
            When::Success,
        ));
    }
    steps.extend(local.then(|| local_step(ToolsLocalVerb::RecordRestore)));
    steps
}

/// On a runner that only runs archived tests, nothing else saves the tools it restored from the
/// network cache to the node store, so the node stays cold for the lanes that would read it.
pub(crate) fn tools_prime(facts: &RepoFacts, plan: Option<&JobId>) -> Option<Step> {
    (!facts.tools.tools.is_empty() && tools_local(facts)).then(|| Step {
        when: verified_when(facts, When::PrimeToolsFromNetwork, VerifiedSave::Prime),
        hosted_only: node_first(facts),
        ..step(StepKind::ToolsLocal(
            ToolsLocalVerb::Prime,
            restored_anchor(facts, plan),
        ))
    })
}

pub(crate) fn tools_save(facts: &RepoFacts, plan: Option<&JobId>) -> Vec<Step> {
    if facts.tools.tools.is_empty() {
        return Vec::new();
    }
    let local = tools_local(facts);
    let restored = restored_anchor(facts, plan);
    let saved = if restored == ToolsAnchor::Unanchored {
        ToolsAnchor::Unanchored
    } else {
        ToolsAnchor::Recorded
    };
    let network = Step {
        when: if local {
            verified_when(facts, When::SaveToolsUnlessLocal, VerifiedSave::UnlessLocal)
        } else {
            verified_when(
                facts,
                When::SaveToolsOnDefaultBranch,
                VerifiedSave::OnDefaultBranch,
            )
        },
        hosted_only: node_first(facts),
        ..step(StepKind::CacheSave(tools_cache_key(saved.clone())))
    };
    let anchor: Vec<Step> = if tools_verified(facts) {
        vec![
            tools_step(ToolsLocalVerb::Record, restored.clone(), When::Success),
            tools_step(
                ToolsLocalVerb::AnchorSave,
                saved.clone(),
                When::SaveVerifiedTools(VerifiedSave::Anchor),
            ),
        ]
    } else {
        Vec::new()
    };
    if !local {
        return anchor.into_iter().chain([network]).collect();
    }
    let checked = crate::base_cache::checks_saves(facts);
    let saving = |verb| Step {
        when: if verb == ToolsLocalVerb::RecordSave {
            When::SavingRun
        } else {
            verified_when(facts, When::SavingRun, VerifiedSave::NodeStore)
        },
        ..step(StepKind::ToolsLocal(verb, saved.clone()))
    };
    // A node hit skips the network restore, so the network entry is looked up here: an evicted
    // entry is saved again by the first trusted run that finds the node store warm.
    let lookup = Step {
        when: When::LookupToolsOnNodeHit,
        hosted_only: node_first(facts),
        ..step(StepKind::CacheLookup(tools_cache_key(restored)))
    };
    let mut steps = anchor;
    steps.extend([
        lookup,
        saving(if checked {
            ToolsLocalVerb::SaveChecked
        } else {
            ToolsLocalVerb::Save
        }),
        network,
    ]);
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
