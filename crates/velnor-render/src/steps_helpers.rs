// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CacheKey, GateNeed, Job, MERGE_GROUP_SKIP_OUTPUT, MERGE_GROUP_STEP, RenderedStep, StepKind,
    TESTS_BY_PACKAGE_OUTPUT, TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT,
    sources_step, trimmed_lane,
};
use crate::cargo::reports_tests;
use crate::tests::Tests;

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

pub(super) fn test_outputs<'a>(
    job: &'a Job,
    tests: &'a Tests,
) -> impl Iterator<Item = (String, String)> + 'a {
    job.steps
        .iter()
        .any(|step| matches!(&step.kind, StepKind::Cargo(command) if reports_tests(command)))
        .then(|| {
            [
                Some(TESTS_RUN_OUTPUT),
                Some(TESTS_LISTED_OUTPUT),
                job.reports_ignored.then_some(TESTS_IGNORED_OUTPUT),
                Some(TESTS_BY_PACKAGE_OUTPUT),
            ]
        })
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|name| tests.outputs(name))
}

pub(super) fn merge_group_output(job: &Job) -> Option<(String, String)> {
    job.steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::MergeGroupPassed { .. }))
        .then(|| {
            (
                MERGE_GROUP_SKIP_OUTPUT.to_owned(),
                format!("${{{{ steps.{MERGE_GROUP_STEP}.outputs.skip }}}}"),
            )
        })
}
