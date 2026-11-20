// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{
    CacheKey, CargoCommand, CargoLane, GateNeed, RenderedStep, Setup, Step, StepKind, cargo_step,
    merge_group_skipped, sources_step, trimmed_lane,
};
use velnor_model::RepoPath;

pub(super) fn saved_unless_hit(kind: &StepKind) -> Vec<String> {
    match kind {
        StepKind::CacheRestore(CacheKey::Delta { root, .. }) => vec![
            format!("steps.delta_base_{}.outcome == 'success'", root.slug()),
            format!(
                "steps.target_{}.outputs.cache-matched-key != ''",
                root.slug()
            ),
        ],
        StepKind::CacheSave(CacheKey::Sources { lane, .. }) => vec![format!(
            "steps.{}.outputs.cache-hit != 'true'",
            sources_step(lane.slug())
        )],
        StepKind::SourcesFetch { lane, position } => {
            let mut terms = vec![format!(
                "steps.{}.outputs.cache-hit != 'true'",
                sources_step(lane)
            )];
            if let Some(previous) = position.checked_sub(1) {
                terms.push(format!(
                    "steps.{}.outcome == 'success'",
                    crate::gate::sources_fetch_step(previous)
                ));
            }
            terms
        }
        StepKind::SourcesFetchAll { lane, .. } => vec![format!(
            "steps.{}.outputs.cache-hit != 'true'",
            sources_step(lane)
        )],
        StepKind::CacheTrim { lane, family } => vec![trimmed_lane(lane, *family)],
        StepKind::PrivateCheck { cached }
        | StepKind::PrivateToken { cached, .. }
        | StepKind::PrivateFetch { cached, .. } => crate::private_steps::unless_hit(cached),
        _ => Vec::new(),
    }
}

pub(super) fn variable(need: GateNeed) -> &'static str {
    match need {
        GateNeed::TestFilter => "TEST_FILTER",
        GateNeed::DoctestsKnown => "DOCTESTS_KNOWN",
        _ => "PACKAGE_ARGS",
    }
}

pub(super) fn rustup_add(kind: &str, items: &[String]) -> RenderedStep {
    RenderedStep {
        name: format!("Add Rust {kind}s"),
        run: Some(format!("rustup {kind} add {}", items.join(" "))),
        ..RenderedStep::default()
    }
}

pub(super) fn extra_conditions(step: &Step, setup: Setup, after: &[String]) -> Vec<String> {
    let mut extra = saved_unless_hit(&step.kind);
    extra.extend(crate::private_steps::repository_conditions(step));
    extra.extend(after.iter().map(String::as_str).map(setup_not_failed));
    extra.extend(
        step.hosted_only
            .then(|| crate::gate::NOT_SELF_HOSTED.to_owned()),
    );
    extra.extend(
        step.skip_when_cold
            .then(|| crate::gate::PREPARE_COLD.to_owned()),
    );
    extra.extend(
        step.skip_after_merge_group
            .as_ref()
            .map(|from| merge_group_skipped(from, setup)),
    );
    extra
}

pub(super) fn leading_steps(step: &Step, audit: bool, lane: CargoLane) -> Vec<RenderedStep> {
    match &step.kind {
        StepKind::Cargo(CargoCommand::Doctest) if audit => vec![cargo_step(
            &CargoCommand::BuildAll,
            step.manifest.as_ref(),
            true,
            lane,
            None,
        )],
        StepKind::UploadGroupArtifact(upload) => {
            vec![crate::artifact_steps::group_stage(upload)]
        }
        _ => Vec::new(),
    }
}

pub(super) fn root_key(step: &Step) -> Option<RepoPath> {
    step.directory
        .clone()
        .or_else(|| step.manifest.as_ref().and_then(RepoPath::parent))
}

pub(super) fn after_setup(
    step: &Step,
    setups: &BTreeMap<Option<RepoPath>, Vec<String>>,
) -> Vec<String> {
    if !step.after_setup {
        return Vec::new();
    }
    if matches!(step.kind, StepKind::TestSetup { .. }) {
        return setups.get(&root_key(step)).cloned().unwrap_or_default();
    }
    setups.values().flatten().cloned().collect()
}

pub(super) fn setup_not_failed(id: &str) -> String {
    format!("steps.{id}.outcome != 'failure' && steps.{id}.outcome != 'cancelled'")
}
