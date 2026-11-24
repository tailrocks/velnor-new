// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{
    CacheKey, CargoCommand, CargoLane, GateNeed, RenderedStep, Setup, Step, StepKind, cargo_step,
    merge_group_skipped, sources_step, trimmed_lane,
};
use velnor_model::{RepoPath, SetupOrder};

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

pub(super) fn trailing_steps(kind: &StepKind, lane: CargoLane) -> Vec<RenderedStep> {
    match kind {
        StepKind::SourcesFetchAll { manifests, .. } => {
            crate::base_steps::sources_fetch_all_rest(manifests, lane)
        }
        StepKind::PrivateFetch {
            filled,
            filled_private,
            ..
        } => crate::private_steps::filled_fetch(filled, *filled_private),
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

fn any_gate(step: &Step, setup: Setup) -> Option<String> {
    let gates: Vec<Vec<String>> = step
        .any_gate
        .iter()
        .map(|gate| crate::gate::gate_parts(gate, setup == Setup::Replanned))
        .collect();
    let first = gates.first()?;
    let shared: Vec<&String> = first
        .iter()
        .filter(|part| gates.iter().all(|parts| parts.contains(part)))
        .collect();
    let owns: Vec<Vec<&String>> = gates
        .iter()
        .map(|parts| parts.iter().filter(|part| !shared.contains(part)).collect())
        .collect();
    let mut terms: Vec<String> = shared.into_iter().cloned().collect();
    if owns.iter().any(Vec::is_empty) {
        return (!terms.is_empty()).then(|| terms.join(" && "));
    }
    let alternatives: Vec<String> = owns
        .iter()
        .map(|own| match own.as_slice() {
            [only] => (*only).clone(),
            _ => format!(
                "({})",
                own.iter()
                    .map(|part| part.as_str())
                    .collect::<Vec<_>>()
                    .join(" && ")
            ),
        })
        .collect();
    terms.push(format!("({})", alternatives.join(" || ")));
    Some(terms.join(" && "))
}

pub(super) fn extra_conditions(step: &Step, setup: Setup, after: &[String]) -> Vec<String> {
    let mut extra = saved_unless_hit(&step.kind);
    extra.extend(crate::private_steps::repository_conditions(step));
    extra.extend(after.iter().map(String::as_str).map(setup_not_failed));
    extra.extend(
        step.hosted_only
            .then(|| crate::gate::NOT_SELF_HOSTED.to_owned()),
    );
    extra.extend(any_gate(step, setup));
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
    if step.setup_order != SetupOrder::AfterSetup {
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
