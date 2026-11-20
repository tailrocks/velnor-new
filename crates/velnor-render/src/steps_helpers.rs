// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CacheKey, GateNeed, RenderedStep, Setup, Step, StepKind, merge_group_skipped, sources_step,
    trimmed_lane,
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

pub(super) fn extra_conditions(step: &Step, setup: Setup) -> Vec<String> {
    let mut extra = saved_unless_hit(&step.kind);
    extra.extend(crate::private_steps::repository_conditions(step));
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
