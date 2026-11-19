// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::lane::Lane;
use crate::plan::step;
use crate::units::Unit;
use velnor_model::{CacheKey, CacheRoot, Host, JobId, RepoFacts, RustRoot, Step, StepKind, When};

const DELTA_HELPER: (u64, u64, u64) = (0, 1, 95);

pub(crate) fn restore(facts: &RepoFacts, unit: &Unit, job: &JobId, lane: &Lane) -> Vec<Step> {
    let root = &unit.roots[0];
    if unit.grouped || !supported(facts, root, lane.deltas) {
        return Vec::new();
    }
    let root = CacheRoot::new(&root.manifest, root.target_dir.clone());
    vec![
        StepKind::DeltaPrepare(root.clone()),
        StepKind::CacheRestore(CacheKey::Delta {
            job: job.clone(),
            root: root.clone(),
            host: Host,
        }),
        StepKind::DeltaApply(root),
    ]
    .into_iter()
    .map(|kind| Step {
        when: When::PullRequest,
        ..step(kind)
    })
    .collect()
}

pub(crate) fn append(
    steps: &mut Vec<Step>,
    facts: &RepoFacts,
    unit: &Unit,
    lane: &Lane,
    job: &JobId,
) {
    if unit.grouped {
        return;
    }
    if steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::CacheDecide { .. }))
    {
        steps.extend(save(facts, &unit.roots[0], lane, job));
    }
}

pub(crate) fn save(facts: &RepoFacts, root: &RustRoot, lane: &Lane, job: &JobId) -> Vec<Step> {
    if !supported(facts, root, lane.deltas) {
        return Vec::new();
    }
    let cache_root = CacheRoot::new(&root.manifest, root.target_dir.clone());
    vec![
        Step {
            directory: root.manifest.parent(),
            when: When::SaveDelta,
            ..step(StepKind::InputsSave {
                target: root.target_dir.clone(),
                cli: crate::base_cache::inputs_cli(facts),
            })
        },
        Step {
            when: When::SaveDelta,
            ..step(StepKind::DeltaStage(
                cache_root.clone(),
                root.members
                    .iter()
                    .map(|member| member.name.clone())
                    .collect(),
            ))
        },
        Step {
            when: When::SaveDeltaDecided,
            ..step(StepKind::CacheSave(CacheKey::Delta {
                job: job.clone(),
                root: cache_root,
                host: Host,
            }))
        },
    ]
    .into_iter()
    .map(|saved| Step {
        gate: lane.steps(),
        ..saved
    })
    .collect()
}

fn supported(facts: &RepoFacts, root: &RustRoot, deltas: bool) -> bool {
    deltas
        && facts.private_sources.is_none()
        && facts
            .helper
            .as_ref()
            .is_some_and(|helper| helper.version().at_least(DELTA_HELPER))
        && root.target_dir
            == root
                .manifest
                .sibling(&velnor_model::RepoPath::from_static("target"))
}
