// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CacheKey, CargoCommand, CargoLane, GateNeed, RenderedStep, Step, StepKind, cargo_step,
    sources_step, trimmed_lane,
};

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
            vec![crate::artifact_steps::group_guard(upload)]
        }
        _ => Vec::new(),
    }
}
